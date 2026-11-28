//! The secretary: what happens to a recording without anyone asking.
//!
//! A recording arrives one of three ways: recorded here, brought in by hand,
//! or found in a watched folder. From then on it needs nobody. It waits its
//! turn, is transcribed, has its minutes written, and the user is told once.
//! When a step fails it is tried again later, and whatever was achieved (the
//! original, the transcript) is kept.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use super::audio_in;
use super::calendar;
use super::chunks::RATE;
use super::config::{Config, SpeechModelNow, WatchedFolder};
use super::edits;
use super::evidence::{self, Evidence};
use super::google::Google;
use super::llm::{self, Provider, Ways};
use super::minutes;
use super::phone::{self, Arrival, Phone, PhoneStatus};
use super::pipeline::{self, Monitor, Request, Stage};
use super::power::KeepAwake;
use super::recorder;
use super::store::{self, EventRef, Found, Library, Meeting, MeetingSummary, Origin, Status};
use super::templates;
use super::transcript::{LineSpeaker, NameScope, Speaker};
use super::upload::{self, Backlog, DriveStatus, Uploads};
use super::vocab::Vocabulary;
use super::watch::{self, Ledger, Look, Settling};
// The parts below reach these as `super::vocab` and so on, as they did when
// they were in this file.
use super::{
    ask, banner, dictation, documents, feedback, mac_calendar, secrets, status, transcript, vocab,
};

mod changes;
mod library;
mod linked;
mod queue;
mod recording;
mod schedule;
mod tray;

use changes::*;
pub use changes::{tidy_names, KeptTask, Naming};
pub use library::to_trash;
pub use schedule::{CalendarCheck, CalendarStatus};
pub use tray::{tray_action, tray_menu};

/// How long to wait before trying a failed step again: one minute, then
/// longer each time, half an hour at most.
const BACKOFF_SECS: [i64; 6] = [60, 120, 300, 600, 1_200, 1_800];
/// A recording that cannot be transcribed after this many tries is failed.
const MAX_TRANSCRIBE_ATTEMPTS: u32 = 4;
/// Minutes that cannot be written after this many tries wait for the user
/// or the next start; the transcript is kept meanwhile.
const MAX_MINUTES_ATTEMPTS: u32 = 12;
/// How often minutes with no way to be written look again for one (a
/// sign-in, a saved key, a model on this computer). Waiting does not use up
/// their tries.
const LOOK_FOR_A_WAY_MS: i64 = 5 * 60_000;
/// How often watched folders are looked at.
const WATCH_EVERY: Duration = Duration::from_secs(10);

/// A meeting was added or something about it changed.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct MeetingChanged {
    pub id: String,
}

/// A recording started or ended.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct RecordingChanged {
    pub recording: bool,
}

/// A calendar event is about to start (or has just started) and nothing is
/// being recorded: the moment to offer recording it.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct MeetingStarting {
    pub event: calendar::Event,
    /// Minutes until it starts; negative once it has.
    pub minutes_until: i32,
}

/// What the small window above everything else shows.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Banner {
    /// A calendar event is starting: offer to record it.
    Starting { event: calendar::Event },
    /// A meeting's minutes are ready.
    Ready {
        id: String,
        title: String,
        actions: u32,
        to_check: u32,
    },
}

/// The banner has something new to show, or nothing any more.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct BannerChanged {}

/// The dictation choices were saved (from the settings or the tray), so a
/// screen showing them reads them again.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct DictationChanged {}

/// The main window should show this meeting.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct OpenMeeting {
    pub id: String,
}

/// A calendar event of today, for the band at the top.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct Upcoming {
    pub event: calendar::Event,
    /// The user turned its reminder off.
    pub muted: bool,
}

/// What the user may tell the app about a meeting. A field left out is not
/// changed.
#[derive(Clone, Debug, Default, Serialize, Deserialize, Type)]
pub struct MeetingInfo {
    pub title: Option<String>,
    pub template_id: Option<String>,
    pub customer: Option<String>,
    pub people: Option<Vec<String>>,
    /// The place or site.
    pub place: Option<String>,
    pub note: Option<String>,
    /// Names and words for this meeting only.
    pub terms: Option<Vec<String>>,
}

/// What fixing a misheard word did in a meeting.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct WordFixOutcome {
    /// How many places were fixed.
    pub places: u32,
    /// The lines as they were, to put back if the user undoes the fix.
    pub before: Vec<super::transcript::Segment>,
    /// The vocabulary learnt the misheard words, for meetings from now on.
    pub remembered: bool,
}

/// The recording in progress, as the screen shows it.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct RecordingState {
    pub id: String,
    pub started_at_ms: i64,
    pub elapsed_ms: u64,
    /// How loud the microphone is right now, 0 to 1.
    pub level: f32,
    /// When "중요 표시" was pressed, in milliseconds into the recording.
    pub marks_ms: Vec<u64>,
    pub device: String,
    /// No sound at all is arriving: the microphone is muted or not allowed.
    pub no_sound: bool,
    /// The microphone went away (a lid closed, a headset dropped) and is
    /// being opened again. The meeting stays open meanwhile.
    pub reconnecting: bool,
}

/// What bringing a file in led to.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct Imported {
    pub id: String,
    /// The same recording was already here; nothing was added.
    pub duplicate: bool,
    pub sha256: String,
}

struct Slot {
    folder: PathBuf,
    summary: MeetingSummary,
    queued_at_ms: i64,
    retry_at_ms: i64,
    /// Not tried again until the app restarts or the user asks.
    parked: bool,
}

/// A meeting moved to the trash a moment ago.
struct Trashed {
    id: String,
    /// Its Drive folder went to Drive's trash with it (decision 5 of #67).
    from_drive: bool,
    folder: PathBuf,
    sha256: String,
    /// Where it lies in the trash, when the system says.
    in_trash: Option<PathBuf>,
    at: Instant,
}

struct Active {
    id: String,
    folder: PathBuf,
    session: recorder::Session,
    started_at_ms: i64,
    marks_ms: Vec<u64>,
    _awake: KeepAwake,
}

/// Why a step could not be done.
struct Trouble {
    code: &'static str,
    detail: String,
    /// Whether trying again could help.
    retry: bool,
}

impl Trouble {
    fn new(code: &'static str, error: anyhow::Error, retry: bool) -> Self {
        Self {
            code,
            detail: format!("{error:#}"),
            retry,
        }
    }
}

pub struct Secretary {
    app: AppHandle,
    data_dir: PathBuf,
    config: Mutex<Config>,
    slots: Mutex<HashMap<String, Slot>>,
    ledger: Mutex<Ledger>,
    /// The meeting last moved to the trash, which can be brought back for a
    /// minute from the screen.
    trashed: Mutex<Option<Trashed>>,
    /// Held while a meeting is read, changed and written back by the user
    /// or by a change from the phone, so neither writes over the other.
    edit_lock: Mutex<()>,
    /// Set when there may be work; the queue sleeps on it.
    wake: (Mutex<bool>, Condvar),
    recording: Mutex<Option<Active>>,
    /// The user's calendars as iCalendar text, as last read.
    calendars: Mutex<Vec<String>>,
    /// Set to have the calendars read again now.
    calendars_stale: std::sync::atomic::AtomicBool,
    /// The calendars are being read right now.
    calendar_reading: std::sync::atomic::AtomicBool,
    /// How the calendars answered when last read.
    calendar_status: Mutex<CalendarStatus>,
    /// What the banner window shows, if anything.
    banner: Mutex<Option<Banner>>,
    /// A run from the command line: no notifications, tray or shortcuts.
    quiet: bool,
    /// The phone's recordings in the user's Google Drive.
    phone: Phone,
    /// Meetings put in the same Drive folder, for the phone.
    uploads: Uploads,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Clears a "being done" flag when it goes out of scope, a panic included.
struct ClearOnDrop<'a>(&'a std::sync::atomic::AtomicBool);

impl Drop for ClearOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Transcribes with the speech model to use now (`run` does the work with
/// a model id): the chosen one, or, while it is not here, the best stand-in
/// that is, and then the outcome's stats name the chosen one. With no
/// usable model here it tries the chosen one, whose own error says why.
fn transcribe_now<E>(
    config: &Config,
    here: impl Fn(&str) -> bool,
    run: impl FnOnce(&str) -> Result<Option<pipeline::Outcome>, E>,
) -> Result<Option<pipeline::Outcome>, E> {
    let speech = config
        .speech_model_now(here)
        .unwrap_or_else(|| SpeechModelNow {
            model: config.speech_model().to_string(),
            standing_in_for: None,
        });
    let mut outcome = run(&speech.model)?;
    if let Some(outcome) = outcome.as_mut() {
        outcome.stats.stood_in_for = speech.standing_in_for;
    }
    Ok(outcome)
}

fn backoff_ms(attempts: u32) -> i64 {
    let index = (attempts.saturating_sub(1) as usize).min(BACKOFF_SECS.len() - 1);
    BACKOFF_SECS[index] * 1_000
}

impl Secretary {
    /// Reads the library and gets ready; nothing runs until [`start`].
    ///
    /// [`start`]: Secretary::start
    pub fn new(app: &AppHandle, data_dir: PathBuf, config: Config, quiet: bool) -> Arc<Self> {
        let google = Arc::new(Google::new(&data_dir));
        let secretary = Arc::new(Self {
            app: app.clone(),
            ledger: Mutex::new(Ledger::load(&data_dir)),
            phone: Phone::new(&data_dir, google.clone()),
            uploads: Uploads::new(&data_dir, google),
            trashed: Mutex::new(None),
            edit_lock: Mutex::new(()),
            data_dir,
            config: Mutex::new(config),
            slots: Mutex::new(HashMap::new()),
            wake: (Mutex::new(true), Condvar::new()),
            recording: Mutex::new(None),
            calendars: Mutex::new(Vec::new()),
            calendars_stale: std::sync::atomic::AtomicBool::new(true),
            calendar_reading: std::sync::atomic::AtomicBool::new(false),
            calendar_status: Mutex::new(CalendarStatus::default()),
            banner: Mutex::new(None),
            quiet,
        });
        secretary.read_library();
        secretary
    }

    /// The secretary of a running app.
    pub fn of(app: &AppHandle) -> Option<Arc<Self>> {
        app.try_state::<Arc<Secretary>>()
            .map(|state| state.inner().clone())
    }

    /// Starts the queue and the folder watcher, and picks up what a crash or
    /// a restart left unfinished.
    pub fn start(self: &Arc<Self>) {
        self.recover();
        for (name, run) in [
            ("seogi-queue", Self::work_loop as fn(Arc<Self>)),
            ("seogi-watch", Self::watch_loop as fn(Arc<Self>)),
            ("seogi-calendar", Self::calendar_loop as fn(Arc<Self>)),
        ] {
            let me = self.clone();
            if let Err(e) = std::thread::Builder::new()
                .name(name.into())
                .spawn(move || run(me))
            {
                error!("Could not start {name}: {e}");
            }
        }
        if !self.quiet {
            let me = self.clone();
            if let Err(e) = std::thread::Builder::new()
                .name("seogi-phone".into())
                .spawn(move || me.phone_loop())
            {
                error!("Could not start seogi-phone: {e}");
            }
            let me = self.clone();
            if let Err(e) = std::thread::Builder::new()
                .name("seogi-drive".into())
                .spawn(move || me.drive_loop())
            {
                error!("Could not start seogi-drive: {e}");
            }
            self.register_shortcuts();
            super::banner::prepare(&self.app);
        }
        // For looking at the banner while working on it:
        // SEOGI_TEST_BANNER=starting or ready shows one a moment after the
        // app starts.
        if let Ok(kind) = std::env::var("SEOGI_TEST_BANNER") {
            let me = self.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(4));
                let now = now_ms();
                me.show_banner(if kind == "ready" {
                    Banner::Ready {
                        id: me
                            .meetings()
                            .first()
                            .map_or(String::new(), |m| m.id.clone()),
                        title: "[미팅] 새봄타일 / 욕실 타일 샘플".to_string(),
                        actions: 4,
                        to_check: 1,
                    }
                } else {
                    Banner::Starting {
                        event: calendar::Event {
                            uid: "test".to_string(),
                            title: "[회의실A] 김서윤 / 초도 상담".to_string(),
                            start_ms: now,
                            end_ms: now + 3_600_000,
                            location: String::new(),
                            attendees: Vec::new(),
                        },
                    }
                });
            });
        }
    }

    // ----- settings ---------------------------------------------------------

    pub fn config(&self) -> Config {
        lock(&self.config).clone()
    }

    /// The language recordings are in, which dictation listens for too.
    pub fn language(&self) -> String {
        lock(&self.config).language.clone()
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Replaces the settings and saves them. A new library folder is read at
    /// once; meetings in the old one stay where they are.
    pub fn set_config(&self, mut config: Config) -> Result<()> {
        let library_moved = {
            let mut current = lock(&self.config);
            config.keep_dictation_of(&current);
            config.save(&self.data_dir)?;
            let moved = current.library() != config.library();
            *current = config;
            moved
        };
        self.calendars_stale
            .store(true, std::sync::atomic::Ordering::Release);
        if library_moved {
            self.read_library();
        }
        self.minutes_may_go();
        self.wake();
        // A meeting waiting for "이 컴퓨터에서 녹음한 회의도 올리기" may go now.
        self.uploads.wake_up();
        Ok(())
    }

    /// The one way the dictation settings in `seogi.json` change: `change`
    /// is applied to what is saved now, under the lock, and the result is
    /// saved. So the settings screen and the tray never undo each other.
    /// Nothing else is read again.
    pub fn update_dictation(
        &self,
        change: impl FnOnce(&mut super::dictation::DictationConfig),
    ) -> Result<()> {
        let mut current = lock(&self.config);
        let mut config = current.clone();
        change(&mut config.dictation);
        config.save(&self.data_dir)?;
        *current = config;
        Ok(())
    }

    fn library(&self) -> Library {
        Library::new(lock(&self.config).library())
    }

    /// Starts watching `folder` for new recordings, making it if need be.
    /// Recordings already in it are left alone.
    pub fn watch_folder(&self, folder: &Path) -> Result<()> {
        std::fs::create_dir_all(folder)
            .with_context(|| format!("could not create {}", folder.display()))?;
        let mut config = self.config();
        let path = folder.display().to_string();
        match config.watched.iter_mut().find(|w| w.path == path) {
            Some(watched) => watched.enabled = true,
            None => config.watched.push(WatchedFolder {
                path,
                enabled: true,
                template_id: String::new(),
                since_ms: now_ms(),
            }),
        }
        self.set_config(config)?;
        // What is in it now is noted at once, so that a file dropped in a
        // moment later is new.
        self.note_watched_folders();
        Ok(())
    }

    /// Notes, for each watched folder not yet looked at, the recordings it
    /// already holds: they are left alone.
    fn note_watched_folders(&self) {
        for watched in self.config().watched.iter().filter(|w| w.enabled) {
            let folder = Path::new(&watched.path);
            let mut ledger = lock(&self.ledger);
            if let watch::Noted::Now(left) = ledger.note_what_is_there(folder, watched.since_ms) {
                info!(
                    "Watching {}: {left} recording(s) already there are left alone",
                    folder.display()
                );
                if let Err(e) = ledger.save(&self.data_dir) {
                    warn!("Could not save the ledger: {e:#}");
                }
            }
        }
    }

    // ----- telling the user -------------------------------------------------

    fn notify(&self, title: &str, body: &str) {
        if self.quiet {
            return;
        }
        use tauri_plugin_notification::NotificationExt;
        if let Err(e) = self
            .app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
        {
            warn!("Could not show a notification: {e}");
        }
    }
}

fn shown_title(meeting: &Meeting) -> String {
    if meeting.title.is_empty() {
        meeting.recorded_at()
    } else {
        meeting.title.clone()
    }
}

fn is_korean(app: &AppHandle) -> bool {
    crate::settings::get_settings(app)
        .app_language
        .starts_with("ko")
}

/// The Korean text, or the English one when the app is not set to Korean.
fn say<'a>(app: &AppHandle, korean: &'a str, english: &'a str) -> &'a str {
    if is_korean(app) {
        korean
    } else {
        english
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_that_panics_still_lets_the_calendars_be_read_again() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let reading = AtomicBool::new(true);
        let read = std::panic::catch_unwind(|| {
            let _done = ClearOnDrop(&reading);
            panic!("the read broke");
        });
        assert!(read.is_err());
        assert!(!reading.load(Ordering::Acquire));
    }

    /// The path a recording takes in `Secretary::transcribe`: the chosen
    /// model when it is here; while it is not, the best stand-in, named in
    /// the meeting's stats; with none here, the chosen one (whose error
    /// says why; `process` keeps the recording waiting before that).
    #[test]
    fn a_recording_is_transcribed_by_a_stand_in_while_its_model_is_missing() {
        use super::super::config::{FUN_ASR_NANO, QWEN3_ASR_1_7B, WHISPER_TURBO};
        use super::super::transcript::Transcript;
        let run = |model: &str| -> Result<Option<pipeline::Outcome>, ()> {
            Ok(Some(pipeline::Outcome {
                transcript: Transcript {
                    language: "ko".into(),
                    model: model.to_string(),
                    segments: Vec::new(),
                    speakers: Vec::new(),
                    corrections: Vec::new(),
                },
                stats: pipeline::Stats::default(),
            }))
        };
        let with = |ids: &'static [&'static str]| move |id: &str| ids.contains(&id);
        let config = Config::default();

        let done = transcribe_now(&config, with(&[QWEN3_ASR_1_7B, WHISPER_TURBO]), run)
            .unwrap()
            .unwrap();
        assert_eq!(done.transcript.model, QWEN3_ASR_1_7B);
        assert_eq!(done.stats.stood_in_for, None);

        let done = transcribe_now(&config, with(&[WHISPER_TURBO, FUN_ASR_NANO]), run)
            .unwrap()
            .unwrap();
        assert_eq!(done.transcript.model, WHISPER_TURBO);
        assert_eq!(done.stats.stood_in_for.as_deref(), Some(QWEN3_ASR_1_7B));

        // A model the user picked is used when it is here, and nothing is
        // said to stand in.
        let picked = Config {
            speech_model: WHISPER_TURBO.to_string(),
            ..Config::default()
        };
        let done = transcribe_now(&picked, with(&[WHISPER_TURBO]), run)
            .unwrap()
            .unwrap();
        assert_eq!(done.transcript.model, WHISPER_TURBO);
        assert_eq!(done.stats.stood_in_for, None);

        // Nothing here: the chosen model is tried and its error comes back.
        let mut tried = String::new();
        let failed = transcribe_now(&config, with(&[]), |model| {
            tried = model.to_string();
            Err::<Option<pipeline::Outcome>, _>("not downloaded")
        });
        assert_eq!(failed.err(), Some("not downloaded"));
        assert_eq!(tried, QWEN3_ASR_1_7B);
        assert!(config.speech_model_now(with(&[])).is_none());
    }

    #[test]
    fn retries_wait_longer_each_time_up_to_half_an_hour() {
        assert_eq!(backoff_ms(1), 60_000);
        assert_eq!(backoff_ms(2), 120_000);
        assert_eq!(backoff_ms(6), 1_800_000);
        assert_eq!(backoff_ms(40), 1_800_000);
        assert_eq!(backoff_ms(0), 60_000);
    }
}
