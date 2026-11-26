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
use super::evidence::{self, Evidence};
use super::llm::{self, Provider, Ways};
use super::minutes;
use super::pipeline::{self, Monitor, Request, Stage};
use super::power::KeepAwake;
use super::recorder;
use super::store::{self, EventRef, Found, Library, Meeting, MeetingSummary, Origin, Status};
use super::templates;
use super::transcript::{LineSpeaker, NameScope, Speaker};
use super::vocab::Vocabulary;
use super::watch::{self, Ledger, Look, Settling};

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

/// Whether a calendar address works, and what it holds for today.
#[derive(Clone, Debug, Default, Serialize, Deserialize, Type)]
pub struct CalendarCheck {
    pub ok: bool,
    /// Events today.
    pub today: u32,
    pub error: String,
    /// Why it could not be read, as a code the screen has words for (see
    /// [`calendar::trouble`]); empty when it was read.
    pub reason: String,
    /// The address as it should be saved: what was pasted, tidied.
    pub address: String,
    /// The next event from now, within the coming weeks.
    pub next: Option<calendar::Event>,
    /// When it was read (milliseconds since 1970 UTC).
    pub read_at_ms: i64,
}

/// `public_closed` for a public address that answered that there is
/// nothing there: a calendar that is not shared with everyone has none.
/// Otherwise `reason` as it is.
fn public_closed(address: &str, reason: &str) -> String {
    if reason == "refused" && address.contains("/public/") {
        "public_closed".to_string()
    } else {
        reason.to_string()
    }
}

/// How the calendars answered when last read.
#[derive(Clone, Debug, Default, Serialize, Deserialize, Type)]
pub struct CalendarStatus {
    /// The Calendar app's calendars, when some are chosen.
    pub mac: Option<CalendarCheck>,
    /// Each secret address, in the order of the settings.
    pub addresses: Vec<CalendarCheck>,
}

impl CalendarCheck {
    /// What reading `address` gave: its text, or why there is none.
    fn of(address: String, read: Result<&str, String>, now_ms: i64) -> Self {
        use chrono::{Local, TimeZone};
        let text = match read {
            Ok(text) if text.contains("BEGIN:VCALENDAR") => text,
            Ok(_) => {
                return Self {
                    reason: calendar::trouble("not a calendar").to_string(),
                    error: "not a calendar".to_string(),
                    address,
                    read_at_ms: now_ms,
                    ..Self::default()
                }
            }
            Err(error) => {
                return Self {
                    reason: calendar::trouble(&error).to_string(),
                    error,
                    address,
                    read_at_ms: now_ms,
                    ..Self::default()
                }
            }
        };
        let now = Local
            .timestamp_millis_opt(now_ms)
            .single()
            .unwrap_or_else(Local::now);
        let midnight = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .and_then(|start| Local.from_local_datetime(&start).earliest())
            .unwrap_or(now);
        const DAY_MS: i64 = 24 * 3_600_000;
        let start = midnight.timestamp_millis();
        let today = calendar::events_between(text, start, start + DAY_MS - 1);
        let next = calendar::events_between(text, now_ms, now_ms + 60 * DAY_MS)
            .into_iter()
            .filter(|event| event.start_ms >= now_ms)
            .min_by_key(|event| event.start_ms);
        Self {
            ok: true,
            today: today.len() as u32,
            address,
            next,
            read_at_ms: now_ms,
            ..Self::default()
        }
    }
}

/// Today's events (those starting before `midnight`), or, when none of them
/// is wanted, the first wanted event after it.
fn upcoming_from(events: Vec<calendar::Event>, midnight: i64, muted: &[String]) -> Vec<Upcoming> {
    let wanted = |event: &calendar::Event| !muted.contains(&event.uid);
    let today: Vec<Upcoming> = events
        .iter()
        .filter(|event| event.start_ms < midnight)
        .map(|event| Upcoming {
            muted: !wanted(event),
            event: event.clone(),
        })
        .collect();
    if today.iter().any(|upcoming| !upcoming.muted) {
        return today;
    }
    let mut shown = today;
    if let Some(next) = events
        .into_iter()
        .find(|event| event.start_ms >= midnight && wanted(event))
    {
        shown.push(Upcoming {
            muted: false,
            event: next,
        });
    }
    shown
}

#[cfg(test)]
mod upcoming_tests {
    use super::*;

    fn event(uid: &str, start_ms: i64) -> calendar::Event {
        calendar::Event {
            uid: uid.to_string(),
            title: uid.to_string(),
            start_ms,
            end_ms: start_ms + 3_600_000,
            location: String::new(),
            attendees: Vec::new(),
        }
    }

    #[test]
    fn tomorrows_first_event_shows_when_today_has_none_left() {
        let midnight = 1_000_000;
        let shown = upcoming_from(
            vec![
                event("tomorrow", midnight + 10),
                event("later", midnight + 20),
            ],
            midnight,
            &[],
        );
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].event.uid, "tomorrow");
    }

    #[test]
    fn todays_events_are_all_there_is_while_one_is_left() {
        let midnight = 1_000_000;
        let shown = upcoming_from(
            vec![
                event("today", midnight - 10),
                event("tomorrow", midnight + 10),
            ],
            midnight,
            &[],
        );
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].event.uid, "today");
    }

    #[test]
    fn a_muted_event_does_not_hide_the_next_one() {
        let midnight = 1_000_000;
        let shown = upcoming_from(
            vec![
                event("muted", midnight - 10),
                event("tomorrow", midnight + 10),
            ],
            midnight,
            &["muted".to_string()],
        );
        assert_eq!(shown.len(), 2);
        assert!(shown[0].muted);
        assert_eq!(shown[1].event.uid, "tomorrow");
    }
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
    /// Set when there may be work; the queue sleeps on it.
    wake: (Mutex<bool>, Condvar),
    recording: Mutex<Option<Active>>,
    /// The user's calendars as iCalendar text, as last read.
    calendars: Mutex<Vec<String>>,
    /// Set to have the calendars read again now.
    calendars_stale: std::sync::atomic::AtomicBool,
    /// How the calendars answered when last read.
    calendar_status: Mutex<CalendarStatus>,
    /// What the banner window shows, if anything.
    banner: Mutex<Option<Banner>>,
    /// A run from the command line: no notifications, tray or shortcuts.
    quiet: bool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
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
        let secretary = Arc::new(Self {
            app: app.clone(),
            ledger: Mutex::new(Ledger::load(&data_dir)),
            trashed: Mutex::new(None),
            data_dir,
            config: Mutex::new(config),
            slots: Mutex::new(HashMap::new()),
            wake: (Mutex::new(true), Condvar::new()),
            recording: Mutex::new(None),
            calendars: Mutex::new(Vec::new()),
            calendars_stale: std::sync::atomic::AtomicBool::new(true),
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

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Replaces the settings and saves them. A new library folder is read at
    /// once; meetings in the old one stay where they are.
    pub fn set_config(&self, mut config: Config) -> Result<()> {
        let library_moved = {
            let mut current = lock(&self.config);
            // Dictation has its own command (and the floating key moves on
            // its own); a screen holding older settings must not undo it.
            config.dictation = current.dictation.clone();
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
        Ok(())
    }

    /// Replaces only the dictation settings and saves them. Nothing else
    /// is read again: the floating key saves its place here on every drag.
    pub fn set_dictation(&self, dictation: super::dictation::DictationConfig) -> Result<()> {
        let mut current = lock(&self.config);
        let mut config = current.clone();
        config.dictation = dictation;
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

    // ----- the library ------------------------------------------------------

    fn read_library(&self) {
        let found = self.library().scan();
        let mut slots = lock(&self.slots);
        let mut ledger = lock(&self.ledger);
        slots.clear();
        for (meeting, folder) in found {
            if !meeting.sha256.is_empty() {
                ledger
                    .hashes
                    .entry(meeting.sha256.clone())
                    .or_insert_with(|| meeting.id.clone());
            }
            slots.insert(meeting.id.clone(), Self::slot(&meeting, &folder));
        }
    }

    /// Takes in meeting folders that appeared in the library since it was
    /// read: put there by another copy of the app, or copied in by hand. A
    /// meeting already known (by its id or its folder) is left as it is.
    /// One that is not finished may still be in another program's hands,
    /// so it waits for the user's "try again" instead of being started.
    fn pick_up_library(&self) {
        let found = self.library().scan();
        let mut added: Vec<String> = Vec::new();
        {
            let mut slots = lock(&self.slots);
            let mut ledger = lock(&self.ledger);
            for (meeting, folder) in found {
                if slots.contains_key(&meeting.id)
                    || slots.values().any(|slot| slot.folder == folder)
                {
                    continue;
                }
                // A record written moments ago is one being made right now,
                // by this app or another: it is looked at again later.
                let settled = std::fs::metadata(folder.join(store::RECORD_NAME))
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|at| at.elapsed().ok())
                    .is_some_and(|age| age >= Duration::from_secs(60));
                if !settled {
                    continue;
                }
                if !meeting.sha256.is_empty() {
                    ledger
                        .hashes
                        .entry(meeting.sha256.clone())
                        .or_insert_with(|| meeting.id.clone());
                }
                let mut slot = Self::slot(&meeting, &folder);
                slot.parked = !matches!(meeting.status, Status::Done | Status::Failed);
                info!("Found {} in the library", meeting.id);
                added.push(meeting.id.clone());
                slots.insert(meeting.id.clone(), slot);
            }
        }
        for id in added {
            self.changed(&id);
        }
    }

    fn slot(meeting: &Meeting, folder: &Path) -> Slot {
        Slot {
            folder: folder.to_path_buf(),
            summary: meeting.summary(folder),
            queued_at_ms: meeting.queued_at_ms,
            retry_at_ms: meeting.retry_at_ms,
            parked: false,
        }
    }

    /// Every meeting, newest first.
    pub fn meetings(&self) -> Vec<MeetingSummary> {
        let mut meetings: Vec<MeetingSummary> = lock(&self.slots)
            .values()
            .map(|slot| slot.summary.clone())
            .collect();
        meetings.sort_by_key(|meeting| std::cmp::Reverse(meeting.started_at_ms));
        meetings
    }

    /// The meetings whose title, minutes or transcript have `query` in them.
    pub fn search(&self, query: &str) -> Vec<Found> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        let folders: Vec<PathBuf> = lock(&self.slots)
            .values()
            .map(|slot| slot.folder.clone())
            .collect();
        folders
            .into_iter()
            .filter_map(|folder| Library::read(&folder).ok()?.found(query))
            .collect()
    }

    pub fn folder(&self, id: &str) -> Result<PathBuf> {
        lock(&self.slots)
            .get(id)
            .map(|slot| slot.folder.clone())
            .ok_or_else(|| anyhow!("there is no meeting {id}"))
    }

    pub fn meeting(&self, id: &str) -> Result<Meeting> {
        Library::read(&self.folder(id)?)
    }

    /// Writes the meeting to its folder and tells the screen.
    fn save(&self, meeting: &Meeting, folder: &Path) -> Result<()> {
        Library::save(meeting, folder)?;
        let mut slot = Self::slot(meeting, folder);
        {
            let mut slots = lock(&self.slots);
            if let Some(old) = slots.get(&meeting.id) {
                slot.parked = old.parked;
            }
            slots.insert(meeting.id.clone(), slot);
        }
        self.changed(&meeting.id);
        Ok(())
    }

    fn changed(&self, id: &str) {
        let _ = MeetingChanged { id: id.to_string() }.emit(&self.app);
    }

    fn progress(&self, id: &str, stage: &str, percent: u8) {
        let moved = match lock(&self.slots).get_mut(id) {
            Some(slot) if slot.summary.stage != stage || slot.summary.percent != percent => {
                slot.summary.stage = stage.to_string();
                slot.summary.percent = percent;
                true
            }
            _ => false,
        };
        if moved {
            self.changed(id);
        }
    }

    /// Gives a speaker a name. The transcript and the minutes document are
    /// written again; the recording is not touched.
    pub fn rename_speaker(&self, id: &str, speaker: u32, name: &str) -> Result<()> {
        self.name_speaker(id, speaker, name, NameScope::All, 0, 0)
    }

    /// Names a speaker in the transcript: throughout, from a moment on, or
    /// in one stretch (see [`NameScope`]). Tasks given to the speaker's old
    /// name in that part follow the person; the minutes are not written
    /// again.
    pub fn name_speaker(
        &self,
        id: &str,
        speaker: u32,
        name: &str,
        scope: NameScope,
        from_ms: u64,
        to_ms: u64,
    ) -> Result<()> {
        let (mut meeting, folder) = self.editable(id)?;
        let transcript = meeting
            .transcript
            .as_mut()
            .ok_or_else(|| anyhow!("it has not been transcribed yet"))?;
        let (old, from, to) = transcript
            .name_speaker(speaker, name, scope, from_ms, to_ms)
            .ok_or_else(|| anyhow!("there is no speaker {speaker}, or no name"))?;
        let name = name.trim().to_string();
        for minutes in &mut meeting.minutes {
            for section in &mut minutes.sections {
                for item in &mut section.entries {
                    let at = item.sources.first().map_or(0, |source| source.ms);
                    if item.owner == old && (from..to).contains(&at) {
                        item.owner = name.clone();
                    }
                }
            }
        }
        self.save(&meeting, &folder)
    }

    /// Puts a naming back: the speakers, who said each line and who was
    /// given each task, as the screen kept them just before it named
    /// someone (`kept`). Refused, changing nothing, unless the meeting is
    /// still as the screen saw it just after (`now`): any later change of a
    /// name, or minutes written again, keeps it as it is.
    pub fn restore_naming(&self, id: &str, kept: &Naming, now: &Naming) -> Result<()> {
        let (mut meeting, folder) = self.editable(id)?;
        put_back_naming(&mut meeting, kept, now)?;
        self.save(&meeting, &folder)
    }

    // ----- arriving ---------------------------------------------------------

    /// Brings a recording file into the library and queues it. The file is
    /// copied; the one given is left exactly as it was. A recording already
    /// here (the same bytes, whatever its name) is not added again.
    pub fn import(&self, source: &Path, origin: Origin, template_id: &str) -> Result<Imported> {
        if !source.is_file() {
            bail!("{} is not a file", source.display());
        }
        let sha256 = evidence::sha256_file(source)?;
        {
            let mut ledger = lock(&self.ledger);
            if ledger.deleted.contains(&sha256) {
                if origin == Origin::Watched {
                    // Its meeting went to the trash: the copy in the watched
                    // folder is left alone and not brought in again.
                    return Ok(Imported {
                        id: String::new(),
                        duplicate: true,
                        sha256,
                    });
                }
                // Brought in by hand: the user wants it back.
                ledger.deleted.remove(&sha256);
                if let Err(e) = ledger.save(&self.data_dir) {
                    warn!("Could not save the ledger: {e:#}");
                }
            }
        }
        let known = lock(&self.ledger).hashes.get(&sha256).cloned();
        if let Some(id) = known {
            if lock(&self.slots).contains_key(&id) {
                return Ok(Imported {
                    id,
                    duplicate: true,
                    sha256,
                });
            }
        }

        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("{} has no usable name", source.display()))?;
        let stem = source
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        // When it was recorded: the time in its name if a recorder put one
        // there, else the file's own time, which is when the recording ended
        // and is corrected by its length once that is known.
        let modified_ms = Look::at(source).map_or_else(now_ms, |look| look.modified_ms);
        // A time the recorder wrote inside the file is the next best: it is
        // still true when the file was uploaded or renamed hours later.
        let (started_at_ms, time_source) = match store::time_in_name(&name) {
            Some(at) => (at, "file_name"),
            None => match evidence::created_in_file(source) {
                Some(at) if at > 946_684_800_000 && at <= modified_ms + 86_400_000 => {
                    (at, "file_metadata")
                }
                _ => (modified_ms, "file_modified"),
            },
        };
        let title = store::title_in_name(stem);

        let folder = self.library().create_folder(started_at_ms, &title)?;
        let copy = folder.join(&name);
        let copied = std::fs::copy(source, &copy)
            .with_context(|| format!("could not copy {}", source.display()))
            .and_then(|_| evidence::sha256_file(&copy))
            .and_then(|copied| {
                if copied == sha256 {
                    Ok(())
                } else {
                    Err(anyhow!(
                        "{} changed while it was being copied",
                        source.display()
                    ))
                }
            });
        if let Err(e) = copied {
            // Only the half-made copy is removed; the file given is untouched.
            let _ = std::fs::remove_file(&copy);
            let _ = std::fs::remove_dir(&folder);
            return Err(e);
        }

        let mut meeting = Meeting::new(started_at_ms, origin, &name);
        meeting.title_is_auto = title.is_empty();
        meeting.title = title;
        meeting.time_source = time_source.to_string();
        meeting.source_path = std::path::absolute(source)
            .unwrap_or_else(|_| source.to_path_buf())
            .display()
            .to_string();
        meeting.sha256 = sha256.clone();
        meeting.template_id = template_id.to_string();
        meeting.queued_at_ms = now_ms();
        self.save(&meeting, &folder)?;
        self.write_evidence(&meeting, &folder);
        self.remember(&sha256, &meeting.id);
        info!("Queued {} as {}", source.display(), meeting.id);
        self.wake();
        Ok(Imported {
            id: meeting.id,
            duplicate: false,
            sha256,
        })
    }

    fn remember(&self, sha256: &str, id: &str) {
        let mut ledger = lock(&self.ledger);
        ledger.hashes.insert(sha256.to_string(), id.to_string());
        if let Err(e) = ledger.save(&self.data_dir) {
            warn!("Could not save the ledger: {e:#}");
        }
    }

    /// Moves a meeting's whole folder (the original recording, the
    /// transcript, the minutes) to the system's trash, where the user can
    /// still bring it back. Nothing is deleted for good. A meeting being
    /// recorded or worked on is refused. The recording is noted, so the same
    /// file in a watched folder is not brought in again.
    pub fn delete(&self, id: &str) -> Result<()> {
        let folder = self.folder(id)?;
        let meeting = Library::read(&folder)?;
        let busy = matches!(meeting.status, Status::Recording | Status::Working)
            || lock(&self.slots).get(id).is_some_and(|slot| {
                matches!(slot.summary.status, Status::Recording | Status::Working)
            });
        if busy {
            bail!("busy");
        }
        if !meeting.sha256.is_empty() {
            let mut ledger = lock(&self.ledger);
            ledger.deleted.insert(meeting.sha256.clone());
            if let Err(e) = ledger.save(&self.data_dir) {
                warn!("Could not save the ledger: {e:#}");
            }
        }
        // Out of the queue first, so nothing starts on it meanwhile.
        let slot = lock(&self.slots).remove(id);
        let in_trash = match to_trash(&folder) {
            Ok(in_trash) => in_trash,
            Err(e) => {
                // Not in the trash: the meeting stays as it was.
                if let Some(slot) = slot {
                    lock(&self.slots).insert(id.to_string(), slot);
                }
                bail!("the meeting could not be moved to the trash ({e})");
            }
        };
        *lock(&self.trashed) = Some(Trashed {
            folder: folder.clone(),
            sha256: meeting.sha256.clone(),
            in_trash,
            at: Instant::now(),
        });
        info!("A meeting was moved to the trash");
        self.changed(id);
        Ok(())
    }

    /// Brings the meeting moved to the trash in the last minute back to
    /// where it was.
    pub fn undo_delete(&self) -> Result<()> {
        let Some(trashed) = lock(&self.trashed).take() else {
            bail!("nothing to bring back");
        };
        if trashed.at.elapsed() > Duration::from_secs(90) {
            bail!("too late; it is still in the trash");
        }
        if trashed.folder.exists() {
            bail!("something else is in its place");
        }
        from_trash(&trashed.folder, trashed.in_trash.as_deref())?;
        {
            let mut ledger = lock(&self.ledger);
            ledger.deleted.remove(&trashed.sha256);
            if let Err(e) = ledger.save(&self.data_dir) {
                warn!("Could not save the ledger: {e:#}");
            }
        }
        self.pick_up_library();
        info!("A meeting was brought back from the trash");
        Ok(())
    }

    /// Queues a meeting again: one that failed, or one whose minutes should
    /// be written afresh (`rewrite`).
    pub fn retry(&self, id: &str, rewrite: bool) -> Result<()> {
        let folder = self.folder(id)?;
        let mut meeting = Library::read(&folder)?;
        if matches!(meeting.status, Status::Recording | Status::Working) {
            bail!("it is being worked on right now");
        }
        if rewrite {
            meeting.minutes.clear();
        }
        self.requeue(meeting, &folder, id)
    }

    /// Transcribes a meeting again from its original recording, with what is
    /// now known about it (its people, place and words), and writes its
    /// minutes afresh. The recording itself is not touched.
    pub fn retranscribe(&self, id: &str) -> Result<()> {
        let folder = self.folder(id)?;
        let mut meeting = Library::read(&folder)?;
        if matches!(meeting.status, Status::Recording | Status::Working) {
            bail!("it is being worked on right now");
        }
        meeting.transcript = None;
        meeting.stats = None;
        meeting.minutes.clear();
        self.requeue(meeting, &folder, id)
    }

    fn requeue(&self, mut meeting: Meeting, folder: &Path, id: &str) -> Result<()> {
        meeting.status = if meeting.transcript.is_some() {
            Status::Transcribed
        } else {
            Status::Queued
        };
        meeting.attempts = 0;
        meeting.minutes_attempts = 0;
        meeting.retry_at_ms = 0;
        meeting.error.clear();
        meeting.error_detail.clear();
        meeting.queued_at_ms = now_ms();
        self.save(&meeting, folder)?;
        if let Some(slot) = lock(&self.slots).get_mut(id) {
            slot.parked = false;
        }
        self.wake();
        Ok(())
    }

    fn write_evidence(&self, meeting: &Meeting, folder: &Path) {
        let evidence = Evidence {
            file: meeting.audio_file.clone(),
            sha256: meeting.sha256.clone(),
            size_bytes: std::fs::metadata(folder.join(&meeting.audio_file)).map_or(0, |m| m.len()),
            recorded_start: store::iso_local(meeting.started_at_ms),
            recorded_end: if meeting.duration_ms > 0 {
                store::iso_local(meeting.started_at_ms + meeting.duration_ms as i64)
            } else {
                String::new()
            },
            time_source: meeting.time_source.clone(),
            duration_ms: meeting.duration_ms,
            origin: meeting.origin.as_str().to_string(),
            original_path: meeting.source_path.clone(),
            input_device: meeting.device.clone(),
            capture_note: meeting.capture_note.clone(),
            event_title: meeting
                .event
                .as_ref()
                .map_or(String::new(), |event| event.title.clone()),
            event_start: meeting
                .event
                .as_ref()
                .map_or(String::new(), |event| store::iso_local(event.start_ms)),
            app: format!(
                "{} {}",
                crate::brand::APP_NAME_ASCII,
                env!("CARGO_PKG_VERSION")
            ),
            written_at: store::iso_local(now_ms()),
        };
        if let Err(e) = evidence::write_sidecar(folder, &evidence) {
            warn!("Could not write {}: {e:#}", evidence::SIDECAR_NAME);
        }
    }

    // ----- recording --------------------------------------------------------

    /// Starts recording a meeting from the microphone. Returns its id.
    pub fn start_recording(
        self: &Arc<Self>,
        title: Option<String>,
        event: Option<(String, i64)>,
    ) -> Result<String> {
        let mut recording = lock(&self.recording);
        if recording.is_some() {
            bail!("a recording is already in progress");
        }
        let config = self.config();
        let started_at_ms = now_ms();
        let title = title.map(|t| t.trim().to_string()).unwrap_or_default();
        let folder = self.library().create_folder(started_at_ms, &title)?;
        let path = folder.join(store::RECORDING_NAME);

        let me = Arc::downgrade(self);
        let device = (!config.input_device.is_empty()).then(|| config.input_device.clone());
        let session = recorder::Session::start(&path, device, move |reason| {
            // The microphone stopped by itself. This thread is the
            // recorder's; the rest is done on another.
            if let Some(me) = me.upgrade() {
                std::thread::spawn(move || me.recording_broke(reason));
            }
        });
        let session = match session {
            Ok(session) => session,
            Err(e) => {
                // Nothing was recorded, so there is no meeting.
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::remove_dir(&folder);
                return Err(e);
            }
        };

        let mut meeting = Meeting::new(started_at_ms, Origin::Recorded, store::RECORDING_NAME);
        meeting.title_is_auto = title.is_empty();
        meeting.title = title;
        meeting.time_source = "recorder".to_string();
        meeting.status = Status::Recording;
        meeting.device = session.device.clone();
        self.apply_calendar(
            &mut meeting,
            event.as_ref().map(|(uid, start)| (uid.as_str(), *start)),
        );
        self.save(&meeting, &folder)?;
        *recording = Some(Active {
            id: meeting.id.clone(),
            folder,
            session,
            started_at_ms,
            marks_ms: Vec::new(),
            _awake: KeepAwake::new(),
        });
        drop(recording);
        info!("Recording {} from {}", meeting.id, meeting.device);
        self.recording_changed(true);
        self.watch_for_no_sound(&meeting.id);
        Ok(meeting.id)
    }

    /// A microphone macOS does not let Seogi use gives silence, not an
    /// error. A recording started by its shortcut or by the calendar may have
    /// no window in sight, so a recording that hears nothing at all a few
    /// seconds in is said once with a notification.
    fn watch_for_no_sound(self: &Arc<Self>, id: &str) {
        let me = Arc::downgrade(self);
        let id = id.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(6));
            let Some(me) = me.upgrade() else {
                return;
            };
            let dead = lock(&me.recording)
                .as_ref()
                .is_some_and(|active| active.id == id && active.session.is_dead());
            if dead {
                warn!("Recording {id} hears nothing: muted, or the microphone is not allowed");
                me.notify(
                    say(&me.app, "소리가 들어오지 않습니다", "No sound is coming in"),
                    say(
                        &me.app,
                        "마이크 음소거나, 시스템 설정 › 개인정보 보호 및 보안 › 마이크에서 서기가 켜져 있는지 확인해 주세요.",
                        "Check that the microphone is not muted and that Seogi is on in System Settings › Privacy & Security › Microphone.",
                    ),
                );
            }
        });
    }

    /// Ends the recording and queues it. Returns its id.
    pub fn stop_recording(&self) -> Result<String> {
        let active = lock(&self.recording)
            .take()
            .ok_or_else(|| anyhow!("no recording is in progress"))?;
        self.finish_recording(active, None)
    }

    fn recording_broke(&self, reason: String) {
        let Some(active) = lock(&self.recording).take() else {
            return;
        };
        warn!("The recording stopped on its own: {reason}");
        match self.finish_recording(active, Some(("microphone_lost", reason))) {
            Ok(_) => self.notify(
                say(&self.app, "녹음이 멈췄습니다", "The recording stopped"),
                say(
                    &self.app,
                    "마이크 연결이 끊겼습니다. 그때까지의 녹음으로 회의록을 만듭니다.",
                    "The microphone went away. Minutes will be made from what was recorded.",
                ),
            ),
            Err(e) => error!("Could not finish the interrupted recording: {e:#}"),
        }
    }

    fn finish_recording(
        &self,
        active: Active,
        trouble: Option<(&'static str, String)>,
    ) -> Result<String> {
        let Active {
            id,
            folder,
            session,
            ..
        } = active;
        let path = folder.join(store::RECORDING_NAME);
        let (frames, trouble) = match session.stop() {
            Ok(finished) => (
                finished.frames,
                trouble.or(finished.trouble.map(|why| ("microphone_lost", why))),
            ),
            // The file was not completed; make whole what reached the disk.
            Err(e) => (
                recorder::repair(&path).unwrap_or(0),
                Some(("file_incomplete", format!("{e:#}"))),
            ),
        };
        let mut meeting = Library::read(&folder)?;
        meeting.duration_ms = frames * 1_000 / u64::from(recorder::RATE);
        let queued = self.queue_recorded(&mut meeting, &folder, trouble);
        self.recording_changed(false);
        queued.map(|()| id)
    }

    /// A finished recording gets its hash and evidence record and joins the
    /// queue.
    fn queue_recorded(
        &self,
        meeting: &mut Meeting,
        folder: &Path,
        trouble: Option<(&'static str, String)>,
    ) -> Result<()> {
        meeting.sha256 = evidence::sha256_file(&folder.join(&meeting.audio_file))?;
        if let Some((note, detail)) = trouble {
            meeting.capture_note = note.to_string();
            meeting.error_detail = detail;
        }
        if meeting.duration_ms < 1_000 {
            // Kept, like every recording, but there is nothing to write up.
            meeting.status = Status::Failed;
            meeting.error = "too_short".to_string();
        } else {
            meeting.status = Status::Queued;
            meeting.queued_at_ms = now_ms();
            self.remember(&meeting.sha256, &meeting.id);
        }
        self.save(meeting, folder)?;
        self.write_evidence(meeting, folder);
        self.wake();
        Ok(())
    }

    /// Marks this moment of the recording as important. Returns the moment,
    /// in milliseconds into the recording.
    pub fn mark(&self) -> Result<u64> {
        let mut recording = lock(&self.recording);
        let active = recording
            .as_mut()
            .ok_or_else(|| anyhow!("no recording is in progress"))?;
        let at = active.session.elapsed_ms();
        let mut meeting = Library::read(&active.folder)?;
        meeting.marks_ms.push(at);
        self.save(&meeting, &active.folder)?;
        active.marks_ms.push(at);
        Ok(at)
    }

    pub fn recording(&self) -> Option<RecordingState> {
        lock(&self.recording).as_ref().map(|active| RecordingState {
            id: active.id.clone(),
            started_at_ms: active.started_at_ms,
            elapsed_ms: active.session.elapsed_ms(),
            level: active.session.level(),
            marks_ms: active.marks_ms.clone(),
            device: active.session.device.clone(),
            no_sound: active.session.is_dead(),
        })
    }

    /// Whether a recording is in progress, without waiting for one that is
    /// just starting (which counts).
    pub fn is_recording(&self) -> bool {
        self.recording
            .try_lock()
            .map_or(true, |recording| recording.is_some())
    }

    fn recording_changed(&self, recording: bool) {
        let _ = RecordingChanged { recording }.emit(&self.app);
        if !self.quiet {
            super::status::set_meeting_recording(&self.app, recording);
            crate::tray::set_tray_state(
                &self.app,
                if recording {
                    crate::tray::TrayIconState::Recording
                } else {
                    crate::tray::TrayIconState::Idle
                },
            );
            crate::tray::rebuild_tray_menu(&self.app);
        }
    }

    /// Starts a recording, or ends the one in progress.
    pub fn toggle_recording(self: &Arc<Self>) {
        let result = if self.is_recording() {
            self.stop_recording()
        } else {
            self.start_recording(None, None)
        };
        if let Err(e) = result {
            warn!("Could not start or stop recording: {e:#}");
            self.notify(
                say(&self.app, "녹음을 시작하지 못했습니다", "Could not record"),
                &format!("{e:#}"),
            );
        }
    }

    fn register_shortcuts(self: &Arc<Self>) {
        use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
        let config = self.config();
        let bindings: [(String, Shortcut); 2] = [
            (config.record_shortcut, |me| me.toggle_recording()),
            (config.mark_shortcut, |me| {
                let _ = me.mark();
            }),
        ];
        for (keys, action) in bindings {
            if keys.trim().is_empty() {
                continue;
            }
            let me = self.clone();
            let registered =
                self.app
                    .global_shortcut()
                    .on_shortcut(keys.as_str(), move |_, _, event| {
                        if event.state == ShortcutState::Pressed {
                            let me = me.clone();
                            std::thread::spawn(move || action(&me));
                        }
                    });
            if let Err(e) = registered {
                warn!("Could not set the shortcut {keys}: {e}");
            }
        }
    }

    // ----- restarting -------------------------------------------------------

    /// What a crash or a restart left half done goes back in the queue.
    fn recover(&self) {
        let unfinished: Vec<(PathBuf, Status)> = lock(&self.slots)
            .values()
            .filter(|slot| matches!(slot.summary.status, Status::Recording | Status::Working))
            .map(|slot| (slot.folder.clone(), slot.summary.status))
            .collect();
        for (folder, status) in unfinished {
            let Ok(mut meeting) = Library::read(&folder) else {
                continue;
            };
            let result = if status == Status::Recording {
                let path = folder.join(&meeting.audio_file);
                // Still being written: another copy of the app is recording.
                let fresh =
                    Look::at(&path).is_some_and(|look| now_ms() - look.modified_ms < 15_000);
                if fresh {
                    continue;
                }
                let frames = recorder::repair(&path).unwrap_or(0);
                meeting.duration_ms = frames * 1_000 / u64::from(recorder::RATE);
                info!(
                    "Recovering the recording {} ({} s)",
                    meeting.id,
                    meeting.duration_ms / 1_000
                );
                self.queue_recorded(
                    &mut meeting,
                    &folder,
                    Some((
                        "app_stopped",
                        "the app stopped while this was being recorded".to_string(),
                    )),
                )
            } else {
                meeting.status = if meeting.transcript.is_some() {
                    Status::Transcribed
                } else {
                    Status::Queued
                };
                meeting.stage.clear();
                meeting.percent = 0;
                self.save(&meeting, &folder)
            };
            if let Err(e) = result {
                warn!("Could not recover {}: {e:#}", folder.display());
            }
        }
    }

    // ----- the queue --------------------------------------------------------

    fn wake(&self) {
        *lock(&self.wake.0) = true;
        self.wake.1.notify_all();
    }

    fn work_loop(self: Arc<Self>) {
        loop {
            match self.next_job() {
                Some(id) => {
                    // The faint dot while minutes are being written.
                    if !self.quiet {
                        super::status::set_meeting_busy(&self.app, true);
                    }
                    if let Err(e) = self.process(&id) {
                        // The meeting's own record could not be read or
                        // written. Leave it be for a while, not in a spin.
                        error!("Could not work on {id}: {e:#}");
                        if let Some(slot) = lock(&self.slots).get_mut(&id) {
                            slot.retry_at_ms = now_ms() + 60_000;
                        }
                    }
                }
                None => {
                    // Nothing left to do, however the last job ended: the
                    // faint dot goes. Checked here rather than after each
                    // job, so a job that failed (its retry written only
                    // afterwards) cannot pass for the next one and leave the
                    // dot on. Setting it when already off does nothing.
                    if !self.quiet {
                        super::status::set_meeting_busy(&self.app, false);
                    }
                    let mut woken = lock(&self.wake.0);
                    if !*woken {
                        // Wakes on its own too: a retry may have come due.
                        woken = self
                            .wake
                            .1
                            .wait_timeout(woken, Duration::from_secs(15))
                            .map_or_else(|e| e.into_inner().0, |(guard, _)| guard);
                    }
                    *woken = false;
                }
            }
        }
    }

    /// The meeting to work on next: recordings waiting to be transcribed
    /// first, in the order they arrived, then minutes waiting to be retried.
    fn next_job(&self) -> Option<String> {
        let now = now_ms();
        lock(&self.slots)
            .iter()
            .filter(|(_, slot)| {
                matches!(slot.summary.status, Status::Queued | Status::Transcribed)
                    && !slot.parked
                    && slot.retry_at_ms <= now
            })
            .min_by_key(|(_, slot)| (slot.summary.status != Status::Queued, slot.queued_at_ms))
            .map(|(id, _)| id.clone())
    }

    /// Whether nothing is waiting or being worked on. Minutes waiting for a
    /// model that cannot be reached do not count: that can last for hours.
    pub fn is_settled(&self) -> bool {
        !lock(&self.slots).values().any(|slot| {
            matches!(slot.summary.status, Status::Queued | Status::Working)
                && slot.summary.error != "speech_model_missing"
        })
    }

    /// The speech model to transcribe with now: the chosen one, or one that
    /// stands in while it is not on this computer; `None` when no usable
    /// model is here yet.
    fn speech_model_now(&self) -> Option<SpeechModelNow> {
        self.config().speech_model_now(self.model_here())
    }

    /// Whether a speech model is downloaded. Without the model manager (a
    /// run that has none) every model counts as here, as it always did.
    fn model_here(&self) -> impl Fn(&str) -> bool + '_ {
        use crate::managers::model::ModelManager;
        let models = self.app.try_state::<Arc<ModelManager>>();
        move |id: &str| {
            models.as_ref().is_none_or(|models| {
                models
                    .get_model_info(id)
                    .is_some_and(|model| model.is_downloaded)
            })
        }
    }

    fn process(&self, id: &str) -> Result<()> {
        let mut folder = self.folder(id)?;
        let mut meeting = Library::read(&folder)?;
        let _awake = KeepAwake::new();
        let started = Instant::now();
        let cancel = AtomicBool::new(false);

        if meeting.transcript.is_none() && self.speech_model_now().is_none() {
            // Nothing is wrong with the recording. It waits, without using
            // up its tries, until the model has been downloaded.
            if meeting.error != "speech_model_missing" {
                meeting.error = "speech_model_missing".to_string();
                meeting.status = Status::Queued;
                self.save(&meeting, &folder)?;
            }
            if let Some(slot) = lock(&self.slots).get_mut(id) {
                slot.retry_at_ms = now_ms() + 30_000;
            }
            return Ok(());
        }

        if meeting.transcript.is_none() {
            meeting.status = Status::Working;
            meeting.stage = "decoding".to_string();
            meeting.percent = 0;
            meeting.error.clear();
            meeting.error_detail.clear();
            meeting.attempts += 1;
            self.save(&meeting, &folder)?;

            match self.transcribe(&mut meeting, &folder, &cancel) {
                Ok(true) => {}
                Ok(false) => {
                    meeting.status = Status::Queued;
                    meeting.stage.clear();
                    return self.save(&meeting, &folder);
                }
                Err(trouble) => return self.set_back(&mut meeting, &folder, trouble),
            }
            // The start time may be known better now; the folder is named by it.
            folder = self
                .library()
                .retitle_folder(&folder, meeting.started_at_ms, &meeting.title);
            meeting.status = Status::Transcribed;
            meeting.stage.clear();
            meeting.retry_at_ms = 0;
            meeting.total_ms += started.elapsed().as_millis() as u64;
            self.save(&meeting, &folder)?;
            self.write_evidence(&meeting, &folder);
        }

        let has_speech = meeting
            .transcript
            .as_ref()
            .is_some_and(|transcript| !transcript.segments.is_empty());
        if !has_speech {
            meeting.status = Status::Done;
            meeting.error = "no_speech".to_string();
            meeting.percent = 100;
            self.save(&meeting, &folder)?;
            self.notify(
                say(
                    &self.app,
                    "녹음에서 말소리를 찾지 못했습니다",
                    "No speech was found in the recording",
                ),
                &shown_title(&meeting),
            );
            return Ok(());
        }

        if meeting.minutes.is_empty() {
            let to_try = self.ways_to_write();
            if to_try.is_empty() {
                return self.wait_for_a_way(&mut meeting, &folder);
            }
            let minutes_started = Instant::now();
            meeting.status = Status::Working;
            meeting.stage = "writing_minutes".to_string();
            meeting.percent = 80;
            meeting.minutes_attempts += 1;
            self.save(&meeting, &folder)?;

            match self.write_minutes(&mut meeting, &to_try, &cancel) {
                Ok(true) => {}
                Ok(false) => {
                    meeting.status = Status::Transcribed;
                    meeting.stage.clear();
                    return self.save(&meeting, &folder);
                }
                Err(trouble) => {
                    // The transcript is kept and readable; the minutes are
                    // tried again later.
                    warn!(
                        "Minutes for {} failed (try {}): {}",
                        meeting.id, meeting.minutes_attempts, trouble.detail
                    );
                    meeting.status = Status::Transcribed;
                    meeting.stage.clear();
                    meeting.error = trouble.code.to_string();
                    meeting.error_detail = trouble.detail;
                    let give_up =
                        !trouble.retry || meeting.minutes_attempts >= MAX_MINUTES_ATTEMPTS;
                    meeting.retry_at_ms = if give_up {
                        0
                    } else {
                        now_ms() + backoff_ms(meeting.minutes_attempts)
                    };
                    self.save(&meeting, &folder)?;
                    if give_up {
                        if let Some(slot) = lock(&self.slots).get_mut(&meeting.id) {
                            slot.parked = true;
                        }
                        self.notify(
                            say(
                                &self.app,
                                "회의록을 아직 쓰지 못했습니다",
                                "The minutes are not written yet",
                            ),
                            say(
                                &self.app,
                                "받아쓴 글은 저장했습니다. 회의록을 쓰는 모델에 연결되면 다시 씁니다.",
                                "The transcript is saved. Minutes will be written once the model can be reached.",
                            ),
                        );
                    }
                    return Ok(());
                }
            }
            meeting.minutes_ms = minutes_started.elapsed().as_millis() as u64;
            meeting.total_ms += meeting.minutes_ms;
        }

        folder = self
            .library()
            .retitle_folder(&folder, meeting.started_at_ms, &meeting.title);
        meeting.status = Status::Done;
        meeting.stage.clear();
        meeting.percent = 100;
        meeting.error.clear();
        meeting.error_detail.clear();
        meeting.retry_at_ms = 0;
        self.save(&meeting, &folder)?;
        self.write_evidence(&meeting, &folder);
        self.leave_minutes_beside_source(&meeting, &folder);

        let summary = meeting.summary(&folder);
        info!(
            "Minutes ready for {}: {} verified, {} to check, in {} s",
            meeting.id,
            summary.verified,
            summary.to_check,
            meeting.total_ms / 1_000
        );
        self.show_banner(Banner::Ready {
            id: meeting.id.clone(),
            title: shown_title(&meeting),
            actions: summary.actions,
            to_check: summary.to_check,
        });
        Ok(())
    }

    /// Copies the minutes of a recording that came from a watched folder to
    /// that folder, beside the recording: `<its name> 회의록.md`, and the
    /// same as `.txt` for phones that will not open the first. The copy is a
    /// convenience; the meeting's own folder stays the record.
    fn leave_minutes_beside_source(&self, meeting: &Meeting, folder: &Path) {
        if meeting.origin != Origin::Watched || !self.config().minutes_beside_source {
            return;
        }
        let source = Path::new(&meeting.source_path);
        let (Some(beside), Some(name)) = (
            source.parent().filter(|dir| dir.is_dir()),
            source.file_stem().and_then(|stem| stem.to_str()),
        ) else {
            return;
        };
        let minutes = folder.join(store::MINUTES_NAME);
        for ending in ["md", "txt"] {
            let copy = beside.join(format!("{name} 회의록.{ending}"));
            if let Err(e) = std::fs::copy(&minutes, &copy) {
                warn!("Could not leave the minutes at {}: {e}", copy.display());
            }
        }
    }

    /// Records why a meeting could not be transcribed and when to try again.
    fn set_back(&self, meeting: &mut Meeting, folder: &Path, trouble: Trouble) -> Result<()> {
        warn!(
            "Transcribing {} failed (try {}): {}",
            meeting.id, meeting.attempts, trouble.detail
        );
        meeting.stage.clear();
        meeting.percent = 0;
        meeting.error = trouble.code.to_string();
        meeting.error_detail = trouble.detail;
        if trouble.retry && meeting.attempts < MAX_TRANSCRIBE_ATTEMPTS {
            meeting.status = Status::Queued;
            meeting.retry_at_ms = now_ms() + backoff_ms(meeting.attempts);
            return self.save(meeting, folder);
        }
        meeting.status = Status::Failed;
        meeting.retry_at_ms = 0;
        self.save(meeting, folder)?;
        self.notify(
            say(
                &self.app,
                "녹음을 정리하지 못했습니다",
                "A recording could not be processed",
            ),
            &shown_title(meeting),
        );
        Ok(())
    }

    /// `Ok(false)` if it was stopped part way.
    fn transcribe(
        &self,
        meeting: &mut Meeting,
        folder: &Path,
        cancel: &AtomicBool,
    ) -> Result<bool, Trouble> {
        let audio = folder.join(&meeting.audio_file);
        // A file that cannot be read will not read better next time.
        let pcm = audio_in::decode(&audio).map_err(|e| Trouble::new("cannot_decode", e, false))?;
        meeting.duration_ms = pcm.len() as u64 * 1_000 / RATE as u64;
        if meeting.time_source == "file_modified" {
            meeting.started_at_ms -= meeting.duration_ms as i64;
            meeting.time_source = "file_modified_minus_length".to_string();
        }
        // Now that its start and length are known, the calendar can say
        // which meeting this was.
        self.apply_calendar(meeting, None);

        let config = self.config();
        let vocabulary = Vocabulary::load(&config.vocabulary(&self.data_dir)).unwrap_or_else(|e| {
            warn!("The vocabulary could not be read, going without: {e:#}");
            Vocabulary::default()
        });
        // What is known about this meeting beforehand steers the hints.
        let mut mentions: Vec<String> = Vec::new();
        if !meeting.title_is_auto && !meeting.title.is_empty() {
            mentions.push(meeting.title.clone());
        }
        mentions.extend(meeting.people.iter().cloned());
        if !meeting.customer.is_empty() {
            mentions.push(meeting.customer.clone());
        }
        if let Some(event) = &meeting.event {
            mentions.push(event.title.clone());
        }
        if !meeting.place.is_empty() {
            mentions.push(meeting.place.clone());
        }
        mentions.extend(meeting.terms.iter().cloned());

        let monitor = JobMonitor {
            secretary: self,
            id: &meeting.id,
            cancel,
        };
        let outcome = transcribe_now(&config, self.model_here(), |model| {
            let request = Request {
                pcm: &pcm,
                language: &config.language,
                vocabulary: &vocabulary,
                mentions: &mentions,
                diarize: config.diarize,
                model_id: Some(model),
            };
            pipeline::transcribe(&self.app, &request, &monitor)
        });
        match outcome {
            Ok(Some(outcome)) => {
                if let Some(chosen) = &outcome.stats.stood_in_for {
                    info!(
                        "{chosen} is not here; {} was transcribed with {}",
                        meeting.id, outcome.transcript.model
                    );
                }
                meeting.transcript = Some(outcome.transcript);
                meeting.stats = Some(outcome.stats);
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(e) => Err(Trouble::new("speech_model", e, true)),
        }
    }

    /// The ways minutes can be written with now, in the order they are
    /// tried ([`Ways::to_try`]): the one chosen first when it can be used.
    /// A model on this computer that is not ready yet is still tried, as
    /// before, and its own error says why.
    fn ways_to_write(&self) -> Vec<Provider> {
        let chosen = self.config().summary;
        let mut to_try = Ways::look(&chosen, &|name| super::secrets::has(name)).to_try(&chosen);
        if to_try.is_empty() && chosen.is_local() {
            to_try.push(chosen);
        }
        to_try
    }

    /// Minutes that nothing on this computer can write: no key, no
    /// signed-in plan, no model here. They wait without using up their tries
    /// and are written as soon as a way appears; the user hears of it once.
    fn wait_for_a_way(&self, meeting: &mut Meeting, folder: &Path) -> Result<()> {
        let first_time = meeting.error != "summary_unavailable";
        if first_time {
            warn!(
                "Minutes for {}: nothing can write them ({} cannot be used, no other plan is signed in, no other key is saved, no model on this computer)",
                meeting.id,
                self.config().summary.label()
            );
        }
        meeting.status = Status::Transcribed;
        meeting.stage.clear();
        meeting.error = "summary_unavailable".to_string();
        meeting.error_detail = format!(
            "{} cannot be used, and no other way to write minutes is ready",
            self.config().summary.label()
        );
        meeting.retry_at_ms = now_ms() + LOOK_FOR_A_WAY_MS;
        self.save(meeting, folder)?;
        if first_time {
            self.notify(
                say(
                    &self.app,
                    "회의록을 아직 쓰지 못했습니다",
                    "The minutes are not written yet",
                ),
                say(
                    &self.app,
                    "받아쓴 글은 저장했습니다. 로그인하거나 키를 넣으면 바로 씁니다.",
                    "The transcript is saved. Sign in or add a key and the minutes are written at once.",
                ),
            );
        }
        Ok(())
    }

    /// Minutes waiting for another try are tried now: a way to write them
    /// may have appeared (a sign-in, a key, other settings). Only within
    /// their tries: minutes that used them all up still wait for the user.
    pub fn minutes_may_go(&self) {
        let mut any = false;
        for slot in lock(&self.slots).values_mut() {
            if slot.summary.status == Status::Transcribed
                && slot.summary.error.starts_with("summary_")
                && !slot.parked
                && slot.retry_at_ms > 0
            {
                slot.retry_at_ms = 0;
                any = true;
            }
        }
        if any {
            self.wake();
        }
    }

    /// `Ok(false)` if it was stopped part way. `to_try` are the ways to
    /// write them with, in order: when one cannot be used (a key refused, a
    /// plan run out) the next writes them, and the minutes say which did.
    fn write_minutes(
        &self,
        meeting: &mut Meeting,
        to_try: &[Provider],
        cancel: &AtomicBool,
    ) -> Result<bool, Trouble> {
        let mut last = None;
        for (index, provider) in to_try.iter().enumerate() {
            match self.write_minutes_with(meeting, provider, cancel) {
                Err(e) if index + 1 < to_try.len() && llm::cannot_use(&format!("{e:#}")) => {
                    warn!(
                        "Minutes for {} could not be written with {} ({e:#}); trying {}",
                        meeting.id,
                        provider.label(),
                        to_try[index + 1].label()
                    );
                    last = Some(e);
                }
                Err(e) => {
                    let code = llm::trouble_code(&format!("{e:#}"));
                    return Err(Trouble::new(code, e, true));
                }
                Ok(written) => return Ok(written),
            }
        }
        let e = last.unwrap_or_else(|| anyhow!("there is no way to write minutes"));
        let code = llm::trouble_code(&format!("{e:#}"));
        Err(Trouble::new(code, e, true))
    }

    fn write_minutes_with(
        &self,
        meeting: &mut Meeting,
        provider: &Provider,
        cancel: &AtomicBool,
    ) -> Result<bool> {
        let all_templates = templates::load(&self.data_dir.join("templates"));
        let Some(transcript) = meeting.transcript.clone() else {
            return Err(anyhow!("there is no transcript"));
        };
        let recorded_on = meeting.recorded_on();
        let context = meeting.context();
        let feedback_rules = super::feedback::rules(&self.data_dir);
        let mut people = meeting.people.clone();
        if !meeting.customer.is_empty() && !people.contains(&meeting.customer) {
            people.push(meeting.customer.clone());
        }
        // Keys for the user's own paid models come from the system's
        // keychain, and only when such a model is the one writing.
        let keychain = |name: &str| super::secrets::get(name);
        let id = meeting.id.clone();
        let progress = |fraction: f32| {
            self.progress(
                &id,
                "writing_minutes",
                80 + (fraction.clamp(0.0, 1.0) * 19.0) as u8,
            )
        };
        let input = minutes::Input {
            transcript: &transcript,
            templates: &all_templates,
            template_id: (!meeting.template_id.is_empty()).then_some(meeting.template_id.as_str()),
            marks_ms: &meeting.marks_ms,
            mark_notes: &meeting.mark_notes,
            people: &people,
            customer: &meeting.customer,
            recorded_on: &recorded_on,
            context: &context,
            feedback_rules: &feedback_rules,
            provider,
            secrets: &keychain,
        };
        match minutes::write(&input, &progress, cancel) {
            Ok(Some(output)) => {
                if let Some(transcript) = meeting.transcript.as_mut() {
                    for (speaker, name) in &output.speaker_names {
                        if let Some(entry) =
                            transcript.speakers.iter_mut().find(|s| s.id == *speaker)
                        {
                            entry.name = name.clone();
                        }
                    }
                }
                let title = output.minutes.title.trim();
                if meeting.title_is_auto && !title.is_empty() {
                    meeting.title = store::safe_name(title);
                }
                meeting.minutes = vec![output.minutes];
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(e) => Err(e),
        }
    }

    // ----- watched folders --------------------------------------------------

    fn watch_loop(self: Arc<Self>) {
        let mut settling = Settling::default();
        // Files that could not be brought in, and when: they are tried again
        // after a while, not at every look.
        let mut refused: HashMap<String, Instant> = HashMap::new();
        const TRY_AGAIN_AFTER: Duration = Duration::from_secs(5 * 60);
        let every = std::env::var("SEOGI_WATCH_SECS")
            .ok()
            .and_then(|secs| secs.parse().ok())
            .map_or(WATCH_EVERY, Duration::from_secs);
        loop {
            self.pick_up_library();
            self.note_watched_folders();
            for watched in self.config().watched.iter().filter(|w| w.enabled) {
                let folder = Path::new(&watched.path);
                // Until what was already there has been noted (a drive not
                // connected yet), nothing in the folder is taken.
                let noted = format!("{}|{}", folder.display(), watched.since_ms);
                if !lock(&self.ledger).noted.contains(&noted) {
                    continue;
                }
                for (file, look) in settling.settled(folder, now_ms()) {
                    let key = Ledger::key(&file, look);
                    if lock(&self.ledger).seen.contains_key(&key) {
                        continue;
                    }
                    if refused
                        .get(&key)
                        .is_some_and(|at| at.elapsed() < TRY_AGAIN_AFTER)
                    {
                        continue;
                    }
                    // A file the cloud has not brought down yet is read (and
                    // so fetched) here; if that fails it is tried again in a
                    // few minutes.
                    match self.import(&file, Origin::Watched, &watched.template_id) {
                        Ok(imported) => {
                            let mut ledger = lock(&self.ledger);
                            ledger.seen.insert(key, imported.sha256);
                            if let Err(e) = ledger.save(&self.data_dir) {
                                warn!("Could not save the ledger: {e:#}");
                            }
                        }
                        Err(e) => {
                            warn!("Could not bring in {}: {e:#}", file.display());
                            refused.insert(key, Instant::now());
                        }
                    }
                }
            }
            std::thread::sleep(every);
        }
    }

    // ----- the calendar -----------------------------------------------------

    /// Reads the calendars now and again, and says when a meeting is about
    /// to start.
    fn calendar_loop(self: Arc<Self>) {
        use std::sync::atomic::Ordering;
        const REFRESH_EVERY: Duration = Duration::from_secs(15 * 60);
        // What was read last time serves until the calendars answer.
        *lock(&self.calendars) = self.cached_calendars();
        let mut read_at: Option<Instant> = None;
        let mut reminded: std::collections::HashSet<(String, i64)> = Default::default();
        let mut told: Option<&'static str> = None;
        loop {
            let due = read_at.is_none_or(|at| at.elapsed() >= REFRESH_EVERY);
            // macOS can take the Calendar app's permission away (an update
            // it did not recognize as the same app). Asking whether it is
            // back costs nothing, so each turn does, and the calendars are
            // read the moment it is, without restarting Seogi.
            let allowed_again =
                self.calendar_denied() && super::mac_calendar::access() == "granted";
            if self.calendars_stale.swap(false, Ordering::AcqRel) || due || allowed_again {
                self.read_calendars();
                read_at = Some(Instant::now());
            }
            // Right after a new signature macOS may not list Seogi at all
            // (not_determined): then it is asked, not sent to System Settings.
            let refused = self.calendar_denied().then(|| {
                if super::mac_calendar::access() == "not_determined" {
                    "not_determined"
                } else {
                    "denied"
                }
            });
            if refused != told {
                use crate::notices::{self, Notice, NoticeCode, NoticeSource};
                match refused {
                    Some(access) => notices::raise_without_overlay(
                        &self.app,
                        Notice::new(NoticeCode::CalendarPermissionLost, NoticeSource::Calendar)
                            .with_detail(access.to_string()),
                    ),
                    None => notices::clear_matching(
                        &self.app,
                        NoticeSource::Calendar,
                        Some(NoticeCode::CalendarPermissionLost),
                    ),
                }
                told = refused;
            }
            self.remind(&mut reminded);
            std::thread::sleep(Duration::from_secs(20));
        }
    }

    /// Calendars from the Calendar app were chosen, and macOS refused them
    /// at the last read.
    fn calendar_denied(&self) -> bool {
        lock(&self.calendar_status)
            .mac
            .as_ref()
            .is_some_and(|check| check.reason == "mac_denied")
    }

    fn calendar_cache(&self, index: usize) -> PathBuf {
        self.data_dir.join("calendar").join(format!("{index}.ics"))
    }

    fn cached_calendars(&self) -> Vec<String> {
        (0..self.config().calendar_urls.len())
            .filter_map(|index| std::fs::read_to_string(self.calendar_cache(index)).ok())
            .collect()
    }

    /// Reads every calendar the user gave. One that cannot be reached keeps
    /// what was read from it last time.
    pub fn read_calendars(&self) {
        let config = self.config();
        let sources = config.calendar_urls;
        let mut texts: Vec<String> = Vec::new();
        let mut status: Vec<CalendarCheck> = Vec::new();
        let mac = (!config.mac_calendars.is_empty()).then(|| {
            const DAY_MS: i64 = 24 * 3_600_000;
            let now = now_ms();
            match super::mac_calendar::events(
                &config.mac_calendars,
                now - 30 * DAY_MS,
                now + 90 * DAY_MS,
            ) {
                Ok(events) => {
                    let text = calendar::to_ics(&events);
                    let check = CalendarCheck::of(String::new(), Ok(&text), now);
                    texts.push(text);
                    check
                }
                Err(e) => {
                    warn!("The Calendar app could not be read: {e:#}");
                    CalendarCheck {
                        reason: if super::mac_calendar::access() == "granted" {
                            "mac_missing"
                        } else {
                            "mac_denied"
                        }
                        .to_string(),
                        error: format!("{e:#}"),
                        read_at_ms: now,
                        ..CalendarCheck::default()
                    }
                }
            }
        });
        for (index, source) in sources.iter().enumerate() {
            let cache = self.calendar_cache(index);
            let read = calendar::fetch(source);
            // The address is a secret: the status keeps only its place.
            let mut check = CalendarCheck::of(
                String::new(),
                read.as_deref().map_err(|e| format!("{e:#}")),
                now_ms(),
            );
            check.reason = public_closed(source, &check.reason);
            status.push(check);
            match read {
                Ok(text) => {
                    if let Some(dir) = cache.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    if let Err(e) = store::write_atomically(&cache, text.as_bytes()) {
                        warn!("Could not keep a copy of calendar {}: {e:#}", index + 1);
                    }
                    texts.push(text);
                }
                Err(e) => {
                    // The address is a secret and is not part of the message.
                    warn!("Calendar {} could not be read: {e:#}", index + 1);
                    if let Ok(text) = std::fs::read_to_string(&cache) {
                        texts.push(text);
                    }
                }
            }
        }
        let all: Vec<&CalendarCheck> = mac.iter().chain(status.iter()).collect();
        info!(
            "Read {} of {} calendar(s), {} event(s) today",
            all.iter().filter(|check| check.ok).count(),
            all.len(),
            all.iter().map(|check| check.today).sum::<u32>()
        );
        *lock(&self.calendars) = texts;
        *lock(&self.calendar_status) = CalendarStatus {
            mac,
            addresses: status,
        };
    }

    /// How the calendars answered when last read.
    pub fn calendar_status(&self) -> CalendarStatus {
        lock(&self.calendar_status).clone()
    }

    /// Today's calendar events that have not ended, earliest first. When
    /// none is left today, the next one within a week, so the home can say
    /// "내일 10:00 …". The day ends at midnight on this computer's clock.
    pub fn today(&self) -> Vec<Upcoming> {
        use chrono::{Duration as Span, Local, TimeZone};
        let now = Local::now();
        let midnight = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .and_then(|start| Local.from_local_datetime(&start).earliest())
            .map_or(now.timestamp_millis(), |start| {
                (start + Span::days(1)).timestamp_millis()
            });
        let muted = self.config().muted_events;
        let calendars = lock(&self.calendars).clone();
        let week = now.timestamp_millis() + 7 * 24 * 3_600_000;
        upcoming_from(
            calendar::events_of(&calendars, now.timestamp_millis(), week),
            midnight,
            &muted,
        )
    }

    /// Whether a calendar address (or file) can be read, how many events it
    /// has today and which is next.
    pub fn check_calendar(source: &str) -> CalendarCheck {
        let address = calendar::normalize(source);
        let read = calendar::fetch(&address);
        let mut check = CalendarCheck::of(
            address,
            read.as_deref().map_err(|e| format!("{e:#}")),
            now_ms(),
        );
        check.reason = public_closed(&check.address, &check.reason);
        if check.ok {
            info!("A calendar was checked: {} event(s) today", check.today);
        } else {
            // The address is a secret and is not part of the message.
            warn!("A calendar could not be read: {}", check.error);
        }
        check
    }

    /// The calendar's events from now until `hours` from now.
    pub fn upcoming(&self, hours: u32) -> Vec<calendar::Event> {
        let now = now_ms();
        let calendars = lock(&self.calendars).clone();
        calendar::events_of(&calendars, now, now + i64::from(hours) * 3_600_000)
    }

    /// Gives a meeting what its calendar event knows: a title, the kind of
    /// minutes, who was there. What the user set themselves is left alone.
    ///
    /// `chosen` names the event (its id and start) when the user picked one;
    /// otherwise it is the event the recording shares the most time with.
    fn apply_calendar(&self, meeting: &mut Meeting, chosen: Option<(&str, i64)>) {
        if meeting.event.is_some() {
            return;
        }
        let calendars = lock(&self.calendars).clone();
        if calendars.is_empty() {
            return;
        }
        const DAY_MS: i64 = 24 * 3_600_000;
        let start = meeting.started_at_ms;
        let end = start + meeting.duration_ms as i64;
        let events = calendar::events_of(&calendars, start - DAY_MS, end + DAY_MS);
        let found = match chosen {
            Some((uid, at)) => events
                .iter()
                .find(|event| event.uid == uid && event.start_ms == at),
            None => calendar::best_match(&events, start, end),
        };
        let Some(event) = found else {
            return;
        };
        let reading = calendar::read_title(&calendar::load_rules(&self.data_dir), &event.title);
        meeting.event = Some(EventRef {
            uid: event.uid.clone(),
            title: event.title.clone(),
            start_ms: event.start_ms,
            end_ms: event.end_ms,
        });
        if meeting.title_is_auto && !reading.title.is_empty() {
            meeting.title = store::safe_name(&reading.title);
            meeting.title_is_auto = false;
        }
        let known = templates::load(&self.data_dir.join("templates"));
        if meeting.template_id.is_empty() && known.iter().any(|t| t.id == reading.template_id) {
            meeting.template_id = reading.template_id;
        }
        if meeting.place.is_empty() {
            meeting.place = if reading.place.is_empty() {
                event.location.clone()
            } else {
                reading.place
            };
        }
        for name in event.attendees.iter().chain(reading.mentions.iter()) {
            if !meeting.people.contains(name) {
                meeting.people.push(name.clone());
            }
        }
        info!("{} matched a calendar event", meeting.id);
    }

    /// Tells the screen, once, about each event that is about to start
    /// while nothing is being recorded.
    fn remind(&self, reminded: &mut std::collections::HashSet<(String, i64)>) {
        let config = self.config();
        if !config.remind || self.is_recording() {
            return;
        }
        let now = now_ms();
        let lead = i64::from(config.remind_minutes_before) * 60_000;
        let calendars = lock(&self.calendars).clone();
        // From `lead` before the start until five minutes into it.
        for event in calendar::events_of(&calendars, now - 5 * 60_000, now + lead) {
            let in_window = event.start_ms - lead <= now && now < event.start_ms + 5 * 60_000;
            if !in_window
                || config.muted_events.contains(&event.uid)
                || !reminded.insert((event.uid.clone(), event.start_ms))
            {
                continue;
            }
            let minutes_until = ((event.start_ms - now) as f64 / 60_000.0).round() as i32;
            let title =
                calendar::read_title(&calendar::load_rules(&self.data_dir), &event.title).title;
            let _ = MeetingStarting {
                event: event.clone(),
                minutes_until,
            }
            .emit(&self.app);
            let _ = minutes_until;
            let _ = title;
            self.show_banner(Banner::Starting { event });
        }
    }

    /// Stops reminders for a calendar event (every occurrence of it).
    pub fn mute_event(&self, uid: &str, muted: bool) -> Result<()> {
        let mut config = self.config();
        config.muted_events.retain(|other| other != uid);
        if muted {
            config.muted_events.push(uid.to_string());
        }
        self.set_config(config)
    }

    /// Answers a question about a meeting from its transcript, with the
    /// words the answer rests on.
    pub fn ask(&self, id: &str, question: &str) -> Result<super::ask::Answer> {
        let meeting = self.meeting(id)?;
        let transcript = meeting
            .transcript
            .ok_or_else(|| anyhow!("it has not been transcribed yet"))?;
        let keychain = |name: &str| super::secrets::get(name);
        super::ask::answer(&transcript, question, &self.config().summary, &keychain)
    }

    // ----- what the user tells the app --------------------------------------

    fn editable(&self, id: &str) -> Result<(Meeting, PathBuf)> {
        let folder = self.folder(id)?;
        let meeting = Library::read(&folder)?;
        if meeting.status == Status::Working {
            bail!("it is being worked on right now");
        }
        Ok((meeting, folder))
    }

    /// Sets what the user knows about a meeting: its title, kind, customer,
    /// who was there. Used for the minutes when they are written (again).
    pub fn set_meeting_info(&self, id: &str, info: MeetingInfo) -> Result<()> {
        let (mut meeting, folder) = self.editable(id)?;
        if let Some(title) = info.title {
            let title = store::safe_name(&title);
            meeting.title_is_auto = title.is_empty();
            meeting.title = title;
        }
        if let Some(template_id) = info.template_id {
            meeting.template_id = template_id.trim().to_string();
        }
        if let Some(customer) = info.customer {
            meeting.customer = customer.trim().to_string();
        }
        let tidy = |names: Vec<String>| -> Vec<String> { tidy_names(names) };
        if let Some(people) = info.people {
            meeting.people = tidy(people);
        }
        if let Some(place) = info.place {
            meeting.place = place.trim().to_string();
        }
        if let Some(note) = info.note {
            meeting.note = note.trim().to_string();
        }
        if let Some(terms) = info.terms {
            meeting.terms = tidy(terms);
        }
        self.save(&meeting, &folder)
    }

    /// Candidate words for a meeting from its documents, without those it
    /// has already or the vocabulary knows.
    pub fn terms_from_documents(
        &self,
        id: &str,
        paths: &[PathBuf],
    ) -> Result<super::documents::TermsFound> {
        let meeting = self.meeting(id)?;
        let config = self.config();
        let vocabulary = Vocabulary::load(&config.vocabulary(&self.data_dir)).unwrap_or_default();
        let mut known: Vec<String> = meeting.terms.clone();
        known.extend(meeting.people.iter().cloned());
        let keychain = |name: &str| super::secrets::get(name);
        let mut found = super::documents::terms_from(paths, &config.summary, &keychain, &known)?;
        found.terms.retain(|term| !vocabulary.knows(term));
        Ok(found)
    }

    /// Adds words to the user's vocabulary, which is for every meeting.
    /// Returns how many were new.
    pub fn add_to_vocabulary(&self, terms: &[String]) -> Result<usize> {
        let path = self.config().vocabulary(&self.data_dir);
        let heading = format!(
            "서기에서 자료로 추가 ({})",
            chrono::Local::now().format("%Y-%m-%d")
        );
        super::vocab::append_terms(&path, terms, &heading)
    }

    /// Writes a note beside a mark of the recording in progress.
    pub fn set_mark_note(&self, index: usize, note: &str) -> Result<()> {
        let folder = lock(&self.recording)
            .as_ref()
            .map(|active| active.folder.clone())
            .ok_or_else(|| anyhow!("no recording is in progress"))?;
        let mut meeting = Library::read(&folder)?;
        if index >= meeting.marks_ms.len() {
            bail!("there is no such mark");
        }
        meeting
            .mark_notes
            .resize(meeting.marks_ms.len(), String::new());
        meeting.mark_notes[index] = note.trim().to_string();
        self.save(&meeting, &folder)
    }

    /// Ticks a task off, or back on.
    pub fn set_action_done(&self, id: &str, section: &str, index: usize, done: bool) -> Result<()> {
        let (mut meeting, folder) = self.editable(id)?;
        let entry = meeting
            .minutes
            .first_mut()
            .and_then(|minutes| minutes.sections.iter_mut().find(|s| s.key == section))
            .and_then(|section| section.entries.get_mut(index))
            .ok_or_else(|| anyhow!("there is no such task"))?;
        entry.done = done;
        self.save(&meeting, &folder)
    }

    // ----- the banner -------------------------------------------------------

    /// Shows the banner window with `banner`. If the window cannot be made,
    /// the system's own notification says it instead.
    fn show_banner(&self, banner: Banner) {
        if self.quiet {
            return;
        }
        let (title, body, starting) = match &banner {
            Banner::Starting { event } => (
                format!(
                    "{} {}",
                    say(&self.app, "곧 시작합니다:", "Starting soon:"),
                    event.title
                ),
                String::new(),
                true,
            ),
            Banner::Ready {
                title,
                actions,
                to_check,
                ..
            } => (
                say(&self.app, "회의록이 준비됐습니다", "The minutes are ready").to_string(),
                if is_korean(&self.app) {
                    format!("{title} · 할 일 {actions}개 · 확인 필요 {to_check}곳")
                } else {
                    format!("{title} · {actions} tasks · {to_check} to check")
                },
                false,
            ),
        };
        *lock(&self.banner) = Some(banner);
        if let Err(e) = super::banner::show(&self.app, starting) {
            warn!("The banner could not be shown ({e:#}); using a notification");
            *lock(&self.banner) = None;
            self.notify(&title, &body);
            return;
        }
        let _ = BannerChanged {}.emit(&self.app);
    }

    pub fn banner(&self) -> Option<Banner> {
        lock(&self.banner).clone()
    }

    /// What a button of the banner does: `record`, `mute`, `open`, or
    /// anything else to put it away.
    pub fn banner_action(self: &Arc<Self>, action: &str) -> Result<()> {
        let banner = lock(&self.banner).take();
        super::banner::hide(&self.app);
        let _ = BannerChanged {}.emit(&self.app);
        match (action, banner) {
            ("record", Some(Banner::Starting { event })) => self
                .start_recording(None, Some((event.uid, event.start_ms)))
                .map(|_| ()),
            ("mute", Some(Banner::Starting { event })) => self.mute_event(&event.uid, true),
            ("open", Some(Banner::Ready { id, .. })) => {
                crate::show_main_window(&self.app);
                let _ = OpenMeeting { id }.emit(&self.app);
                Ok(())
            }
            _ => Ok(()),
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

/// What a global shortcut does.
type Shortcut = fn(&Arc<Secretary>);

/// Reports the pipeline's progress as the meeting's.
struct JobMonitor<'a> {
    secretary: &'a Secretary,
    id: &'a str,
    cancel: &'a AtomicBool,
}

impl Monitor for JobMonitor<'_> {
    fn progress(&self, stage: Stage, fraction: f32) {
        let fraction = fraction.clamp(0.0, 1.0);
        // Transcribing is most of the work; the minutes take the last fifth.
        let (name, from, span) = match stage {
            Stage::DetectingSpeech => ("detecting_speech", 2.0, 3.0),
            Stage::Diarizing => ("diarizing", 5.0, 20.0),
            Stage::Transcribing => ("transcribing", 25.0, 55.0),
        };
        self.secretary
            .progress(self.id, name, (from + fraction * span) as u8);
    }

    fn cancel_flag(&self) -> &AtomicBool {
        self.cancel
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

/// The tray menu: recording first, the window second.
pub fn tray_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    let recording = Secretary::of(app).is_some_and(|secretary| secretary.is_recording());
    let item = |id: &str, korean: &str, english: &str| {
        MenuItem::with_id(app, id, say(app, korean, english), true, None::<&str>)
    };
    let menu = Menu::new(app)?;
    if recording {
        menu.append(&item("seogi_stop", "녹음 끝내기", "Stop recording")?)?;
        menu.append(&item("seogi_mark", "중요 표시", "Mark this moment")?)?;
    } else {
        menu.append(&item("seogi_record", "녹음 시작", "Start recording")?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&item("settings", "서기 열기", "Open Seogi")?)?;
    menu.append(&item("quit", "끝내기", "Quit")?)?;
    Ok(menu)
}

/// Does what a tray menu item asks. Returns whether the item was Seogi's.
pub fn tray_action(app: &AppHandle, id: &str) -> bool {
    let Some(secretary) = Secretary::of(app) else {
        return false;
    };
    match id {
        "seogi_record" | "seogi_stop" => {
            std::thread::spawn(move || secretary.toggle_recording());
            true
        }
        "seogi_mark" => {
            let _ = secretary.mark();
            true
        }
        _ => false,
    }
}

/// Moves a file or folder to the system's trash. On macOS through the file
/// manager itself, not by asking Finder, which would make the system ask the
/// user for permission to control Finder.
pub fn to_trash(path: &Path) -> Result<Option<PathBuf>> {
    #[cfg(target_os = "macos")]
    {
        use objc2::rc::Retained;
        use objc2_foundation::{NSFileManager, NSString, NSURL};
        let text = path
            .to_str()
            .ok_or_else(|| anyhow!("the folder's name cannot be read"))?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(text));
        let mut landed: Option<Retained<NSURL>> = None;
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, Some(&mut landed))
            .map_err(|e| anyhow!("{}", e.localizedDescription()))?;
        Ok(landed
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string())))
    }
    #[cfg(not(target_os = "macos"))]
    {
        trash::delete(path).map_err(|e| anyhow!("{e}"))?;
        Ok(None)
    }
}

/// Puts back what [`to_trash`] moved: from where it landed (macOS), or by
/// asking the system's trash for the item that was at `original`.
fn from_trash(original: &Path, in_trash: Option<&Path>) -> Result<()> {
    if let Some(in_trash) = in_trash {
        return std::fs::rename(in_trash, original)
            .map_err(|e| anyhow!("it could not be put back ({e})"));
    }
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        let items: Vec<_> = trash::os_limited::list()
            .map_err(|e| anyhow!("{e}"))?
            .into_iter()
            .filter(|item| item.original_path() == original)
            .collect();
        if items.is_empty() {
            bail!("it is no longer in the trash");
        }
        trash::os_limited::restore_all(items).map_err(|e| anyhow!("{e}"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        bail!("it is no longer in the trash")
    }
}

/// Names as they are kept: composed the one way (a name typed and the same
/// name from a file can differ in how their Korean letters are encoded),
/// spaces tidied, and each once, whatever its case.
pub fn tidy_names(names: Vec<String>) -> Vec<String> {
    use unicode_normalization::UnicodeNormalization;
    let mut kept: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for name in names {
        let name: String = name.nfc().collect::<String>();
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        let key = name.to_lowercase();
        if !name.is_empty() && !seen.contains(&key) {
            seen.push(key);
            kept.push(name);
        }
    }
    kept
}

/// One task of the minutes as a naming keeps it: which task (its text and
/// the moment it rests on) and who was given it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct KeptTask {
    pub text: String,
    /// The first moment the task rests on, when it has one.
    pub at_ms: Option<u64>,
    pub owner: String,
}

/// Who said what and who was given each task, kept by the screen around a
/// naming of speakers, so the naming can be put back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Naming {
    pub speakers: Vec<Speaker>,
    /// Every line of the transcript, in order.
    pub lines: Vec<LineSpeaker>,
    /// Every task, in order: each minutes, its sections, their entries.
    pub tasks: Vec<KeptTask>,
}

/// Who said what and who was given each task in `meeting` now.
fn naming_of(meeting: &Meeting) -> Naming {
    let transcript = meeting.transcript.as_ref();
    Naming {
        speakers: transcript.map(|t| t.speakers.clone()).unwrap_or_default(),
        lines: transcript
            .map(|t| {
                t.segments
                    .iter()
                    .map(|s| LineSpeaker {
                        start_ms: s.start_ms,
                        speaker: s.speaker,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        tasks: meeting
            .minutes
            .iter()
            .flat_map(|m| &m.sections)
            .flat_map(|section| &section.entries)
            .map(|entry| KeptTask {
                text: entry.text.clone(),
                at_ms: entry.sources.first().map(|source| source.ms),
                owner: entry.owner.clone(),
            })
            .collect(),
    }
}

/// Puts `kept` (taken just before a naming) back into `meeting`, only when
/// the meeting is still exactly `now` (taken just after it): then what is
/// undone is that naming and nothing else. Nothing changes otherwise.
fn put_back_naming(meeting: &mut Meeting, kept: &Naming, now: &Naming) -> Result<()> {
    if naming_of(meeting) != *now {
        bail!("who said what has changed since");
    }
    let same_task = |a: &KeptTask, b: &KeptTask| a.text == b.text && a.at_ms == b.at_ms;
    if kept.tasks.len() != now.tasks.len()
        || kept
            .tasks
            .iter()
            .zip(&now.tasks)
            .any(|(a, b)| !same_task(a, b))
    {
        bail!("the minutes have changed since");
    }
    let mut transcript = meeting
        .transcript
        .clone()
        .ok_or_else(|| anyhow!("it has not been transcribed yet"))?;
    transcript
        .put_back_speakers(&kept.speakers, &kept.lines)
        .map_err(|e| anyhow!(e))?;
    meeting.transcript = Some(transcript);
    let entries = meeting
        .minutes
        .iter_mut()
        .flat_map(|m| m.sections.iter_mut())
        .flat_map(|section| section.entries.iter_mut());
    for (entry, task) in entries.zip(&kept.tasks) {
        entry.owner = task.owner.clone();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    #[ignore = "moves a folder to the real trash"]
    fn a_folder_goes_to_the_trash_whole_and_comes_back() {
        let home = std::env::var("HOME").unwrap();
        let dir = Path::new(&home).join(format!("seogi-trash-selftest-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("녹음.wav"), b"x").unwrap();
        let landed = to_trash(&dir).unwrap();
        assert!(!dir.exists());
        from_trash(&dir, landed.as_deref()).unwrap();
        assert!(dir.join("녹음.wav").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_naming_is_put_back_with_the_tasks_or_not_at_all() {
        use crate::seogi::minutes::{Entry, Minutes, MinutesSection, Source};
        use crate::seogi::templates::SectionKind;
        use crate::seogi::transcript::{Segment, Transcript};
        let task = |owner: &str, ms: u64| Entry {
            text: "도면 다시 그려 보내기".into(),
            owner: owner.into(),
            due: String::new(),
            sources: vec![Source {
                ms,
                quote: "도면".into(),
            }],
            reasons: vec![],
            done: false,
            faint: false,
        };
        let mut meeting = Meeting::new(0, store::Origin::Recorded, "녹음.wav");
        meeting.transcript = Some(Transcript {
            segments: [(0, 1), (1_000, 2), (2_000, 1)]
                .iter()
                .map(|&(start_ms, speaker)| Segment {
                    start_ms,
                    end_ms: start_ms + 900,
                    speaker,
                    text: "말".into(),
                    ..Default::default()
                })
                .collect(),
            speakers: vec![
                Speaker {
                    id: 1,
                    name: "화자 1".into(),
                },
                Speaker {
                    id: 2,
                    name: "화자 2".into(),
                },
            ],
            ..Default::default()
        });
        meeting.minutes = vec![Minutes {
            template_id: "consult".into(),
            template_name: "상담".into(),
            title: String::new(),
            summary: String::new(),
            story: vec![],
            feedback: None,
            sections: vec![MinutesSection {
                key: "our_tasks".into(),
                title: "우리 숙제".into(),
                kind: SectionKind::Actions,
                entries: vec![task("화자 1", 0), task("화자 2", 1_000)],
            }],
            verified: 0,
            to_check: 0,
            dropped: 0,
            not_rechecked: false,
            written_by: String::new(),
        }];
        let before = meeting.clone();
        let kept = naming_of(&meeting);
        // Named as the screen does it, tasks following.
        let named = meeting.transcript.as_mut().unwrap();
        named.name_speaker(1, "김서윤 실장", NameScope::All, 0, 0);
        meeting.minutes[0].sections[0].entries[0].owner = "김서윤 실장".into();
        let after = meeting.clone();
        let now = naming_of(&meeting);
        put_back_naming(&mut meeting, &kept, &now).unwrap();
        assert_eq!(meeting, before);

        // Refused, changing nothing: the case and what goes wrong.
        let refused = |change: &dyn Fn(&mut Meeting)| {
            let mut changed = after.clone();
            change(&mut changed);
            let untouched = changed.clone();
            assert!(put_back_naming(&mut changed, &kept, &now).is_err());
            assert_eq!(changed, untouched);
        };
        // Another name changed since.
        refused(&|m| {
            m.transcript.as_mut().unwrap().speakers[1].name = "고객".into();
        });
        // Minutes written again with as many tasks, but other ones: the old
        // owners would land on the wrong tasks.
        refused(&|m| {
            m.minutes[0].sections[0].entries[0].text = "견적서 다시 보내기".into();
        });
        refused(&|m| {
            m.minutes[0].sections[0].entries[1].sources[0].ms = 5_000;
        });
        // Fewer tasks.
        refused(&|m| {
            m.minutes[0].sections[0].entries.pop();
        });
        // A task given to someone else by hand since.
        refused(&|m| {
            m.minutes[0].sections[0].entries[1].owner = "박도현 소장".into();
        });
    }

    #[test]
    fn the_same_name_is_kept_once_however_it_was_written() {
        let decomposed: String = "보리"
            .chars()
            .flat_map(|c| {
                use unicode_normalization::UnicodeNormalization;
                c.to_string().nfd().collect::<Vec<_>>()
            })
            .collect();
        assert_ne!(decomposed, "보리");
        assert_eq!(
            tidy_names(vec![
                "보리".into(),
                " 민트 ".into(),
                "고객".into(),
                "고객".into(),
                decomposed,
                "Jamie  Lee".into(),
                "jamie lee".into(),
                " ".into(),
            ]),
            ["보리", "민트", "고객", "Jamie Lee"]
        );
    }

    #[test]
    fn a_calendar_is_checked_for_today_and_the_next_event() {
        let now = 1_795_500_000_000; // a moment on a working day
        let stamp = |ms: i64| {
            use chrono::TimeZone;
            chrono::Utc
                .timestamp_millis_opt(ms)
                .unwrap()
                .format("%Y%m%dT%H%M%SZ")
                .to_string()
        };
        let text = format!(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:다음 회의\r\nDTSTART:{}\r\nDTEND:{}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
            stamp(now + 3_600_000),
            stamp(now + 7_200_000)
        );
        let check = CalendarCheck::of("https://x/basic.ics".into(), Ok(&text), now);
        assert!(check.ok);
        assert_eq!(
            check.next.map(|event| event.title),
            Some("다음 회의".into())
        );

        let page = CalendarCheck::of(String::new(), Ok("<html>"), now);
        assert!(!page.ok);
        assert_eq!(page.reason, "not_calendar");

        // A public address of a calendar that is not public answers 404.
        let closed = CalendarCheck::of(
            String::new(),
            Err("the calendar answered 404 Not Found".into()),
            now,
        );
        assert_eq!(closed.reason, "refused");
        assert_eq!(
            public_closed(
                "https://calendar.google.com/calendar/ical/me%40example.com/public/basic.ics",
                &closed.reason
            ),
            "public_closed"
        );
        assert_eq!(
            public_closed(
                "https://calendar.google.com/calendar/ical/me%40example.com/private-1/basic.ics",
                &closed.reason
            ),
            "refused"
        );
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
