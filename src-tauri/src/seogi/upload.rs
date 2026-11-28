//! Meetings put in the user's Google Drive, for the phone to read.
//!
//! The phone and this computer share one Drive folder (`서기`, found by its
//! marker; see [`google`]). When Google is connected, each meeting whose
//! minutes are ready gets a folder of its own inside it, named like the
//! meeting's folder here and marked with the meeting's id, holding:
//!
//! - the original recording, uploaded as it is (resumable: an upload cut
//!   off goes on where it stopped), for a meeting recorded on this
//!   computer. A phone's recording is already in Drive: it and its JSON are
//!   moved (not copied) into the meeting's folder instead;
//! - `전사.txt` and `회의록.md`, as in the meeting's folder here;
//! - `회의.json`, what the apps read: the tasks with their ids, the times
//!   and the speakers ([`meeting_json`]).
//!
//! Only this computer writes those files. Nothing in Drive is deleted:
//! "드라이브에서 빼기" moves a meeting's Drive folder to Drive's trash, and
//! the meeting stays here. A recording that would leave the account with
//! less than 1 GB waits and the text still goes up; the user is told once.
//!
//! What was uploaded, and where, is kept in `drive-uploads.json` in the
//! app's data folder, so nothing goes up twice and an upload resumes after
//! a restart.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Context, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use specta::Type;

use super::edits::{self, Conflict, Edit, EditsFile, Side};
use super::google::{self, FileInfo, Google, Standing, Upload, Versioned};
use super::minutes::Entry;
use super::store::{self, Meeting, Origin, Status};
use super::templates::SectionKind;

const STATE_NAME: &str = "drive-uploads.json";
/// The file the apps read in each meeting's Drive folder.
pub const MEETING_JSON: &str = "회의.json";
/// Below this much room left in the account, recordings wait.
pub const KEEP_FREE: u64 = 1024 * 1024 * 1024;
/// The most a file of changes may be: far more than a meeting's changes.
const EDITS_LIMIT: usize = 4 * 1024 * 1024;
/// How often the queue is looked at when nothing wakes it.
const LOOK_EVERY: Duration = Duration::from_secs(10 * 60);

/// One file put in a meeting's Drive folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Sent {
    id: String,
    /// The SHA-256 of what was sent, so the same content is not sent again.
    sha256: String,
}

/// A resumable upload under way.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Session {
    url: String,
    name: String,
    sha256: String,
    size: u64,
}

/// What is known about one meeting in Drive.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Up {
    /// Its Drive folder's id; empty before it is made.
    #[serde(default)]
    folder: String,
    /// The name the folder was given last.
    #[serde(default)]
    folder_name: String,
    /// By file name.
    #[serde(default)]
    files: BTreeMap<String, Sent>,
    #[serde(default)]
    session: Option<Session>,
    /// To be put (or put again) in Drive at the next turn.
    #[serde(default)]
    wanted: bool,
    /// The user took it out of Drive ("드라이브에서 빼기"): never put there
    /// again until they ask.
    #[serde(default)]
    excluded: bool,
    /// Its Drive folder is still to go to the trash.
    #[serde(default)]
    to_trash: bool,
    /// Its recording waits for room in the account.
    #[serde(default)]
    audio_waits: bool,
    /// For a meeting from the phone: the phone's recording and its JSON in
    /// Drive, which move into the meeting's folder.
    #[serde(default)]
    phone_files: Vec<String>,
    /// Counts the times it was asked for, so a turn that read the meeting
    /// before a newer ask (its minutes written again meanwhile) leaves it
    /// asked for.
    #[serde(default)]
    asks: u64,
    /// This computer's changes the phone can make too, by key (see
    /// [`edits`]): written to `고친것-컴퓨터.json` in the meeting's folder.
    #[serde(default)]
    edits: BTreeMap<String, Edit>,
    /// The phone's changes, as last read from its `고친것-폰.json`.
    #[serde(default)]
    phone_edits: BTreeMap<String, Edit>,
    /// Drive's version of each of the phone's files (by file id) when they
    /// were last read. Normally one; two phones, or a reply lost while one
    /// was made, can leave more, and their changes are read as one.
    #[serde(default)]
    phone_edits_versions: BTreeMap<String, Option<u64>>,
    /// Changes made before this moment (in milliseconds since 1970) are for
    /// a transcript or minutes since replaced, on either side: they are
    /// left alone.
    #[serde(default)]
    edits_since: i64,
    /// Its folder last put in Drive's trash, with what was in it: taken out
    /// of the trash again when the meeting goes back to Drive, so nothing
    /// goes up twice and a phone's recording moved there comes back with it.
    #[serde(default)]
    trashed_folder: Option<TrashedFolder>,
}

impl Up {
    /// Whether the minutes are in Drive.
    fn uploaded(&self) -> bool {
        self.files.contains_key(MEETING_JSON)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    meetings: BTreeMap<String, Up>,
    /// The user has been told that recordings wait for room.
    #[serde(default)]
    low_space_said: bool,
}

impl Saved {
    fn load(data_dir: &Path) -> Self {
        std::fs::read_to_string(data_dir.join(STATE_NAME))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save(&self, data_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(data_dir)
            .with_context(|| format!("could not create {}", data_dir.display()))?;
        let text = serde_json::to_string_pretty(self)?;
        store::write_atomically(&data_dir.join(STATE_NAME), text.as_bytes())
    }
}

/// A meeting's folder in Drive's trash, and what had been sent into it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct TrashedFolder {
    id: String,
    #[serde(default)]
    files: BTreeMap<String, Sent>,
}

/// Where putting meetings in Drive stands, for the screen.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct DriveStatus {
    /// Uploading right now.
    pub uploading: bool,
    /// Meetings waiting to go up.
    pub waiting: u32,
    /// Meetings whose recording waits for room in the account.
    pub audio_waiting: u32,
    /// Meetings the user took out of Drive, by id.
    pub excluded: Vec<String>,
    /// Meetings in Drive, by id.
    pub uploaded: Vec<String>,
    /// Why the last turn failed: `offline`, `drive`, `local`, or empty.
    pub trouble: String,
}

/// Meetings done before Google was connected, that "지난 회의 올리기" would
/// put in Drive.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct Backlog {
    pub meetings: u32,
    /// About how many bytes would go up.
    pub bytes: u64,
}

/// What the uploading needs from the rest of the app.
pub trait Library {
    /// A meeting and its folder, with ids given to its tasks (and saved)
    /// when it has none yet.
    fn meeting(&self, id: &str) -> Result<Option<(Meeting, PathBuf)>>;
    /// The ids of the phone's recordings in Drive a meeting came from, as
    /// the ledger knows them.
    fn phone_audio(&self, id: &str) -> Vec<String>;
    /// Whether meetings recorded here go up, and their recordings with them.
    fn settings(&self) -> Settings;
    /// Tells the user, once, that recordings wait for room in Drive.
    fn low_space(&self);
    /// Tells the user, once, that Google has to be connected again.
    fn lost_sign_in(&self);
    /// The phone's file of changes came (or changed): `winning` holds the
    /// phone's changes that stand, to be made to the meeting here. Returns
    /// whether the meeting changed.
    fn phone_edits_arrived(
        &self,
        id: &str,
        winning: &[(String, serde_json::Value)],
    ) -> Result<bool>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// "이 컴퓨터에서 녹음한 회의도 올리기".
    pub own: bool,
    /// "녹음 파일도 올리기".
    pub audio: bool,
}

/// The Drive calls uploading makes.
pub trait Remote {
    /// Still signed in (the user may disconnect during a turn).
    fn connected(&self) -> bool;
    fn root(&self) -> Result<String>;
    fn info(&self, id: &str) -> Result<Option<FileInfo>>;
    fn find_meeting_folder(&self, root: &str, meeting_id: &str) -> Result<Option<String>>;
    fn find_named(&self, parent: &str, name: &str) -> Result<Option<FileInfo>>;
    fn make_meeting_folder(&self, root: &str, name: &str, meeting_id: &str) -> Result<String>;
    fn rename(&self, id: &str, name: &str) -> Result<()>;
    fn move_into(&self, id: &str, to: &str, from: &[String]) -> Result<()>;
    fn trash(&self, id: &str) -> Result<()>;
    fn untrash(&self, id: &str) -> Result<()>;
    fn create(&self, parent: &str, name: &str, mime: &str, bytes: &[u8]) -> Result<String>;
    fn update(&self, id: &str, mime: &str, bytes: &[u8]) -> Result<()>;
    fn free_space(&self) -> Result<Option<u64>>;
    fn start_upload(&self, parent: &str, name: &str, mime: &str, size: u64) -> Result<String>;
    fn upload_offset(&self, session: &str, size: u64) -> Result<Upload>;
    fn upload_chunk(&self, session: &str, file: &Path, offset: u64, size: u64) -> Result<Upload>;
    /// Every file called `name` the app may see, with its Drive version.
    fn find_everywhere(&self, name: &str) -> Result<Vec<Versioned>>;
    /// A small file's content.
    fn read(&self, id: &str) -> Result<Vec<u8>>;
    /// The folders in `root` that are in Drive's trash.
    fn trashed_folders(&self, root: &str) -> Result<Vec<String>>;
}

/// [`Remote`] through the user's Google sign-in.
struct Live<'a>(&'a Google);

impl Remote for Live<'_> {
    fn connected(&self) -> bool {
        matches!(self.0.standing(), Standing::Connected { .. })
    }
    fn root(&self) -> Result<String> {
        self.0.call(google::find_or_make_folder)
    }
    fn info(&self, id: &str) -> Result<Option<FileInfo>> {
        self.0.call(|token| google::file_info(token, id))
    }
    fn find_meeting_folder(&self, root: &str, meeting_id: &str) -> Result<Option<String>> {
        self.0
            .call(|token| google::find_meeting_folder(token, root, meeting_id))
    }
    fn find_named(&self, parent: &str, name: &str) -> Result<Option<FileInfo>> {
        self.0.call(|token| google::find_named(token, parent, name))
    }
    fn make_meeting_folder(&self, root: &str, name: &str, meeting_id: &str) -> Result<String> {
        self.0
            .call(|token| google::make_meeting_folder(token, root, name, meeting_id))
    }
    fn rename(&self, id: &str, name: &str) -> Result<()> {
        self.0.call(|token| google::rename(token, id, name))
    }
    fn move_into(&self, id: &str, to: &str, from: &[String]) -> Result<()> {
        self.0.call(|token| google::move_into(token, id, to, from))
    }
    fn trash(&self, id: &str) -> Result<()> {
        self.0.call(|token| google::trash(token, id))
    }
    fn untrash(&self, id: &str) -> Result<()> {
        self.0.call(|token| google::untrash(token, id))
    }
    fn create(&self, parent: &str, name: &str, mime: &str, bytes: &[u8]) -> Result<String> {
        self.0
            .call(|token| google::create_small(token, parent, name, mime, bytes))
    }
    fn update(&self, id: &str, mime: &str, bytes: &[u8]) -> Result<()> {
        self.0
            .call(|token| google::update_small(token, id, mime, bytes))
    }
    fn free_space(&self) -> Result<Option<u64>> {
        self.0.call(google::free_space)
    }
    fn start_upload(&self, parent: &str, name: &str, mime: &str, size: u64) -> Result<String> {
        self.0
            .call(|token| google::start_upload(token, parent, name, mime, size))
    }
    fn upload_offset(&self, session: &str, size: u64) -> Result<Upload> {
        self.0
            .call(|token| google::upload_offset(token, session, size))
    }
    fn upload_chunk(&self, session: &str, file: &Path, offset: u64, size: u64) -> Result<Upload> {
        self.0
            .call(|token| google::upload_chunk(token, session, file, offset, size))
    }
    fn find_everywhere(&self, name: &str) -> Result<Vec<Versioned>> {
        self.0.call(|token| google::find_everywhere(token, name))
    }
    fn read(&self, id: &str) -> Result<Vec<u8>> {
        self.0
            .call(|token| google::read_small(token, id, EDITS_LIMIT))
    }
    fn trashed_folders(&self, root: &str) -> Result<Vec<String>> {
        self.0.call(|token| google::trashed_folders(token, root))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// The meeting's Drive folder went to Drive's trash: it is not put there
/// again.
#[derive(Debug)]
struct TakenOut;

impl std::fmt::Display for TakenOut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("its Drive folder is in Drive's trash")
    }
}

impl std::error::Error for TakenOut {}

/// The meeting cannot take changes right now (it is being worked on): they
/// are read again at a later turn, and nothing is wrong.
#[derive(Debug)]
pub struct Later;

impl std::fmt::Display for Later {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the meeting is being worked on")
    }
}

impl std::error::Error for Later {}

/// Errors after which the rest of the turn would fail the same way: no
/// connection, the sign-in, or Drive itself busy or failing.
fn stops_the_turn(error: &anyhow::Error) -> bool {
    error.downcast_ref::<google::Offline>().is_some()
        || error.downcast_ref::<google::NeedsReconnect>().is_some()
        || matches!(
            error.downcast_ref::<google::HttpStatus>(),
            Some(google::HttpStatus(429 | 500..=599))
        )
}

fn is_not_found(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<google::HttpStatus>(),
        Some(google::HttpStatus(404))
    )
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The type Drive is told for a recording, by its ending.
fn audio_mime(name: &str) -> &'static str {
    let ending = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ending.as_str() {
        "wav" => "audio/wav",
        "m4a" | "mp4" | "aac" => "audio/mp4",
        "mp3" => "audio/mpeg",
        "ogg" | "opus" => "audio/ogg",
        "flac" => "audio/flac",
        "webm" => "audio/webm",
        _ => "application/octet-stream",
    }
}

/// Putting meetings in Drive.
pub struct Uploads {
    google: Arc<Google>,
    data_dir: PathBuf,
    saved: Mutex<Saved>,
    uploading: AtomicBool,
    trouble: Mutex<String>,
    wake: (Mutex<bool>, Condvar),
}

impl Uploads {
    pub fn new(data_dir: &Path, google: Arc<Google>) -> Self {
        Self {
            google,
            data_dir: data_dir.to_path_buf(),
            saved: Mutex::new(Saved::load(data_dir)),
            uploading: AtomicBool::new(false),
            trouble: Mutex::new(String::new()),
            wake: (Mutex::new(false), Condvar::new()),
        }
    }

    fn connected(&self) -> bool {
        matches!(self.google.standing(), Standing::Connected { .. })
    }

    fn change(&self, change: impl FnOnce(&mut Saved)) {
        let mut saved = lock(&self.saved);
        change(&mut saved);
        if let Err(e) = saved.save(&self.data_dir) {
            warn!("Could not save what was put in Drive: {e:#}");
        }
    }

    pub fn status(&self) -> DriveStatus {
        let saved = lock(&self.saved);
        let ids = |keep: &dyn Fn(&Up) -> bool| -> Vec<String> {
            saved
                .meetings
                .iter()
                .filter(|(_, up)| keep(up))
                .map(|(id, _)| id.clone())
                .collect()
        };
        DriveStatus {
            uploading: self.uploading.load(Ordering::SeqCst),
            waiting: saved
                .meetings
                .values()
                .filter(|up| up.wanted && !up.audio_waits && !up.excluded)
                .count() as u32,
            audio_waiting: saved.meetings.values().filter(|up| up.audio_waits).count() as u32,
            excluded: ids(&|up| up.excluded),
            uploaded: ids(&|up| up.uploaded() && !up.excluded),
            trouble: lock(&self.trouble).clone(),
        }
    }

    /// Whether the user took this meeting out of Drive.
    pub fn is_excluded(&self, id: &str) -> bool {
        lock(&self.saved)
            .meetings
            .get(id)
            .is_some_and(|up| up.excluded)
    }

    /// Whether a meeting is in Drive (or on its way there).
    pub fn in_drive(&self, id: &str) -> bool {
        lock(&self.saved)
            .meetings
            .get(id)
            .is_some_and(|up| !up.excluded && (up.uploaded() || up.wanted || !up.folder.is_empty()))
    }

    /// Has a meeting put in Drive (again, when it changed) at the next turn,
    /// unless the user took it out.
    pub fn want(&self, id: &str) {
        let mut wanted = false;
        self.change(|saved| {
            let up = saved.meetings.entry(id.to_string()).or_default();
            if !up.excluded {
                up.wanted = true;
                up.asks += 1;
                wanted = true;
            }
        });
        if wanted {
            self.wake_up();
        }
    }

    /// Notes the phone's recording and its JSON in Drive for a meeting
    /// brought in from the phone; they move into its folder once its
    /// minutes go up.
    pub fn note_phone_files(&self, id: &str, files: &[String]) {
        self.change(|saved| {
            let up = saved.meetings.entry(id.to_string()).or_default();
            for file in files.iter().filter(|file| !file.is_empty()) {
                if !up.phone_files.contains(file) {
                    up.phone_files.push(file.clone());
                }
            }
        });
    }

    /// "드라이브에서 빼기": the meeting's Drive folder goes to Drive's trash
    /// and the meeting is not put there again. The meeting here stays.
    pub fn exclude(&self, id: &str) {
        self.change(|saved| {
            let up = saved.meetings.entry(id.to_string()).or_default();
            up.excluded = true;
            up.wanted = false;
            up.to_trash = true;
            up.session = None;
            up.audio_waits = false;
        });
        self.wake_up();
    }

    /// Puts a meeting taken out back in Drive.
    pub fn include(&self, id: &str) {
        self.change(|saved| {
            let up = saved.meetings.entry(id.to_string()).or_default();
            up.excluded = false;
            up.to_trash = false;
            up.wanted = true;
            up.asks += 1;
        });
        self.wake_up();
    }

    /// The user changed a meeting here: `changes` (things the phone can
    /// change too) are noted with `at_ms`, and a meeting in Drive goes up
    /// again so the phone sees what changed.
    pub fn edited(&self, id: &str, changes: Vec<(String, serde_json::Value)>, at_ms: i64) {
        let mut again = false;
        self.change(|saved| {
            // A meeting never put in Drive has no one to tell: nothing is
            // kept, and a computer without Google stays as it was.
            if !saved.meetings.contains_key(id) {
                return;
            }
            let up = saved.meetings.entry(id.to_string()).or_default();
            for (key, value) in changes {
                up.edits.insert(key.clone(), Edit { key, value, at_ms });
            }
            // Also while it is on its way up: the turn sends it again.
            if (up.uploaded() || up.wanted) && !up.excluded {
                up.wanted = true;
                up.asks += 1;
                again = true;
            }
        });
        if again {
            self.wake_up();
        }
    }

    /// The meeting's transcript or minutes were replaced (transcribed or
    /// written again): the changes made so far, on either side, are for
    /// what was there before and are set aside from `at_ms` on.
    pub fn reset_edits(&self, id: &str, at_ms: i64) {
        self.change(|saved| {
            if let Some(up) = saved.meetings.get_mut(id) {
                up.edits.clear();
                up.phone_edits.clear();
                up.edits_since = at_ms;
            }
        });
    }

    /// The changes of a meeting made on both sides that say different
    /// things: the one that stands is in the meeting, the other is here.
    pub fn conflicts(&self, id: &str) -> Vec<Conflict> {
        let up = self.up(id);
        edits::merge(&up.edits, &up.phone_edits).1
    }

    /// The change of `key` that did not stand, to take instead.
    pub fn losing_edit(&self, id: &str, key: &str) -> Option<Edit> {
        let up = self.up(id);
        let (winners, _) = edits::merge(&up.edits, &up.phone_edits);
        let (_, side) = winners.get(key)?;
        let other = match side {
            Side::Computer => up.phone_edits.get(key),
            Side::Phone => up.edits.get(key),
        }?;
        (other.value != winners[key].0.value).then(|| other.clone())
    }

    pub fn wake_up(&self) {
        let (flag, signal) = &self.wake;
        *lock(flag) = true;
        signal.notify_all();
    }

    /// Waits until the next turn is due or asked for.
    pub fn wait(&self) {
        let (flag, signal) = &self.wake;
        let mut asked = lock(flag);
        if !*asked {
            asked = signal
                .wait_timeout(asked, LOOK_EVERY)
                .map(|(guard, _)| guard)
                .unwrap_or_else(|e| e.into_inner().0);
        }
        *asked = false;
    }

    /// One turn: does what is waiting. Does nothing while not connected.
    pub fn run(&self, library: &dyn Library) {
        if !self.connected() {
            return;
        }
        let busy = {
            let saved = lock(&self.saved);
            // A meeting in Drive may have changes from the phone to read.
            saved
                .meetings
                .values()
                .any(|up| up.wanted || up.to_trash || (up.uploaded() && !up.excluded))
        };
        if !busy || self.uploading.swap(true, Ordering::SeqCst) {
            return;
        }
        let result = self.run_once(&Live(&self.google), library);
        self.uploading.store(false, Ordering::SeqCst);
        let trouble = match result {
            Ok(()) => String::new(),
            Err(e) if e.downcast_ref::<google::NeedsReconnect>().is_some() => {
                if matches!(self.google.standing(), Standing::Lost { .. }) {
                    library.lost_sign_in();
                }
                String::new()
            }
            Err(e) => {
                warn!("Could not put meetings in Drive: {e:#}");
                if e.downcast_ref::<google::Offline>().is_some() {
                    "offline"
                } else if e.downcast_ref::<google::HttpStatus>().is_some() {
                    "drive"
                } else {
                    "local"
                }
                .to_string()
            }
        };
        *lock(&self.trouble) = trouble;
    }

    fn run_once(&self, remote: &dyn Remote, library: &dyn Library) -> Result<()> {
        let root = remote.root()?;
        let to_trash: Vec<String> = lock(&self.saved)
            .meetings
            .iter()
            .filter(|(_, up)| up.to_trash)
            .map(|(id, _)| id.clone())
            .collect();
        let mut local_trouble = None;
        for id in to_trash {
            match self.trash_folder(remote, &root, &id) {
                Ok(()) => {}
                Err(e) if stops_the_turn(&e) => return Err(e),
                Err(e) => {
                    // That one folder: tried again at the next turn, and
                    // the uploads still go on.
                    warn!("Meeting {id}'s Drive folder could not go to the trash yet: {e:#}");
                    local_trouble = Some(e);
                }
            }
        }
        if let Err(e) = self.notice_trashed_folders(remote, &root) {
            if stops_the_turn(&e) {
                return Err(e);
            }
            warn!("Could not look for meeting folders in Drive's trash: {e:#}");
        }
        match self.pull_phone_edits(remote, library) {
            Ok(()) => {}
            Err(e) if stops_the_turn(&e) => return Err(e),
            Err(e) => {
                warn!("The phone's changes could not be read yet: {e:#}");
                local_trouble = Some(e);
            }
        }
        let wanted: Vec<String> = lock(&self.saved)
            .meetings
            .iter()
            .filter(|(_, up)| up.wanted && !up.excluded)
            .map(|(id, _)| id.clone())
            .collect();
        for id in wanted {
            if !remote.connected() {
                break;
            }
            match self.upload_meeting(remote, library, &root, &id) {
                Ok(()) => {}
                Err(e) if e.downcast_ref::<TakenOut>().is_some() => {}
                Err(e) if stops_the_turn(&e) => return Err(e),
                Err(e) => {
                    // Its own trouble (a file that cannot be read here, one
                    // refused file): the others go on, it is tried again at
                    // the next turn.
                    warn!("Meeting {id} could not be put in Drive yet: {e:#}");
                    local_trouble = Some(e);
                }
            }
        }
        match local_trouble {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Notes that a meeting's Drive folder was put in Drive's trash: the
    /// meeting is taken out of Drive and stays here.
    fn taken_out_in_drive(&self, id: &str) {
        info!("Meeting {id}'s Drive folder is in Drive's trash: it stays out of Drive");
        self.put_up(id, |up| {
            if !up.folder.is_empty() {
                up.trashed_folder = Some(TrashedFolder {
                    id: up.folder.clone(),
                    files: up.files.clone(),
                });
            }
            up.excluded = true;
            up.wanted = false;
            up.to_trash = false;
            up.audio_waits = false;
            up.session = None;
            up.folder.clear();
            up.folder_name.clear();
            up.files.clear();
        });
    }

    /// Meetings whose Drive folder was put in Drive's trash (on the phone,
    /// or by hand) are taken out of Drive: one search for all of them.
    fn notice_trashed_folders(&self, remote: &dyn Remote, root: &str) -> Result<()> {
        let folders: BTreeMap<String, String> = lock(&self.saved)
            .meetings
            .iter()
            .filter(|(_, up)| !up.excluded && !up.folder.is_empty())
            .map(|(id, up)| (up.folder.clone(), id.clone()))
            .collect();
        if folders.is_empty() {
            return Ok(());
        }
        for folder in remote.trashed_folders(root)? {
            if let Some(id) = folders.get(&folder) {
                self.taken_out_in_drive(id);
            }
        }
        Ok(())
    }

    /// Reads the phone's files of changes that are new or changed since the
    /// last turn (one search for all meetings) and makes the phone's
    /// changes that stand here. A meeting that changed goes up again.
    fn pull_phone_edits(&self, remote: &dyn Remote, library: &dyn Library) -> Result<()> {
        let folders: BTreeMap<String, String> = lock(&self.saved)
            .meetings
            .iter()
            .filter(|(_, up)| up.uploaded() && !up.excluded && !up.folder.is_empty())
            .map(|(id, up)| (up.folder.clone(), id.clone()))
            .collect();
        if folders.is_empty() {
            return Ok(());
        }
        let mut by_meeting: BTreeMap<String, Vec<Versioned>> = BTreeMap::new();
        for found in remote.find_everywhere(edits::PHONE_FILE)? {
            if let Some(id) = found.parents.iter().find_map(|p| folders.get(p)) {
                by_meeting.entry(id.clone()).or_default().push(found);
            }
        }
        let mut trouble = None;
        for (id, files) in by_meeting {
            let up = self.up(&id);
            let versions: BTreeMap<String, Option<u64>> =
                files.iter().map(|f| (f.id.clone(), f.version)).collect();
            if versions == up.phone_edits_versions && versions.values().all(Option::is_some) {
                continue;
            }
            match self.take_phone_edits(remote, library, &id, &files, &up, versions) {
                Ok(()) => {}
                Err(e) if e.downcast_ref::<Later>().is_some() => {}
                Err(e) if stops_the_turn(&e) => return Err(e),
                Err(e) => {
                    warn!("Meeting {id}'s changes from the phone could not be taken yet: {e:#}");
                    trouble = Some(e);
                }
            }
        }
        trouble.map_or(Ok(()), Err)
    }

    fn take_phone_edits(
        &self,
        remote: &dyn Remote,
        library: &dyn Library,
        id: &str,
        files: &[Versioned],
        up: &Up,
        versions: BTreeMap<String, Option<u64>>,
    ) -> Result<()> {
        // Every phone file of the meeting, read as one: the latest of each key.
        let mut theirs: BTreeMap<String, Edit> = BTreeMap::new();
        for file in files {
            let bytes = remote.read(&file.id)?;
            let Some(parsed) =
                EditsFile::parse(&bytes).filter(|f| f.meeting.is_empty() || f.meeting == id)
            else {
                continue;
            };
            for (key, edit) in parsed.by_key() {
                if edit.at_ms < up.edits_since {
                    continue;
                }
                if theirs.get(&key).is_none_or(|e| e.at_ms <= edit.at_ms) {
                    theirs.insert(key, edit);
                }
            }
        }
        let (winners, _) = edits::merge(&up.edits, &theirs);
        let winning: Vec<(String, serde_json::Value)> = winners
            .into_iter()
            .filter(|(_, (_, side))| *side == Side::Phone)
            .map(|(key, (edit, _))| (key, edit.value))
            .collect();
        // Kept first, so the faint lines are there when the screen hears of it.
        self.put_up(id, |up| up.phone_edits = theirs);
        let changed = library.phone_edits_arrived(id, &winning)?;
        self.put_up(id, |up| up.phone_edits_versions = versions);
        if changed {
            info!("Meeting {id} took changes made on the phone");
            self.want(id);
        }
        Ok(())
    }

    fn up(&self, id: &str) -> Up {
        lock(&self.saved)
            .meetings
            .get(id)
            .cloned()
            .unwrap_or_default()
    }

    fn put_up(&self, id: &str, change: impl FnOnce(&mut Up)) {
        self.change(|saved| change(saved.meetings.entry(id.to_string()).or_default()));
    }

    fn trash_folder(&self, remote: &dyn Remote, root: &str, id: &str) -> Result<()> {
        let up = self.up(id);
        let folder = if up.folder.is_empty() {
            remote.find_meeting_folder(root, id)?
        } else {
            Some(up.folder.clone())
        };
        let mut trashed = None;
        if let Some(folder) = folder {
            match remote.trash(&folder) {
                Ok(()) => {
                    info!("Meeting {id}'s Drive folder went to Drive's trash");
                    trashed = Some(TrashedFolder {
                        id: folder,
                        files: up.files.clone(),
                    });
                }
                Err(e) if is_not_found(&e) => {}
                Err(e) => return Err(e),
            }
        }
        self.put_up(id, |up| {
            up.trashed_folder = trashed;
            up.to_trash = false;
            up.folder.clear();
            up.folder_name.clear();
            up.files.clear();
            up.session = None;
        });
        Ok(())
    }

    /// The meeting's Drive folder, made when it has none (or it went to the
    /// trash), with the name of its folder here.
    fn folder_for(&self, remote: &dyn Remote, root: &str, id: &str, name: &str) -> Result<String> {
        let up = self.up(id);
        if !up.folder.is_empty() {
            match remote.info(&up.folder)? {
                Some(info) if !info.trashed => {
                    if info.name != name {
                        remote.rename(&up.folder, name)?;
                        self.put_up(id, |up| up.folder_name = name.to_string());
                    }
                    return Ok(up.folder);
                }
                Some(_) => {
                    // Put in Drive's trash (from the phone, or by hand):
                    // taken out of Drive, as "드라이브에서 빼기" does
                    // (decision 5 of #67). "드라이브에 다시 올리기" puts it back.
                    self.taken_out_in_drive(id);
                    return Err(TakenOut.into());
                }
                None => {
                    // Gone for good: what was in it is sent again into a
                    // new one.
                    self.put_up(id, |up| {
                        up.folder.clear();
                        up.files.clear();
                        up.session = None;
                    });
                }
            }
        }
        if let Some(trashed) = self.up(id).trashed_folder {
            // Back in Drive: its old folder comes out of the trash, with the
            // recording (and a phone's recording moved there) still in it.
            let back = match remote.untrash(&trashed.id) {
                Ok(()) => true,
                Err(e) if is_not_found(&e) => false,
                Err(e) => return Err(e),
            };
            self.put_up(id, |up| up.trashed_folder = None);
            if back {
                info!("Meeting {id}'s Drive folder came out of Drive's trash");
                remote.rename(&trashed.id, name)?;
                self.put_up(id, |up| {
                    up.folder = trashed.id.clone();
                    up.folder_name = name.to_string();
                    up.files = trashed.files.clone();
                });
                return Ok(trashed.id);
            }
        }
        let folder = match remote.find_meeting_folder(root, id)? {
            Some(found) => {
                remote.rename(&found, name)?;
                found
            }
            None => remote.make_meeting_folder(root, name, id)?,
        };
        self.put_up(id, |up| {
            up.folder = folder.clone();
            up.folder_name = name.to_string();
        });
        Ok(folder)
    }

    fn upload_meeting(
        &self,
        remote: &dyn Remote,
        library: &dyn Library,
        root: &str,
        id: &str,
    ) -> Result<()> {
        let asked = self.up(id).asks;
        let Some((meeting, folder)) = library.meeting(id)? else {
            // Deleted here meanwhile: Drive keeps what it has.
            self.put_up(id, |up| up.wanted = false);
            return Ok(());
        };
        if meeting.status != Status::Done || meeting.minutes.is_empty() {
            // The minutes are being written again: they go up when done.
            return Ok(());
        }
        let settings = library.settings();
        let phone = meeting.origin == Origin::Phone;
        if !phone && !settings.own {
            return Ok(());
        }
        let name = folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| store::Library::folder_name(meeting.started_at_ms, &meeting.title));
        let drive_folder = self.folder_for(remote, root, id, &name)?;

        let mut audio_in_folder = false;
        if phone {
            audio_in_folder = self.move_phone_files(remote, library, id, root, &drive_folder)?;
        } else if settings.audio {
            audio_in_folder =
                self.upload_audio(remote, library, id, &meeting, &folder, &drive_folder)?;
        } else {
            self.put_up(id, |up| {
                up.audio_waits = false;
                up.session = None;
            });
        }

        let transcript = std::fs::read(folder.join(store::TRANSCRIPT_NAME)).ok();
        let minutes = std::fs::read(folder.join(store::MINUTES_NAME)).ok();
        let record = meeting_json(&meeting, audio_in_folder, self.up(id).edits_since);
        let mine = self.up(id).edits;
        let computer_edits = (!mine.is_empty()).then(|| {
            serde_json::to_vec_pretty(&EditsFile::computer(id, &mine)).unwrap_or_default()
        });
        let files: [(&str, &str, Option<Vec<u8>>); 4] = [
            (
                store::TRANSCRIPT_NAME,
                "text/plain; charset=utf-8",
                transcript,
            ),
            (store::MINUTES_NAME, "text/markdown; charset=utf-8", minutes),
            (
                edits::COMPUTER_FILE,
                "application/json; charset=utf-8",
                computer_edits,
            ),
            // Last: the phone takes the meeting as ready when it is there.
            (
                MEETING_JSON,
                "application/json; charset=utf-8",
                Some(record),
            ),
        ];
        for (file, mime, bytes) in files {
            let Some(bytes) = bytes else { continue };
            self.send_small(remote, id, &drive_folder, file, mime, &bytes)?;
        }
        // A recording waiting for room keeps the meeting in the queue: it
        // goes up at a later turn, once there is room. So does a newer ask
        // (the minutes written again while this turn was sending): the
        // next turn sends what changed. One taken out meanwhile is not.
        self.put_up(id, |up| {
            if up.excluded {
                up.wanted = false;
                up.audio_waits = false;
            } else {
                up.wanted = up.audio_waits || up.asks != asked;
            }
        });
        info!("Meeting {id} is in Drive");
        Ok(())
    }

    /// Sends a small file, or new content for one sent before; the same
    /// content is not sent again.
    fn send_small(
        &self,
        remote: &dyn Remote,
        id: &str,
        drive_folder: &str,
        name: &str,
        mime: &str,
        bytes: &[u8],
    ) -> Result<()> {
        let sum = sha256(bytes);
        let before = self.up(id).files.get(name).cloned();
        if before.as_ref().is_some_and(|sent| sent.sha256 == sum) {
            return Ok(());
        }
        let file_id = match before {
            Some(sent) => match remote.update(&sent.id, mime, bytes) {
                Ok(()) => sent.id,
                Err(e) if is_not_found(&e) => remote.create(drive_folder, name, mime, bytes)?,
                Err(e) => return Err(e),
            },
            None => remote.create(drive_folder, name, mime, bytes)?,
        };
        self.put_up(id, |up| {
            up.files.insert(
                name.to_string(),
                Sent {
                    id: file_id,
                    sha256: sum,
                },
            );
        });
        Ok(())
    }

    /// Moves the phone's recording and its JSON into the meeting's Drive
    /// folder (the same files: nothing is copied or deleted). Returns
    /// whether the recording is there.
    fn move_phone_files(
        &self,
        remote: &dyn Remote,
        library: &dyn Library,
        id: &str,
        root: &str,
        drive_folder: &str,
    ) -> Result<bool> {
        let mut files = self.up(id).phone_files;
        if files.is_empty() {
            // Brought in before the files were noted: the ledger knows the
            // recording, and its JSON is the one beside it with its name.
            for audio in library.phone_audio(id) {
                let Some(info) = remote.info(&audio)? else {
                    continue;
                };
                files.push(audio);
                let stem = Path::new(&info.name)
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let parent = info.parents.first().map_or(root, String::as_str);
                if let Some(json) = remote.find_named(parent, &format!("{stem}.json"))? {
                    files.push(json.id);
                }
            }
            self.note_phone_files(id, &files);
        }
        let mut audio_here = false;
        for file in &files {
            let Some(info) = remote.info(file)? else {
                continue;
            };
            if info.trashed {
                continue;
            }
            if !info.parents.iter().any(|parent| parent == drive_folder) {
                remote.move_into(file, drive_folder, &info.parents)?;
                info!("Moved a phone recording's file into meeting {id}'s Drive folder");
            }
            if !info.name.to_lowercase().ends_with(".json") {
                audio_here = true;
            }
        }
        Ok(audio_here)
    }

    /// Uploads the original recording as it is, going on from where an
    /// earlier upload stopped. Returns whether it is in the folder; it
    /// waits (and the user is told once) while the account has too little
    /// room left.
    fn upload_audio(
        &self,
        remote: &dyn Remote,
        library: &dyn Library,
        id: &str,
        meeting: &Meeting,
        folder: &Path,
        drive_folder: &str,
    ) -> Result<bool> {
        let name = meeting.audio_file.clone();
        let path = folder.join(&name);
        let size = std::fs::metadata(&path)
            .with_context(|| format!("could not read {}", path.display()))?
            .len();
        let sum = if meeting.sha256.is_empty() {
            super::evidence::sha256_file(&path)?
        } else {
            meeting.sha256.to_lowercase()
        };
        let up = self.up(id);
        if up.files.get(&name).is_some_and(|sent| sent.sha256 == sum) {
            return Ok(true);
        }
        let mut offset = 0;
        let mut session = up
            .session
            .filter(|s| s.name == name && s.sha256 == sum && s.size == size)
            .map(|s| s.url);
        if let Some(url) = &session {
            match remote.upload_offset(url, size)? {
                Upload::At(at) => offset = at,
                Upload::Done(file_id) => {
                    return self.audio_sent(id, &name, file_id, sum);
                }
                Upload::Expired => session = None,
            }
        }
        let url = match session {
            Some(url) => url,
            None => {
                let free = remote.free_space()?;
                if free.is_some_and(|free| free < size.saturating_add(KEEP_FREE)) {
                    info!("Meeting {id}'s recording waits for room in Drive");
                    let mut tell = false;
                    self.change(|saved| {
                        if let Some(up) = saved.meetings.get_mut(id).filter(|up| !up.excluded) {
                            up.audio_waits = true;
                        }
                        tell = !saved.low_space_said;
                        saved.low_space_said = true;
                    });
                    if tell {
                        library.low_space();
                    }
                    return Ok(false);
                }
                let url = remote.start_upload(drive_folder, &name, audio_mime(&name), size)?;
                self.put_up(id, |up| {
                    up.session = Some(Session {
                        url: url.clone(),
                        name: name.clone(),
                        sha256: sum.clone(),
                        size,
                    });
                });
                url
            }
        };
        loop {
            if !remote.connected() {
                return Err(google::NeedsReconnect.into());
            }
            match remote.upload_chunk(&url, &path, offset, size)? {
                Upload::At(at) => {
                    if at <= offset && offset < size {
                        anyhow::bail!("Drive took none of the recording");
                    }
                    offset = at;
                }
                Upload::Done(file_id) => return self.audio_sent(id, &name, file_id, sum),
                Upload::Expired => {
                    self.put_up(id, |up| up.session = None);
                    anyhow::bail!("the upload's address expired; it starts again");
                }
            }
        }
    }

    fn audio_sent(&self, id: &str, name: &str, file_id: String, sum: String) -> Result<bool> {
        self.change(|saved| {
            let up = saved.meetings.entry(id.to_string()).or_default();
            up.files.insert(
                name.to_string(),
                Sent {
                    id: file_id,
                    sha256: sum,
                },
            );
            up.session = None;
            up.audio_waits = false;
            // There was room again: a later shortage is told again.
            if !saved.meetings.values().any(|up| up.audio_waits) {
                saved.low_space_said = false;
            }
        });
        info!("Meeting {id}'s recording is in Drive");
        Ok(true)
    }

    /// Meetings done here and not in Drive, that "지난 회의 올리기" puts
    /// there, from `meetings` (with their folders).
    pub fn backlog_of<'a>(
        &self,
        meetings: impl Iterator<Item = &'a (Meeting, PathBuf)>,
        settings: Settings,
    ) -> (Vec<String>, Backlog) {
        let saved = lock(&self.saved);
        let mut ids = Vec::new();
        let mut backlog = Backlog::default();
        for (meeting, folder) in meetings {
            if meeting.status != Status::Done || meeting.minutes.is_empty() {
                continue;
            }
            let phone = meeting.origin == Origin::Phone;
            if !phone && !settings.own {
                continue;
            }
            if saved
                .meetings
                .get(&meeting.id)
                .is_some_and(|up| up.excluded || up.wanted || up.uploaded())
            {
                continue;
            }
            let size = |name: &str| std::fs::metadata(folder.join(name)).map_or(0, |m| m.len());
            let mut bytes = size(store::TRANSCRIPT_NAME) * 2 + size(store::MINUTES_NAME);
            if !phone && settings.audio {
                bytes += size(&meeting.audio_file);
            }
            ids.push(meeting.id.clone());
            backlog.meetings += 1;
            backlog.bytes += bytes;
        }
        (ids, backlog)
    }
}

/// One task or line of the minutes, as the apps read it.
fn entry_json(entry: &Entry) -> serde_json::Value {
    json!({
        "id": entry.id,
        "text": entry.text,
        "owner": entry.owner,
        "due": entry.due,
        "done": entry.done,
        "needs_check": entry.needs_check(),
        "added": entry.added,
        "sources": entry.sources.iter().map(|s| json!({ "ms": s.ms, "quote": s.quote })).collect::<Vec<_>>(),
    })
}

/// `회의.json`: what the apps read of a meeting. The format is described in
/// `docs/drive-folder.md`; fields are only ever added.
/// `edits_since`: changes made before it are for a transcript or minutes
/// since replaced (see [`edits`]); 0 when there were none.
pub fn meeting_json(meeting: &Meeting, audio_in_folder: bool, edits_since: i64) -> Vec<u8> {
    let minutes = meeting.minutes.first();
    let transcript = meeting
        .transcript
        .as_ref()
        .map(super::transcript::Transcript::as_read);
    let polished = match (&meeting.polish, &meeting.transcript) {
        (Some(polish), Some(t)) => polish.shown(t),
        _ => Default::default(),
    };
    let sections: Vec<serde_json::Value> = minutes
        .map(|minutes| {
            minutes
                .sections
                .iter()
                .map(|section| {
                    json!({
                        "key": section.key,
                        "title": section.title,
                        "kind": match section.kind {
                            SectionKind::Actions => "tasks",
                            SectionKind::Items => "items",
                            SectionKind::Quotes => "quotes",
                        },
                        "entries": section.entries.iter().map(entry_json).collect::<Vec<_>>(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let tasks: Vec<serde_json::Value> = minutes
        .map(|minutes| {
            minutes
                .sections
                .iter()
                .filter(|section| section.kind == SectionKind::Actions)
                .flat_map(|section| section.entries.iter().map(entry_json))
                .collect()
        })
        .unwrap_or_default();
    let record = json!({
        "format": 1,
        "app": "seogi-desktop",
        "app_version": env!("CARGO_PKG_VERSION"),
        "id": meeting.id,
        "title": meeting.title,
        "started_at": store::iso_local(meeting.started_at_ms),
        "started_at_ms": meeting.started_at_ms,
        "duration_ms": meeting.duration_ms,
        "origin": meeting.origin.as_str(),
        "audio": {
            "file": meeting.audio_file,
            "sha256": meeting.sha256,
            "in_folder": audio_in_folder,
        },
        "kind": minutes.map_or("", |m| m.template_name.as_str()),
        "summary": minutes.map_or("", |m| m.summary.as_str()),
        "tasks": tasks,
        "story": minutes.map(|m| m.story.iter().map(|part| json!({
            "title": part.title,
            "text": part.text,
            "sources": part.sources.iter().map(|s| json!({ "ms": s.ms, "quote": s.quote })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>()).unwrap_or_default(),
        "sections": sections,
        "marks_ms": meeting.marks_ms,
        "edits_since": edits_since,
        "speakers": transcript.as_ref().map(|t| t.speakers.iter().map(|s| json!({
            "id": s.id,
            "name": t.speaker_name(s.id),
        })).collect::<Vec<_>>()).unwrap_or_default(),
        "lines": transcript.as_ref().map(|t| t.segments.iter().map(|segment| {
            let mut line = json!({
                "start_ms": segment.start_ms,
                "end_ms": segment.end_ms,
                "speaker": segment.speaker,
                "text": segment.text,
            });
            // The line polished for reading, when it is (#108); a line the
            // user fixed has none.
            if let Some(polished) = polished.get(&segment.start_ms) {
                line["polished"] = json!(polished);
            }
            line
        }).collect::<Vec<_>>()).unwrap_or_default(),
    });
    serde_json::to_vec_pretty(&record).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// An upload under way: its folder, its name, what came so far, its size.
    type FakeSession = (String, String, Vec<u8>, u64);

    /// A Drive in memory: folders and files by id.
    #[derive(Default)]
    struct FakeDrive {
        files: Mutex<HashMap<String, FileInfo>>,
        content: Mutex<HashMap<String, Vec<u8>>>,
        marks: Mutex<HashMap<String, String>>,
        free: Option<u64>,
        /// Each upload request takes at most this many bytes, and the
        /// first request after `cut_after` bytes fails as offline.
        chunk: u64,
        cut_after: Mutex<Option<u64>>,
        sessions: Mutex<HashMap<String, FakeSession>>,
        calls: Mutex<Vec<String>>,
        next: Mutex<u32>,
        /// Drive's version of each file: it goes up with every change.
        versions: Mutex<HashMap<String, u64>>,
    }

    impl FakeDrive {
        fn new_id(&self, prefix: &str) -> String {
            let mut next = lock(&self.next);
            *next += 1;
            format!("{prefix}{next}")
        }
        fn put_file(&self, id: &str, name: &str, parent: &str, bytes: &[u8]) {
            lock(&self.files).insert(
                id.into(),
                FileInfo {
                    id: id.into(),
                    name: name.into(),
                    parents: vec![parent.into()],
                    trashed: false,
                    sha256_checksum: None,
                },
            );
            lock(&self.content).insert(id.into(), bytes.to_vec());
            *lock(&self.versions).entry(id.into()).or_default() += 1;
        }
        fn in_folder(&self, folder: &str) -> Vec<String> {
            let mut names: Vec<String> = lock(&self.files)
                .values()
                .filter(|f| f.parents.iter().any(|p| p == folder) && !f.trashed)
                .map(|f| f.name.clone())
                .collect();
            names.sort();
            names
        }
        fn called(&self, what: &str) -> usize {
            lock(&self.calls)
                .iter()
                .filter(|c| c.as_str() == what)
                .count()
        }
        fn note(&self, what: &str) {
            lock(&self.calls).push(what.to_string());
        }
    }

    impl Remote for FakeDrive {
        fn connected(&self) -> bool {
            true
        }
        fn root(&self) -> Result<String> {
            Ok("ROOT".into())
        }
        fn info(&self, id: &str) -> Result<Option<FileInfo>> {
            Ok(lock(&self.files).get(id).cloned())
        }
        fn find_meeting_folder(&self, root: &str, meeting_id: &str) -> Result<Option<String>> {
            let files = lock(&self.files);
            Ok(lock(&self.marks)
                .iter()
                .find(|(folder, id)| {
                    id.as_str() == meeting_id
                        && files
                            .get(folder.as_str())
                            .is_some_and(|f| !f.trashed && f.parents.iter().any(|p| p == root))
                })
                .map(|(folder, _)| folder.clone()))
        }
        fn find_named(&self, parent: &str, name: &str) -> Result<Option<FileInfo>> {
            Ok(lock(&self.files)
                .values()
                .find(|f| f.name == name && f.parents.iter().any(|p| p == parent) && !f.trashed)
                .cloned())
        }
        fn make_meeting_folder(&self, root: &str, name: &str, meeting_id: &str) -> Result<String> {
            self.note("mkdir");
            let id = self.new_id("D");
            self.put_file(&id, name, root, b"");
            lock(&self.marks).insert(id.clone(), meeting_id.into());
            Ok(id)
        }
        fn rename(&self, id: &str, name: &str) -> Result<()> {
            self.note("rename");
            lock(&self.files).get_mut(id).unwrap().name = name.into();
            Ok(())
        }
        fn move_into(&self, id: &str, to: &str, from: &[String]) -> Result<()> {
            self.note("move");
            let mut files = lock(&self.files);
            let file = files.get_mut(id).unwrap();
            file.parents.retain(|p| !from.contains(p));
            file.parents.push(to.into());
            Ok(())
        }
        fn trash(&self, id: &str) -> Result<()> {
            self.note("trash");
            lock(&self.files).get_mut(id).unwrap().trashed = true;
            Ok(())
        }
        fn untrash(&self, id: &str) -> Result<()> {
            self.note("untrash");
            match lock(&self.files).get_mut(id) {
                Some(f) => {
                    f.trashed = false;
                    Ok(())
                }
                None => Err(google::HttpStatus(404).into()),
            }
        }
        fn create(&self, parent: &str, name: &str, _mime: &str, bytes: &[u8]) -> Result<String> {
            self.note("create");
            let id = self.new_id("F");
            self.put_file(&id, name, parent, bytes);
            Ok(id)
        }
        fn update(&self, id: &str, _mime: &str, bytes: &[u8]) -> Result<()> {
            self.note("update");
            lock(&self.content).insert(id.into(), bytes.to_vec());
            *lock(&self.versions).entry(id.into()).or_default() += 1;
            Ok(())
        }
        fn find_everywhere(&self, name: &str) -> Result<Vec<Versioned>> {
            let versions = lock(&self.versions);
            Ok(lock(&self.files)
                .values()
                .filter(|f| f.name == name && !f.trashed)
                .map(|f| Versioned {
                    id: f.id.clone(),
                    parents: f.parents.clone(),
                    version: versions.get(&f.id).copied(),
                })
                .collect())
        }
        fn read(&self, id: &str) -> Result<Vec<u8>> {
            self.note("read");
            Ok(lock(&self.content).get(id).cloned().unwrap_or_default())
        }
        fn trashed_folders(&self, root: &str) -> Result<Vec<String>> {
            Ok(lock(&self.files)
                .values()
                .filter(|f| f.trashed && f.parents.iter().any(|p| p == root))
                .map(|f| f.id.clone())
                .collect())
        }
        fn free_space(&self) -> Result<Option<u64>> {
            Ok(self.free)
        }
        fn start_upload(&self, parent: &str, name: &str, _mime: &str, size: u64) -> Result<String> {
            self.note("start_upload");
            let url = self.new_id("U");
            lock(&self.sessions)
                .insert(url.clone(), (parent.into(), name.into(), Vec::new(), size));
            Ok(url)
        }
        fn upload_offset(&self, session: &str, _size: u64) -> Result<Upload> {
            Ok(match lock(&self.sessions).get(session) {
                Some((_, _, bytes, _)) => Upload::At(bytes.len() as u64),
                None => Upload::Expired,
            })
        }
        fn upload_chunk(
            &self,
            session: &str,
            file: &Path,
            offset: u64,
            size: u64,
        ) -> Result<Upload> {
            self.note("chunk");
            {
                let mut cut = lock(&self.cut_after);
                if cut.is_some_and(|after| offset >= after) {
                    *cut = None;
                    return Err(google::Offline("cut off".into()).into());
                }
            }
            let all = std::fs::read(file)?;
            let mut sessions = lock(&self.sessions);
            let (parent, name, bytes, _) = sessions
                .get_mut(session)
                .ok_or_else(|| anyhow::anyhow!("no session"))?;
            assert_eq!(
                bytes.len() as u64,
                offset,
                "an upload goes on where Drive is"
            );
            let end = (offset + self.chunk.max(1)).min(size);
            bytes.extend_from_slice(&all[offset as usize..end as usize]);
            if end < size {
                return Ok(Upload::At(end));
            }
            let (parent, name, bytes) = (parent.clone(), name.clone(), bytes.clone());
            sessions.remove(session);
            drop(sessions);
            let id = self.new_id("A");
            self.put_file(&id, &name, &parent, &bytes);
            Ok(Upload::Done(id))
        }
    }

    struct FakeLibrary {
        meetings: Mutex<HashMap<String, (Meeting, PathBuf)>>,
        settings: Settings,
        told_low: Mutex<u32>,
        phone_audio: Vec<String>,
    }

    impl FakeLibrary {
        fn new() -> Self {
            Self {
                meetings: Mutex::new(HashMap::new()),
                settings: Settings {
                    own: true,
                    audio: true,
                },
                told_low: Mutex::new(0),
                phone_audio: Vec::new(),
            }
        }
    }

    impl Library for FakeLibrary {
        fn meeting(&self, id: &str) -> Result<Option<(Meeting, PathBuf)>> {
            Ok(lock(&self.meetings).get(id).cloned())
        }
        fn phone_audio(&self, _id: &str) -> Vec<String> {
            self.phone_audio.clone()
        }
        fn settings(&self) -> Settings {
            self.settings
        }
        fn low_space(&self) {
            *lock(&self.told_low) += 1;
        }
        fn lost_sign_in(&self) {}
        fn phone_edits_arrived(
            &self,
            id: &str,
            winning: &[(String, serde_json::Value)],
        ) -> Result<bool> {
            let mut meetings = lock(&self.meetings);
            let Some((meeting, _)) = meetings.get_mut(id) else {
                return Ok(false);
            };
            let mut changed = false;
            for (key, value) in winning {
                changed |= edits::apply(meeting, key, value);
            }
            Ok(changed)
        }
    }

    /// A done meeting in its own folder under `library`.
    fn meeting(library: &Path, id: &str, origin: Origin, audio: &[u8]) -> (Meeting, PathBuf) {
        let folder = library.join(format!("2026-11-27 1000 회의 {id}"));
        std::fs::create_dir_all(&folder).unwrap();
        let mut meeting = Meeting::new(1_795_700_000_000, origin, store::RECORDING_NAME);
        meeting.id = id.into();
        meeting.title = format!("회의 {id}");
        meeting.status = Status::Done;
        meeting.sha256 = sha256(audio);
        meeting.minutes = vec![serde_json::from_value(json!({
            "template_id": "meeting", "template_name": "회의", "title": "", "summary": "한 줄",
            "sections": [{"key": "actions", "title": "할 일", "kind": "actions", "entries": [
                {"id": "t1", "text": "견적서 보내기", "owner": "가", "due": "금요일",
                 "sources": [{"ms": 61000, "quote": "견적서는 금요일까지"}], "reasons": []}
            ]}],
            "verified": 1, "to_check": 0, "dropped": 0, "written_by": "test"
        }))
        .unwrap()];
        std::fs::write(folder.join(store::RECORDING_NAME), audio).unwrap();
        std::fs::write(
            folder.join(store::TRANSCRIPT_NAME),
            "[00:01] 화자 1: 안녕하세요",
        )
        .unwrap();
        std::fs::write(folder.join(store::MINUTES_NAME), "# 회의").unwrap();
        (meeting, folder)
    }

    fn uploads(dir: &Path) -> Uploads {
        Uploads::new(dir, Arc::new(Google::new(dir)))
    }

    #[test]
    fn the_polished_lines_go_up_beside_the_lines_as_said() {
        use crate::seogi::polish::{Polish, PolishedLine};
        use crate::seogi::transcript::{Segment, Transcript};
        let mut meeting = Meeting::new(0, Origin::Recorded, "녹음.wav");
        let line = |start_ms: u64, text: &str| Segment {
            start_ms,
            end_ms: start_ms + 1_000,
            speaker: 1,
            text: text.into(),
            ..Default::default()
        };
        let mut fixed = line(2_000, "어 그건 안돼요");
        fixed.edited = Some("그건 안 돼요!".into());
        meeting.transcript = Some(Transcript {
            segments: vec![line(0, "음 안녕하세요"), fixed],
            ..Default::default()
        });
        meeting.polish = Some(Polish {
            lines: vec![
                PolishedLine {
                    start_ms: 0,
                    from: "음 안녕하세요".into(),
                    text: "안녕하세요.".into(),
                },
                PolishedLine {
                    start_ms: 2_000,
                    from: "어 그건 안돼요".into(),
                    text: "그건 안 돼요.".into(),
                },
            ],
            ..Default::default()
        });
        let json: serde_json::Value =
            serde_json::from_slice(&meeting_json(&meeting, false, 0)).unwrap();
        let lines = json["lines"].as_array().unwrap();
        assert_eq!(lines[0]["text"], "음 안녕하세요");
        assert_eq!(lines[0]["polished"], "안녕하세요.");
        // The user's fix wins: no polished words for that line.
        assert_eq!(lines[1]["text"], "그건 안 돼요!");
        assert!(lines[1].get("polished").is_none());
    }

    #[test]
    fn a_meeting_goes_up_once_with_its_recording_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        let drive = FakeDrive {
            chunk: 4,
            ..FakeDrive::default()
        };
        let library = FakeLibrary::new();
        let audio = b"RIFF-0123456789";
        let (m, folder) = meeting(dir.path(), "m1", Origin::Recorded, audio);
        lock(&library.meetings).insert("m1".into(), (m, folder.clone()));

        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        let drive_folder = uploads.up("m1").folder;
        assert_eq!(
            drive.in_folder(&drive_folder),
            vec!["녹음.wav", "전사.txt", "회의.json", "회의록.md"]
        );
        assert_eq!(drive.in_folder("ROOT"), vec!["2026-11-27 1000 회의 m1"]);
        let sent_audio = &uploads.up("m1").files["녹음.wav"];
        assert_eq!(lock(&drive.content)[&sent_audio.id], audio.to_vec());
        // The original here is as it was.
        assert_eq!(
            std::fs::read(folder.join("녹음.wav")).unwrap(),
            audio.to_vec()
        );
        let json: serde_json::Value =
            serde_json::from_slice(&lock(&drive.content)[&uploads.up("m1").files[MEETING_JSON].id])
                .unwrap();
        assert_eq!(json["tasks"][0]["id"], "t1");
        assert_eq!(json["tasks"][0]["sources"][0]["ms"], 61000);
        assert_eq!(json["audio"]["in_folder"], true);
        assert_eq!(uploads.status().waiting, 0);

        // Asked again with nothing changed: nothing is sent.
        let creates = drive.called("create");
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        assert_eq!(drive.called("create"), creates);
        assert_eq!(drive.called("update"), 0);
        assert_eq!(drive.called("start_upload"), 1);
        assert_eq!(drive.called("mkdir"), 1);

        // Minutes written again: only what changed goes up.
        std::fs::write(folder.join(store::MINUTES_NAME), "# 회의 (다시)").unwrap();
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        assert_eq!(drive.called("update"), 1);
        assert_eq!(drive.called("create"), creates);
    }

    #[test]
    fn an_upload_cut_off_goes_on_where_it_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let drive = FakeDrive {
            chunk: 3,
            cut_after: Mutex::new(Some(6)),
            ..FakeDrive::default()
        };
        let library = FakeLibrary::new();
        let audio = b"abcdefghijklmnop";
        let (m, folder) = meeting(dir.path(), "m1", Origin::Recorded, audio);
        lock(&library.meetings).insert("m1".into(), (m, folder));
        {
            let uploads = uploads(&data);
            uploads.want("m1");
            let error = uploads.run_once(&drive, &library).unwrap_err();
            assert!(error.downcast_ref::<google::Offline>().is_some());
            assert!(uploads.up("m1").wanted);
        }
        // After a restart, from the saved address and Drive's offset.
        let uploads = uploads(&data);
        uploads.run_once(&drive, &library).unwrap();
        assert_eq!(drive.called("start_upload"), 1);
        let sent = &uploads.up("m1").files["녹음.wav"];
        assert_eq!(lock(&drive.content)[&sent.id], audio.to_vec());
        assert!(!uploads.up("m1").wanted);
        assert!(uploads.up("m1").session.is_none());
    }

    #[test]
    fn with_little_room_the_text_goes_up_and_the_recording_waits() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        let drive = FakeDrive {
            chunk: 100,
            free: Some(KEEP_FREE - 1),
            ..FakeDrive::default()
        };
        let library = FakeLibrary::new();
        for id in ["m1", "m2"] {
            let (m, folder) = meeting(dir.path(), id, Origin::Recorded, b"audio");
            lock(&library.meetings).insert(id.into(), (m, folder));
            uploads.want(id);
        }
        uploads.run_once(&drive, &library).unwrap();
        let folder = uploads.up("m1").folder;
        assert_eq!(
            drive.in_folder(&folder),
            vec!["전사.txt", "회의.json", "회의록.md"]
        );
        assert_eq!(drive.called("start_upload"), 0);
        // Told once, for both.
        assert_eq!(*lock(&library.told_low), 1);
        assert_eq!(uploads.status().audio_waiting, 2);
        let json: serde_json::Value =
            serde_json::from_slice(&lock(&drive.content)[&uploads.up("m1").files[MEETING_JSON].id])
                .unwrap();
        assert_eq!(json["audio"]["in_folder"], false);
    }

    #[test]
    fn without_recordings_only_the_text_goes_up() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        let drive = FakeDrive::default();
        let mut library = FakeLibrary::new();
        library.settings.audio = false;
        let (m, folder) = meeting(dir.path(), "m1", Origin::Recorded, b"audio");
        lock(&library.meetings).insert("m1".into(), (m, folder));
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        assert_eq!(drive.in_folder(&uploads.up("m1").folder).len(), 3);
        assert_eq!(drive.called("start_upload"), 0);
        assert_eq!(*lock(&library.told_low), 0);
    }

    #[test]
    fn a_phone_recording_and_its_json_move_into_the_meetings_folder() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        let drive = FakeDrive::default();
        drive.put_file("PA", "2026-11-26 1842 회의.m4a", "ROOT", b"phone audio");
        drive.put_file("PJ", "2026-11-26 1842 회의.json", "ROOT", b"{}");
        drive.put_file("OTHER", "다른 녹음.m4a", "ROOT", b"x");
        let mut library = FakeLibrary::new();
        library.phone_audio = vec!["PA".into()];
        let (m, folder) = meeting(dir.path(), "m1", Origin::Phone, b"phone audio");
        lock(&library.meetings).insert("m1".into(), (m, folder));

        // Brought in before the files were noted: found by the ledger and
        // the JSON's name.
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        let drive_folder = uploads.up("m1").folder;
        assert_eq!(
            drive.in_folder(&drive_folder),
            vec![
                "2026-11-26 1842 회의.json",
                "2026-11-26 1842 회의.m4a",
                "전사.txt",
                "회의.json",
                "회의록.md"
            ]
        );
        assert_eq!(
            drive.in_folder("ROOT"),
            vec!["2026-11-27 1000 회의 m1", "다른 녹음.m4a"]
        );
        // Moved, not uploaded again; the same bytes under the same id.
        assert_eq!(drive.called("start_upload"), 0);
        assert_eq!(lock(&drive.content)["PA"], b"phone audio".to_vec());
        // Again: nothing more moves.
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        assert_eq!(drive.called("move"), 2);
    }

    #[test]
    fn taken_out_of_drive_it_goes_to_the_trash_and_stays_out() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        let drive = FakeDrive::default();
        let library = FakeLibrary::new();
        let (m, folder) = meeting(dir.path(), "m1", Origin::Recorded, b"audio");
        lock(&library.meetings).insert("m1".into(), (m, folder.clone()));
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        let drive_folder = uploads.up("m1").folder;

        uploads.exclude("m1");
        uploads.run_once(&drive, &library).unwrap();
        assert!(lock(&drive.files)[&drive_folder].trashed);
        assert!(uploads.is_excluded("m1"));
        // Asked for again (the minutes written again): it stays out.
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        assert_eq!(drive.called("mkdir"), 1);
        // The meeting here is untouched.
        assert!(folder.join("녹음.wav").exists());
        assert_eq!(uploads.status().excluded, vec!["m1".to_string()]);

        // Put back: the same folder comes out of the trash, and the
        // recording is not sent again.
        let folder_id = lock(&drive.files)
            .values()
            .find(|f| f.trashed && f.parents == vec!["ROOT".to_string()])
            .map(|f| f.id.clone())
            .unwrap();
        let chunks = drive.called("chunk");
        uploads.include("m1");
        uploads.run_once(&drive, &library).unwrap();
        assert_eq!(drive.called("mkdir"), 1);
        assert_eq!(drive.called("untrash"), 1);
        assert_eq!(uploads.up("m1").folder, folder_id);
        assert_eq!(drive.called("chunk"), chunks);
        assert!(!uploads.is_excluded("m1"));

        // Deleted for good meanwhile: a new folder.
        uploads.exclude("m1");
        uploads.run_once(&drive, &library).unwrap();
        lock(&drive.files).remove(&folder_id);
        uploads.include("m1");
        uploads.run_once(&drive, &library).unwrap();
        assert_eq!(drive.called("mkdir"), 2);
    }

    /// A library whose meeting is asked for again while it is being read,
    /// as when its minutes are written again during a long upload.
    struct AskedAgain<'a> {
        inner: FakeLibrary,
        uploads: &'a Uploads,
        once: AtomicBool,
    }

    impl Library for AskedAgain<'_> {
        fn meeting(&self, id: &str) -> Result<Option<(Meeting, PathBuf)>> {
            let found = self.inner.meeting(id);
            if !self.once.swap(true, Ordering::SeqCst) {
                self.uploads.want(id);
            }
            found
        }
        fn phone_audio(&self, id: &str) -> Vec<String> {
            self.inner.phone_audio(id)
        }
        fn settings(&self) -> Settings {
            self.inner.settings
        }
        fn low_space(&self) {}
        fn lost_sign_in(&self) {}
        fn phone_edits_arrived(
            &self,
            id: &str,
            winning: &[(String, serde_json::Value)],
        ) -> Result<bool> {
            self.inner.phone_edits_arrived(id, winning)
        }
    }

    #[test]
    fn asked_again_during_a_turn_it_goes_up_again_at_the_next() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        let drive = FakeDrive::default();
        let inner = FakeLibrary::new();
        let (m, folder) = meeting(dir.path(), "m1", Origin::Recorded, b"audio");
        lock(&inner.meetings).insert("m1".into(), (m, folder));
        let library = AskedAgain {
            inner,
            uploads: &uploads,
            once: AtomicBool::new(false),
        };
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        assert!(uploads.up("m1").wanted);
        uploads.run_once(&drive, &library).unwrap();
        assert!(!uploads.up("m1").wanted);
    }

    #[test]
    fn a_folder_that_will_not_go_to_the_trash_holds_up_nothing_else() {
        struct Stubborn(FakeDrive);
        impl Remote for Stubborn {
            fn connected(&self) -> bool {
                true
            }
            fn root(&self) -> Result<String> {
                self.0.root()
            }
            fn info(&self, id: &str) -> Result<Option<FileInfo>> {
                self.0.info(id)
            }
            fn find_meeting_folder(&self, r: &str, m: &str) -> Result<Option<String>> {
                self.0.find_meeting_folder(r, m)
            }
            fn find_named(&self, p: &str, n: &str) -> Result<Option<FileInfo>> {
                self.0.find_named(p, n)
            }
            fn make_meeting_folder(&self, r: &str, n: &str, m: &str) -> Result<String> {
                self.0.make_meeting_folder(r, n, m)
            }
            fn rename(&self, id: &str, n: &str) -> Result<()> {
                self.0.rename(id, n)
            }
            fn find_everywhere(&self, n: &str) -> Result<Vec<Versioned>> {
                self.0.find_everywhere(n)
            }
            fn read(&self, id: &str) -> Result<Vec<u8>> {
                self.0.read(id)
            }
            fn trashed_folders(&self, r: &str) -> Result<Vec<String>> {
                self.0.trashed_folders(r)
            }
            fn move_into(&self, id: &str, t: &str, f: &[String]) -> Result<()> {
                self.0.move_into(id, t, f)
            }
            fn trash(&self, _id: &str) -> Result<()> {
                Err(google::HttpStatus(403).into())
            }
            fn untrash(&self, id: &str) -> Result<()> {
                self.0.untrash(id)
            }
            fn create(&self, p: &str, n: &str, m: &str, b: &[u8]) -> Result<String> {
                self.0.create(p, n, m, b)
            }
            fn update(&self, id: &str, m: &str, b: &[u8]) -> Result<()> {
                self.0.update(id, m, b)
            }
            fn free_space(&self) -> Result<Option<u64>> {
                self.0.free_space()
            }
            fn start_upload(&self, p: &str, n: &str, m: &str, s: u64) -> Result<String> {
                self.0.start_upload(p, n, m, s)
            }
            fn upload_offset(&self, s: &str, z: u64) -> Result<Upload> {
                self.0.upload_offset(s, z)
            }
            fn upload_chunk(&self, s: &str, f: &Path, o: u64, z: u64) -> Result<Upload> {
                self.0.upload_chunk(s, f, o, z)
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        let drive = Stubborn(FakeDrive {
            chunk: 100,
            ..FakeDrive::default()
        });
        let library = FakeLibrary::new();
        for id in ["m1", "m2"] {
            let (m, folder) = meeting(dir.path(), id, Origin::Recorded, b"audio");
            lock(&library.meetings).insert(id.into(), (m, folder));
        }
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        uploads.exclude("m1");
        uploads.want("m2");
        assert!(uploads.run_once(&drive, &library).is_err());
        assert!(uploads.up("m2").uploaded());
        assert!(uploads.up("m1").to_trash);
    }

    #[test]
    fn the_drive_folder_follows_the_meetings_name() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        let drive = FakeDrive::default();
        let library = FakeLibrary::new();
        let (m, folder) = meeting(dir.path(), "m1", Origin::Recorded, b"audio");
        lock(&library.meetings).insert("m1".into(), (m.clone(), folder.clone()));
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        let renamed = dir.path().join("2026-11-27 1000 견적 회의");
        std::fs::rename(&folder, &renamed).unwrap();
        lock(&library.meetings).insert("m1".into(), (m, renamed));
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        assert_eq!(drive.in_folder("ROOT"), vec!["2026-11-27 1000 견적 회의"]);
        assert_eq!(drive.called("mkdir"), 1);
    }

    #[test]
    fn meetings_of_this_computer_wait_while_the_setting_is_off() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        let drive = FakeDrive::default();
        let mut library = FakeLibrary::new();
        library.settings.own = false;
        let (m, folder) = meeting(dir.path(), "m1", Origin::Recorded, b"audio");
        let (p, pfolder) = meeting(dir.path(), "p1", Origin::Phone, b"phone");
        lock(&library.meetings).insert("m1".into(), (m.clone(), folder.clone()));
        lock(&library.meetings).insert("p1".into(), (p.clone(), pfolder.clone()));
        uploads.want("m1");
        uploads.want("p1");
        uploads.run_once(&drive, &library).unwrap();
        assert!(uploads.up("m1").folder.is_empty());
        assert!(uploads.up("p1").uploaded());

        // The backlog counts what would go up.
        library.settings.own = true;
        let all = [(m, folder), (p, pfolder)];
        let (ids, backlog) = uploads.backlog_of(all.iter(), library.settings);
        // m1 is still wanted, so it is not counted twice.
        assert!(ids.is_empty());
        assert_eq!(backlog.meetings, 0);
    }

    #[test]
    fn the_backlog_counts_meetings_and_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        let (m, folder) = meeting(dir.path(), "m1", Origin::Recorded, b"0123456789");
        let mut working = meeting(dir.path(), "m2", Origin::Recorded, b"x");
        working.0.status = Status::Working;
        let all = [(m, folder), working];
        let settings = Settings {
            own: true,
            audio: true,
        };
        let (ids, backlog) = uploads.backlog_of(all.iter(), settings);
        assert_eq!(ids, vec!["m1".to_string()]);
        assert_eq!(backlog.meetings, 1);
        let text = "[00:01] 화자 1: 안녕하세요".len() as u64 * 2 + "# 회의".len() as u64;
        assert_eq!(backlog.bytes, text + 10);
        uploads.exclude("m1");
        assert!(uploads.backlog_of(all.iter(), settings).0.is_empty());
    }

    /// Truth 5: a task ticked on the phone is ticked here; one changed on
    /// both sides keeps the later change and the other stays, to be taken.
    #[test]
    fn changes_from_the_phone_come_here_and_nothing_is_lost() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        let drive = FakeDrive::default();
        let library = FakeLibrary::new();
        let (m, folder) = meeting(dir.path(), "m1", Origin::Recorded, b"audio");
        lock(&library.meetings).insert("m1".into(), (m, folder));
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        let drive_folder = uploads.up("m1").folder;

        // Ticked here at 100: noted, and the computer's file goes up.
        uploads.edited("m1", vec![("done:t1".into(), json!(true))], 100);
        assert!(uploads.up("m1").wanted);
        uploads.run_once(&drive, &library).unwrap();
        assert!(drive
            .in_folder(&drive_folder)
            .contains(&edits::COMPUTER_FILE.to_string()));

        // The phone took the tick back later, at 200.
        let phone = json!({"format": 1, "device": "phone", "meeting": "m1",
            "edits": [{"key": "done:t1", "value": false, "at_ms": 200}]});
        drive.put_file(
            "P1",
            edits::PHONE_FILE,
            &drive_folder,
            phone.to_string().as_bytes(),
        );
        uploads.run_once(&drive, &library).unwrap();
        let done = |library: &FakeLibrary| {
            lock(&library.meetings)["m1"].0.minutes[0].sections[0].entries[0].done
        };
        assert!(!done(&library));
        let conflicts = uploads.conflicts("m1");
        assert_eq!(conflicts.len(), 1);
        assert_eq!(
            (conflicts[0].other.as_str(), conflicts[0].other_side),
            ("true", Side::Computer)
        );
        assert_eq!(
            uploads.losing_edit("m1", "done:t1").unwrap().value,
            json!(true)
        );

        // The same file is not read again.
        let reads = drive.called("read");
        uploads.run_once(&drive, &library).unwrap();
        assert_eq!(drive.called("read"), reads);

        // Taken here again, later still: the computer's change stands.
        uploads.edited("m1", vec![("done:t1".into(), json!(true))], 300);
        assert_eq!(uploads.conflicts("m1")[0].other_side, Side::Phone);
    }

    /// A change made here before the meeting was ever in Drive waits
    /// quietly: nothing is asked of Drive.
    #[test]
    fn a_change_to_a_meeting_not_in_drive_asks_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        uploads.edited("m9", vec![], 1);
        assert!(!uploads.status().uploaded.contains(&"m9".to_string()));
        uploads.edited("m9", vec![("done:x".into(), json!(true))], 1);
        assert!(!uploads.up("m9").wanted);
    }

    /// Decision 5 of #67: a meeting's Drive folder put in Drive's trash (on
    /// the phone) takes the meeting out of Drive; it is not made again, and
    /// the meeting here stays as it is.
    #[test]
    fn a_folder_trashed_on_the_phone_is_not_put_back() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        let drive = FakeDrive::default();
        let library = FakeLibrary::new();
        let (m, folder) = meeting(dir.path(), "m1", Origin::Recorded, b"audio");
        lock(&library.meetings).insert("m1".into(), (m, folder.clone()));
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        let drive_folder = uploads.up("m1").folder;
        assert!(uploads.in_drive("m1"));

        lock(&drive.files).get_mut(&drive_folder).unwrap().trashed = true;
        uploads.run_once(&drive, &library).unwrap();
        assert!(uploads.is_excluded("m1"));
        assert!(!uploads.in_drive("m1"));
        // Changed here afterwards: still not put back.
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        assert_eq!(drive.called("mkdir"), 1);
        assert!(folder.join("녹음.wav").exists());
        // "드라이브에 다시 올리기" (or undoing the delete) takes the same
        // folder out of the trash.
        uploads.include("m1");
        uploads.run_once(&drive, &library).unwrap();
        assert_eq!(drive.called("mkdir"), 1);
        assert_eq!(uploads.up("m1").folder, drive_folder);
        assert!(!lock(&drive.files)[&drive_folder].trashed);
    }

    /// Asked to go up again after its folder was trashed by hand, before the
    /// turn that looks for trashed folders: it is taken out, not remade.
    #[test]
    fn a_trashed_folder_met_while_uploading_takes_the_meeting_out() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        let drive = FakeDrive::default();
        let library = FakeLibrary::new();
        let (m, folder) = meeting(dir.path(), "m1", Origin::Recorded, b"audio");
        lock(&library.meetings).insert("m1".into(), (m, folder));
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        let drive_folder = uploads.up("m1").folder;
        // Moved out of the root first, so the look for trashed folders misses it.
        {
            let mut files = lock(&drive.files);
            let f = files.get_mut(&drive_folder).unwrap();
            f.trashed = true;
            f.parents = vec!["ELSEWHERE".into()];
        }
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        assert!(uploads.is_excluded("m1"));
        assert_eq!(drive.called("mkdir"), 1);
    }

    /// Two phone files in one folder (two phones, or a reply lost while one
    /// was made) are read as one: no meeting flips back and forth.
    #[test]
    fn two_phone_files_are_read_as_one_and_settle() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        let drive = FakeDrive::default();
        let library = FakeLibrary::new();
        let (m, folder) = meeting(dir.path(), "m1", Origin::Recorded, b"audio");
        lock(&library.meetings).insert("m1".into(), (m, folder));
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        let drive_folder = uploads.up("m1").folder;
        let file = |value: bool, at: i64| {
            json!({"format": 1, "device": "phone", "meeting": "m1",
                "edits": [{"key": "done:t1", "value": value, "at_ms": at}]})
            .to_string()
        };
        drive.put_file(
            "P1",
            edits::PHONE_FILE,
            &drive_folder,
            file(true, 100).as_bytes(),
        );
        drive.put_file(
            "P2",
            edits::PHONE_FILE,
            &drive_folder,
            file(false, 200).as_bytes(),
        );
        uploads.run_once(&drive, &library).unwrap();
        let done = || lock(&library.meetings)["m1"].0.minutes[0].sections[0].entries[0].done;
        assert!(!done());
        // Nothing new: nothing read, nothing sent, the meeting stays put.
        let (reads, updates) = (drive.called("read"), drive.called("update"));
        uploads.run_once(&drive, &library).unwrap();
        uploads.run_once(&drive, &library).unwrap();
        assert_eq!(
            (drive.called("read"), drive.called("update")),
            (reads, updates)
        );
        assert!(!uploads.up("m1").wanted);
        assert!(!done());
    }

    /// Transcribed or written again: changes made before are for what was
    /// there, and are not made to the new one.
    #[test]
    fn changes_from_before_a_new_transcript_are_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        let drive = FakeDrive::default();
        let library = FakeLibrary::new();
        let (m, folder) = meeting(dir.path(), "m1", Origin::Recorded, b"audio");
        lock(&library.meetings).insert("m1".into(), (m, folder));
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        let drive_folder = uploads.up("m1").folder;
        uploads.edited("m1", vec![("done:t1".into(), json!(true))], 100);
        uploads.reset_edits("m1", 500);
        assert!(uploads.up("m1").edits.is_empty());
        let old = json!({"format": 1, "device": "phone", "meeting": "m1",
            "edits": [{"key": "done:t1", "value": true, "at_ms": 400}]});
        drive.put_file(
            "P1",
            edits::PHONE_FILE,
            &drive_folder,
            old.to_string().as_bytes(),
        );
        uploads.want("m1");
        uploads.run_once(&drive, &library).unwrap();
        assert!(!lock(&library.meetings)["m1"].0.minutes[0].sections[0].entries[0].done);
        let json: serde_json::Value =
            serde_json::from_slice(&lock(&drive.content)[&uploads.up("m1").files[MEETING_JSON].id])
                .unwrap();
        assert_eq!(json["edits_since"], 500);
    }

    /// A tick made while the meeting's first upload is under way goes up too.
    #[test]
    fn a_change_made_while_going_up_goes_up_again() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = uploads(&dir.path().join("data"));
        uploads.want("m1");
        let asks = uploads.up("m1").asks;
        uploads.edited("m1", vec![("done:t1".into(), json!(true))], 1);
        assert_eq!(uploads.up("m1").asks, asks + 1);
    }
}
