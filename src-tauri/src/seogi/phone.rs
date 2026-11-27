//! Recordings made on the phone: the phone's Seogi uploads each one to the
//! user's Google Drive (the shared `서기` folder) with a small JSON beside
//! it, and this brings them in as meetings, the same way a recording made
//! here is processed.
//!
//! A recording is taken only whole: its audio and its JSON are both in the
//! folder, and the downloaded audio has the SHA-256 the JSON (or, for a
//! JSON that has none, Drive) gives. Audio without a JSON, or a JSON
//! without its audio, waits for the next look. Nothing in Drive is changed
//! or deleted here; once the meeting's minutes are ready the recording and
//! its JSON move into the meeting's own Drive folder ([`super::upload`]).
//! What was brought in is noted in the ledger by the Drive file and its
//! SHA-256, so nothing comes in twice.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::google::{self, DriveFile, Google, Standing};
use super::watch;

/// How often the folder is looked at while the app runs.
pub const LOOK_EVERY: Duration = Duration::from_secs(5 * 60);
/// A sidecar is a few hundred bytes; anything much larger is not one.
const SIDECAR_LIMIT: usize = 256 * 1024;
/// A recording that could not be brought in (its download did not match,
/// Drive refused that one file, it could not be kept here) waits this long
/// before it is tried again.
const HOLD_FOR: Duration = Duration::from_secs(60 * 60);
const INCOMING: &str = "phone-incoming";

/// The JSON the phone uploads beside each recording.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Sidecar {
    pub app: String,
    /// The audio file's name, in the same folder.
    pub file: String,
    /// When recording started, with the phone's offset
    /// (`2026-11-26T18:42:52.309+09:00`).
    pub started_at: String,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub marks_ms: Vec<u64>,
    #[serde(default)]
    pub title: String,
    /// The calendar event's title, when the phone found one.
    #[serde(default)]
    pub event_title: Option<String>,
    #[serde(default)]
    pub device: String,
    /// Empty for a recording the phone had to bring back after a crash.
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub recovered: bool,
}

impl Sidecar {
    /// Reads a sidecar; `None` for anything that is not the phone's.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let sidecar: Sidecar = serde_json::from_slice(bytes).ok()?;
        (sidecar.app == "seogi-android"
            && safe_file_name(&sidecar.file)
            && sidecar.started_at_ms().is_some())
        .then_some(sidecar)
    }

    pub fn started_at_ms(&self) -> Option<i64> {
        chrono::DateTime::parse_from_rfc3339(&self.started_at)
            .ok()
            .map(|at| at.timestamp_millis())
    }
}

/// A name that can only be a file in one folder, and a recording.
fn safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['/', '\\', ':', '\0'])
        && name != "."
        && name != ".."
        && watch::is_recording(Path::new(name))
}

/// The name the recording keeps on this computer: the phone's, with the
/// few characters Windows does not take in a file name replaced.
fn local_name(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let stem_end = replaced.rfind('.').unwrap_or(replaced.len());
    let (stem, ending) = replaced.split_at(stem_end);
    format!("{}{ending}", stem.trim_end_matches([' ', '.']))
}

fn is_sidecar(file: &DriveFile) -> bool {
    file.name.to_lowercase().ends_with(".json")
}

/// A recording ready to be brought in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ready {
    pub audio: DriveFile,
    /// The Drive id of the JSON beside it.
    pub json_id: String,
    pub sidecar: Sidecar,
    /// The SHA-256 the download must have.
    pub sha256: String,
}

impl Ready {
    /// What the ledger knows it by.
    pub fn key(&self) -> String {
        key(&self.audio.id, &self.sha256)
    }
}

pub fn key(audio_id: &str, sha256: &str) -> String {
    format!("{audio_id}|{}", sha256.to_lowercase())
}

/// What one look at the folder found.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Plan {
    pub ready: Vec<Ready>,
    /// Recordings that are not whole yet: audio with no JSON, a JSON with no
    /// audio, or audio that does not match its JSON.
    pub waiting: usize,
}

/// Pairs each phone JSON with its audio. `sidecars` holds the JSONs read so
/// far, by their Drive file id; `done` says whether a recording (by
/// [`key`]) was brought in already.
pub fn plan(
    files: &[DriveFile],
    sidecars: &HashMap<String, Sidecar>,
    done: impl Fn(&str) -> bool,
) -> Plan {
    let mut plan = Plan::default();
    let mut named: HashSet<&str> = HashSet::new();
    for json in files.iter().filter(|file| is_sidecar(file)) {
        let Some(sidecar) = sidecars.get(&json.id) else {
            continue;
        };
        named.insert(sidecar.file.as_str());
        let wanted = sidecar.sha256.to_lowercase();
        let candidates: Vec<&DriveFile> = files
            .iter()
            .filter(|file| !is_sidecar(file) && file.name == sidecar.file)
            .collect();
        let checksum = |file: &DriveFile| file.sha256_checksum.as_deref().map(str::to_lowercase);
        // The audio this JSON speaks of: the one whose content Drive says
        // matches, or one Drive has no checksum for yet (checked after the
        // download). Another one with the same name is another recording.
        let found = if wanted.is_empty() {
            candidates
                .iter()
                .find(|file| checksum(file).is_some())
                .map(|file| (*file, checksum(file).unwrap_or_default()))
        } else {
            candidates
                .iter()
                .find(|file| checksum(file).as_deref() == Some(wanted.as_str()))
                .or_else(|| candidates.iter().find(|file| checksum(file).is_none()))
                .map(|file| (*file, wanted.clone()))
        };
        let Some((audio, sha256)) = found else {
            plan.waiting += 1;
            continue;
        };
        if audio.size == Some(0) {
            plan.waiting += 1;
            continue;
        }
        if done(&key(&audio.id, &sha256)) {
            continue;
        }
        if plan.ready.iter().any(|ready| ready.audio.id == audio.id) {
            continue;
        }
        plan.ready.push(Ready {
            audio: audio.clone(),
            json_id: json.id.clone(),
            sidecar: sidecar.clone(),
            sha256,
        });
    }
    plan.waiting += files
        .iter()
        .filter(|file| !is_sidecar(file) && !named.contains(file.name.as_str()))
        .count();
    plan
}

/// What a recording from the phone brings with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrival {
    pub file_name: String,
    pub started_at_ms: i64,
    pub duration_ms: u64,
    pub marks_ms: Vec<u64>,
    /// The phone's calendar title, or the phone's own name for it.
    pub title: String,
    /// Whether nobody chose the title (no calendar event on the phone).
    pub title_is_auto: bool,
    pub device: String,
    /// Where it came from, for the record.
    pub source: String,
    /// The phone had to bring it back after it stopped unexpectedly.
    pub recovered: bool,
}

impl Arrival {
    pub fn of(ready: &Ready) -> Result<Self> {
        let sidecar = &ready.sidecar;
        let started_at_ms = sidecar
            .started_at_ms()
            .ok_or_else(|| anyhow!("the phone's start time could not be read"))?;
        let event = sidecar
            .event_title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty());
        let duration_ms = sidecar.duration_ms;
        Ok(Self {
            file_name: local_name(&ready.audio.name),
            started_at_ms,
            duration_ms,
            // A mark past the end is the phone's clock, not a moment.
            marks_ms: sidecar
                .marks_ms
                .iter()
                .copied()
                .filter(|&mark| duration_ms == 0 || mark <= duration_ms)
                .collect(),
            title: event.unwrap_or(sidecar.title.trim()).to_string(),
            title_is_auto: event.is_none(),
            device: sidecar.device.clone(),
            source: format!("Google Drive/{}/{}", google::FOLDER_NAME, ready.audio.name),
            recovered: sidecar.recovered,
        })
    }
}

/// What the pulling needs from the rest of the app.
pub trait Host {
    /// Whether a recording (by [`key`]) was already brought in.
    fn done(&self, key: &str) -> bool;
    /// Whether the library has a recording with this SHA-256, or the user
    /// moved its meeting to the trash: then it is not downloaded at all.
    fn known(&self, sha256: &str) -> bool;
    /// Brings a downloaded, checked recording in as a meeting; returns its
    /// id (empty when it was already there).
    fn bring_in(&self, file: &Path, arrival: &Arrival) -> Result<String>;
    /// Notes a recording as brought in.
    fn note(&self, key: &str, meeting_id: &str);
    /// Notes the Drive files (the recording and its JSON) a meeting was
    /// brought in from, so they can move into its Drive folder later.
    fn brought_from(&self, meeting_id: &str, drive_files: &[String]);
    /// Tells the user, once, that Google has to be connected again.
    fn lost_sign_in(&self);
}

/// Where pulling from the phone stands, for the screen.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct PhoneStatus {
    /// This build can connect to Google.
    pub available: bool,
    /// The connected account's address; empty when not connected (or not
    /// known yet).
    pub account: String,
    pub connected: bool,
    /// Signed in once, and Google no longer takes it.
    pub needs_reconnect: bool,
    /// Waiting for the browser.
    pub connecting: bool,
    /// Looking at the folder or downloading right now.
    pub pulling: bool,
    /// When the folder was last looked at successfully (ms since 1970), 0
    /// for never.
    pub last_look_ms: i64,
    /// How many recordings came in since the app started.
    pub brought_in: u32,
    /// Why the last look failed: `offline` (no connection), `drive` (Drive
    /// refused or failed), `local` (a recording could not be kept on this
    /// computer), or empty.
    pub trouble: String,
}

/// Pulling the phone's recordings from Drive.
pub struct Phone {
    google: Arc<Google>,
    data_dir: PathBuf,
    sidecars: Mutex<HashMap<String, (String, Option<Sidecar>)>>,
    /// Recordings waiting after a failure: since when, and whether the
    /// trouble was on this computer.
    held: Mutex<HashMap<String, (Instant, bool)>>,
    status: Mutex<PhoneStatus>,
    pulling: AtomicBool,
    wake: (Mutex<bool>, Condvar),
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl Phone {
    pub fn new(data_dir: &Path, google: Arc<Google>) -> Self {
        Self {
            google,
            data_dir: data_dir.to_path_buf(),
            sidecars: Mutex::new(HashMap::new()),
            held: Mutex::new(HashMap::new()),
            status: Mutex::new(PhoneStatus::default()),
            pulling: AtomicBool::new(false),
            wake: (Mutex::new(false), Condvar::new()),
        }
    }

    pub fn status(&self) -> PhoneStatus {
        let mut status = lock(&self.status).clone();
        status.available = google::available();
        status.connecting = self.google.is_connecting();
        status.pulling = self.pulling.load(Ordering::SeqCst);
        match self.google.standing() {
            Standing::NotConnected => {
                status.account.clear();
                status.connected = false;
                status.needs_reconnect = false;
            }
            Standing::Connected { account } => {
                status.account = account;
                status.connected = true;
                status.needs_reconnect = false;
            }
            Standing::Lost { account } => {
                status.account = account;
                status.connected = false;
                status.needs_reconnect = true;
            }
        }
        status
    }

    /// Signs in in the browser and, once connected, makes sure the folder
    /// is there (made the way the phone makes it when no phone has yet), and
    /// looks at it.
    pub fn connect(&self, open: impl FnOnce(&str) -> Result<()>) -> Result<String> {
        let account = self.google.connect(open)?;
        lock(&self.status).trouble.clear();
        if let Err(e) = self.google.call(google::find_or_make_folder) {
            warn!("The phone's folder could not be found or made yet: {e:#}");
        }
        self.look_now();
        Ok(account)
    }

    pub fn cancel_connect(&self) {
        self.google.cancel_connect();
    }

    pub fn disconnect(&self) -> Result<()> {
        self.google.disconnect()?;
        let mut status = lock(&self.status);
        status.trouble.clear();
        status.last_look_ms = 0;
        Ok(())
    }

    /// Has the folder looked at now instead of at the next turn. A
    /// recording held for trouble on this computer is tried again too.
    pub fn look_now(&self) {
        lock(&self.held).retain(|_, (_, local)| !*local);
        let (flag, signal) = &self.wake;
        *lock(flag) = true;
        signal.notify_all();
    }

    /// Waits until the next look is due or asked for.
    pub fn wait(&self, every: Duration) {
        let (flag, signal) = &self.wake;
        let mut asked = lock(flag);
        if !*asked {
            asked = signal
                .wait_timeout(asked, every)
                .map(|(guard, _)| guard)
                .unwrap_or_else(|e| e.into_inner().0);
        }
        *asked = false;
    }

    /// One look at the folder: brings in every whole recording not brought
    /// in before. Does nothing while not connected.
    pub fn pull(&self, host: &dyn Host) {
        if !matches!(self.google.standing(), Standing::Connected { .. }) {
            return;
        }
        if self.pulling.swap(true, Ordering::SeqCst) {
            return;
        }
        let result = self.pull_once(&Live(&self.google), host);
        self.pulling.store(false, Ordering::SeqCst);
        let mut status = lock(&self.status);
        match result {
            Ok(look) => {
                status.trouble = if look.local_trouble {
                    "local".to_string()
                } else {
                    String::new()
                };
                status.last_look_ms = chrono::Utc::now().timestamp_millis();
                status.brought_in += look.brought_in;
            }
            Err(e) if e.downcast_ref::<google::NeedsReconnect>().is_some() => {
                drop(status);
                // Not when the user disconnected in the middle of the look.
                if matches!(self.google.standing(), Standing::Lost { .. }) {
                    warn!("Google needs to be connected again for the phone's recordings");
                    host.lost_sign_in();
                }
            }
            Err(e) => {
                warn!("Could not look at the phone's recordings: {e:#}");
                status.trouble = if e.downcast_ref::<google::Offline>().is_some() {
                    "offline"
                } else if e.downcast_ref::<google::HttpStatus>().is_some() {
                    "drive"
                } else {
                    "local"
                }
                .to_string();
            }
        }
    }

    /// One look with `drive`. Fails only for what stops the whole look (no
    /// connection, Drive itself in trouble, the sign-in); a recording that
    /// fails for a reason of its own waits an hour and the others go on.
    fn pull_once(&self, drive: &dyn Drive, host: &dyn Host) -> Result<Look> {
        let folder = drive.folder()?;
        let files = drive.list(&folder)?;
        let sidecars = self.read_sidecars(drive, &files)?;
        let plan = plan(&files, &sidecars, |key| host.done(key));
        if plan.waiting > 0 {
            info!(
                "{} of the phone's recordings are not whole in Drive yet",
                plan.waiting
            );
        }
        let mut look = Look::default();
        for ready in plan.ready {
            if !drive.connected() {
                // Disconnected meanwhile: nothing more comes in.
                break;
            }
            let key = ready.key();
            if let Some(&(at, local)) = lock(&self.held).get(&key) {
                if at.elapsed() < HOLD_FOR {
                    // Still said on the screen while it waits.
                    look.local_trouble |= local;
                    continue;
                }
            }
            if host.known(&ready.sha256) {
                // Already in the library (brought in by hand), or its
                // meeting is in the trash: not downloaded again.
                host.note(&key, "");
                continue;
            }
            match self.bring_in(drive, host, &ready) {
                Ok(id) => {
                    host.note(&key, &id);
                    if !id.is_empty() {
                        host.brought_from(&id, &[ready.audio.id.clone(), ready.json_id.clone()]);
                        look.brought_in += 1;
                        info!("Brought in a recording from the phone as {id}");
                    }
                }
                Err(e) if stops_the_look(&e) => return Err(e),
                Err(e) => {
                    let local = e.downcast_ref::<Mismatch>().is_none()
                        && e.downcast_ref::<google::HttpStatus>().is_none();
                    if local {
                        warn!("A recording from the phone could not be brought in; trying later: {e:#}");
                    } else {
                        warn!("A recording from the phone could not be taken from Drive; trying later: {e:#}");
                    }
                    look.local_trouble |= local;
                    lock(&self.held).insert(key, (Instant::now(), local));
                }
            }
        }
        Ok(look)
    }

    /// Reads each JSON once (again only when Drive says it changed). One
    /// that cannot be read counts as not the phone's until it changes.
    fn read_sidecars(
        &self,
        drive: &dyn Drive,
        files: &[DriveFile],
    ) -> Result<HashMap<String, Sidecar>> {
        let mut cache = lock(&self.sidecars);
        let present: HashSet<&str> = files.iter().map(|file| file.id.as_str()).collect();
        cache.retain(|id, _| present.contains(id.as_str()));
        for json in files.iter().filter(|file| is_sidecar(file)) {
            let fresh = cache
                .get(&json.id)
                .is_some_and(|(modified, _)| *modified == json.modified_time);
            if fresh {
                continue;
            }
            let sidecar = if json.size.is_some_and(|size| size as usize > SIDECAR_LIMIT) {
                None
            } else {
                match drive.read_small(&json.id, SIDECAR_LIMIT) {
                    Ok(bytes) => Sidecar::parse(&bytes),
                    Err(e) if stops_the_look(&e) => return Err(e),
                    Err(e) => {
                        warn!("A JSON in the phone's folder could not be read: {e:#}");
                        None
                    }
                }
            };
            cache.insert(json.id.clone(), (json.modified_time.clone(), sidecar));
        }
        Ok(cache
            .iter()
            .filter_map(|(id, (_, sidecar))| sidecar.clone().map(|s| (id.clone(), s)))
            .collect())
    }

    /// Downloads one recording, checks it, and hands it over.
    fn bring_in(&self, drive: &dyn Drive, host: &dyn Host, ready: &Ready) -> Result<String> {
        let arrival = Arrival::of(ready)?;
        let dir = self.data_dir.join(INCOMING);
        // Its own folder, so the file keeps its name for the meeting.
        let place = dir.join(&ready.audio.id);
        let _ = std::fs::remove_dir_all(&place);
        std::fs::create_dir_all(&place)
            .with_context(|| format!("could not create {}", place.display()))?;
        let file = place.join(&arrival.file_name);
        let result = (|| {
            drive.download(&ready.audio.id, &file)?;
            if !drive.connected() {
                // Disconnected while it was downloading: not brought in.
                return Err(google::NeedsReconnect.into());
            }
            let sha256 = super::evidence::sha256_file(&file)?;
            if !sha256.eq_ignore_ascii_case(&ready.sha256) {
                bail!(Mismatch);
            }
            host.bring_in(&file, &arrival)
        })();
        // The meeting has its own copy; the download is not kept.
        let _ = std::fs::remove_dir_all(&place);
        result
    }
}

/// What one look did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Look {
    brought_in: u32,
    /// A recording could not be kept on this computer (the disk, the
    /// library folder).
    local_trouble: bool,
}

/// Errors after which the rest of the look would fail the same way: no
/// connection, the sign-in, or Drive itself busy or failing.
fn stops_the_look(error: &anyhow::Error) -> bool {
    error.downcast_ref::<google::Offline>().is_some()
        || error.downcast_ref::<google::NeedsReconnect>().is_some()
        || matches!(
            error.downcast_ref::<google::HttpStatus>(),
            Some(google::HttpStatus(429 | 500..=599))
        )
}

/// The Drive calls a look makes.
trait Drive {
    /// Still signed in (the user may disconnect during a look).
    fn connected(&self) -> bool;
    fn folder(&self) -> Result<String>;
    fn list(&self, folder: &str) -> Result<Vec<DriveFile>>;
    fn read_small(&self, id: &str, limit: usize) -> Result<Vec<u8>>;
    fn download(&self, id: &str, to: &Path) -> Result<u64>;
}

/// [`Drive`] through the user's Google sign-in.
struct Live<'a>(&'a Google);

impl Drive for Live<'_> {
    fn connected(&self) -> bool {
        matches!(self.0.standing(), Standing::Connected { .. })
    }

    fn folder(&self) -> Result<String> {
        self.0.call(google::find_or_make_folder)
    }

    fn list(&self, folder: &str) -> Result<Vec<DriveFile>> {
        self.0.call(|token| google::list_folder(token, folder))
    }

    fn read_small(&self, id: &str, limit: usize) -> Result<Vec<u8>> {
        self.0.call(|token| google::read_small(token, id, limit))
    }

    fn download(&self, id: &str, to: &Path) -> Result<u64> {
        self.0.call(|token| google::download(token, id, to))
    }
}

/// The downloaded audio is not what its JSON says.
#[derive(Debug)]
struct Mismatch;

impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the download does not match its JSON")
    }
}

impl std::error::Error for Mismatch {}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: &str, name: &str, sha: Option<&str>) -> DriveFile {
        DriveFile {
            id: id.into(),
            name: name.into(),
            mime_type: if name.ends_with(".json") {
                "application/json".into()
            } else {
                "audio/mp4".into()
            },
            size: Some(100),
            sha256_checksum: sha.map(str::to_string),
            modified_time: "2026-11-26T09:43:28.108Z".into(),
            created_time: "2026-11-26T09:43:28.108Z".into(),
        }
    }

    fn sidecar(file: &str, sha: &str) -> Sidecar {
        Sidecar::parse(
            serde_json::json!({
                "app": "seogi-android",
                "app_version": "0.1.0",
                "file": file,
                "started_at": "2026-11-26T18:42:52.309+09:00",
                "duration_ms": 61_000,
                "marks_ms": [5_000, 70_000],
                "title": "11월 26일 18:42 녹음",
                "event_title": "주간 회의",
                "device": "Phone X",
                "sha256": sha,
                "recovered": false,
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap()
    }

    const NAME: &str = "2026-11-26 1842 주간 회의.m4a";
    const JSON: &str = "2026-11-26 1842 주간 회의.json";

    #[test]
    fn a_whole_pair_is_ready_once() {
        let files = vec![file("J", JSON, Some("x")), file("A", NAME, Some("AB12"))];
        let sidecars = HashMap::from([("J".to_string(), sidecar(NAME, "ab12"))]);
        let plan = plan(&files, &sidecars, |_| false);
        assert_eq!(plan.waiting, 0);
        assert_eq!(plan.ready.len(), 1);
        assert_eq!(plan.ready[0].audio.id, "A");
        assert_eq!(plan.ready[0].sha256, "ab12");
        assert_eq!(plan.ready[0].key(), "A|ab12");
        // Brought in already: nothing to do.
        let again = super::plan(&files, &sidecars, |key| key == "A|ab12");
        assert!(again.ready.is_empty());
        assert_eq!(again.waiting, 0);
    }

    #[test]
    fn half_a_recording_waits() {
        // A JSON whose audio is not there (or went to the trash).
        let only_json = vec![file("J", JSON, Some("x"))];
        let sidecars = HashMap::from([("J".to_string(), sidecar(NAME, "ab12"))]);
        let plan1 = plan(&only_json, &sidecars, |_| false);
        assert!(plan1.ready.is_empty());
        assert_eq!(plan1.waiting, 1);
        // Audio whose JSON is not there yet, or not read yet.
        let only_audio = vec![file("A", NAME, Some("ab12"))];
        let plan2 = plan(&only_audio, &HashMap::new(), |_| false);
        assert!(plan2.ready.is_empty());
        assert_eq!(plan2.waiting, 1);
        // Audio that is not what the JSON says (another upload of the name).
        let other = vec![file("J", JSON, None), file("A", NAME, Some("ffff"))];
        let plan3 = plan(&other, &sidecars, |_| false);
        assert!(plan3.ready.is_empty());
        assert_eq!(plan3.waiting, 1);
        // An empty file is not a finished upload.
        let mut empty = file("A", NAME, None);
        empty.size = Some(0);
        let plan4 = plan(&[file("J", JSON, None), empty], &sidecars, |_| false);
        assert!(plan4.ready.is_empty());
    }

    #[test]
    fn of_two_with_the_same_name_the_matching_one_is_taken() {
        let files = vec![
            file("A1", NAME, Some("ffff")),
            file("J", JSON, None),
            file("A2", NAME, Some("ab12")),
        ];
        let sidecars = HashMap::from([("J".to_string(), sidecar(NAME, "ab12"))]);
        let plan = plan(&files, &sidecars, |_| false);
        assert_eq!(plan.ready.len(), 1);
        assert_eq!(plan.ready[0].audio.id, "A2");
        // Two JSONs for one recording (a phone's retry) bring it in once.
        let doubled = vec![
            file("J1", JSON, None),
            file("J2", JSON, None),
            file("A", NAME, Some("ab12")),
        ];
        let sidecars = HashMap::from([
            ("J1".to_string(), sidecar(NAME, "ab12")),
            ("J2".to_string(), sidecar(NAME, "ab12")),
        ]);
        assert_eq!(super::plan(&doubled, &sidecars, |_| false).ready.len(), 1);
    }

    #[test]
    fn audio_drive_has_not_summed_yet_is_checked_after_the_download() {
        let files = vec![file("J", JSON, None), file("A", NAME, None)];
        let sidecars = HashMap::from([("J".to_string(), sidecar(NAME, "ab12"))]);
        let plan = plan(&files, &sidecars, |_| false);
        assert_eq!(plan.ready.len(), 1);
        assert_eq!(plan.ready[0].sha256, "ab12");
    }

    #[test]
    fn a_recording_brought_back_after_a_crash_is_checked_against_drive() {
        // The phone could not sum it: Drive's checksum is what it must match.
        let files = vec![file("J", JSON, None), file("A", NAME, Some("CD34"))];
        let sidecars = HashMap::from([("J".to_string(), sidecar(NAME, ""))]);
        let plan = plan(&files, &sidecars, |_| false);
        assert_eq!(plan.ready[0].sha256, "cd34");
        // With no sum anywhere it cannot be checked, so it waits.
        let unsummed = vec![file("J", JSON, None), file("A", NAME, None)];
        let plan = super::plan(&unsummed, &sidecars, |_| false);
        assert!(plan.ready.is_empty());
        assert_eq!(plan.waiting, 1);
    }

    #[test]
    fn only_the_phones_json_naming_a_plain_recording_counts() {
        assert!(Sidecar::parse(b"not json").is_none());
        assert!(Sidecar::parse(br#"{"app":"other","file":"a.m4a","started_at":""}"#).is_none());
        for bad in [
            "../a.m4a", "a/b.m4a", r"a\b.m4a", "C:a.m4a", "a.json", "", "..",
        ] {
            let text = serde_json::json!({"app": "seogi-android", "file": bad, "started_at": ""});
            assert!(
                Sidecar::parse(text.to_string().as_bytes()).is_none(),
                "{bad} should be refused"
            );
        }
        // A missing event title (null) is fine.
        let text = br#"{"app":"seogi-android","file":"a.m4a","started_at":"2026-11-26T18:42:52+09:00","event_title":null}"#;
        assert!(Sidecar::parse(text).unwrap().event_title.is_none());
    }

    #[test]
    fn the_meeting_takes_the_phones_title_time_and_marks() {
        let ready = Ready {
            audio: file("A", NAME, Some("ab12")),
            json_id: "J".into(),
            sidecar: sidecar(NAME, "ab12"),
            sha256: "ab12".into(),
        };
        let arrival = Arrival::of(&ready).unwrap();
        assert_eq!(arrival.file_name, NAME);
        assert_eq!(arrival.title, "주간 회의");
        assert!(!arrival.title_is_auto);
        // 18:42:52.309 in Seoul.
        assert_eq!(arrival.started_at_ms, 1_795_686_172_309);
        assert_eq!(arrival.duration_ms, 61_000);
        // The mark past the end is dropped.
        assert_eq!(arrival.marks_ms, vec![5_000]);
        assert_eq!(arrival.device, "Phone X");
        assert!(arrival.source.ends_with(NAME));

        // No calendar event: the phone's own name, which the minutes may replace.
        let mut no_event = ready.clone();
        no_event.sidecar.event_title = None;
        let arrival = Arrival::of(&no_event).unwrap();
        assert_eq!(arrival.title, "11월 26일 18:42 녹음");
        assert!(arrival.title_is_auto);

        // A name Windows would refuse keeps its recording, not its characters.
        let mut odd = ready.clone();
        odd.audio.name = "2026-11-26 1842 왜? \"급\" 회의 .m4a".into();
        assert_eq!(
            Arrival::of(&odd).unwrap().file_name,
            "2026-11-26 1842 왜_ _급_ 회의.m4a"
        );

        let mut bad_time = ready;
        bad_time.sidecar.started_at = "yesterday".into();
        assert!(Arrival::of(&bad_time).is_err());
    }

    // ----- a whole look, with a Drive and a library in memory -------------

    #[derive(Default)]
    struct FakeDrive {
        files: Vec<DriveFile>,
        content: HashMap<String, Vec<u8>>,
        /// Downloads of these fail with this status.
        refuse: HashMap<String, u16>,
        offline: bool,
        downloads: Mutex<Vec<String>>,
        /// The user disconnects while a download runs.
        disconnect_while_downloading: bool,
        gone: AtomicBool,
    }

    impl Drive for FakeDrive {
        fn connected(&self) -> bool {
            !self.gone.load(Ordering::SeqCst)
        }
        fn folder(&self) -> Result<String> {
            Ok("F".into())
        }
        fn list(&self, _folder: &str) -> Result<Vec<DriveFile>> {
            Ok(self.files.clone())
        }
        fn read_small(&self, id: &str, _limit: usize) -> Result<Vec<u8>> {
            Ok(self.content[id].clone())
        }
        fn download(&self, id: &str, to: &Path) -> Result<u64> {
            lock(&self.downloads).push(id.to_string());
            if self.offline {
                return Err(google::Offline("no network".into()).into());
            }
            if let Some(&code) = self.refuse.get(id) {
                return Err(google::HttpStatus(code).into());
            }
            std::fs::write(to, &self.content[id])?;
            if self.disconnect_while_downloading {
                self.gone.store(true, Ordering::SeqCst);
            }
            Ok(self.content[id].len() as u64)
        }
    }

    #[derive(Default)]
    struct FakeLibrary {
        noted: Mutex<HashMap<String, String>>,
        brought: Mutex<Vec<(String, Vec<u8>, Arrival)>>,
        known: HashSet<String>,
        /// Bringing in fails (a full disk).
        full: bool,
        from: Mutex<HashMap<String, Vec<String>>>,
    }

    impl Host for FakeLibrary {
        fn done(&self, key: &str) -> bool {
            lock(&self.noted).contains_key(key)
        }
        fn known(&self, sha256: &str) -> bool {
            self.known.contains(sha256)
        }
        fn bring_in(&self, file: &Path, arrival: &Arrival) -> Result<String> {
            if self.full {
                bail!("no room on the disk");
            }
            let mut brought = lock(&self.brought);
            brought.push((
                file.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(file)?,
                arrival.clone(),
            ));
            Ok(format!("m{}", brought.len()))
        }
        fn note(&self, key: &str, meeting_id: &str) {
            lock(&self.noted).insert(key.to_string(), meeting_id.to_string());
        }
        fn brought_from(&self, meeting_id: &str, drive_files: &[String]) {
            lock(&self.from).insert(meeting_id.to_string(), drive_files.to_vec());
        }
        fn lost_sign_in(&self) {}
    }

    fn sha_of(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// Puts one recording (audio and JSON) in the fake Drive. `bytes` are
    /// what Drive holds; the JSON says `said`.
    fn put(drive: &mut FakeDrive, n: u32, bytes: &[u8], said: &[u8]) {
        let name = format!("2026-11-26 184{n} 회의 {n}.m4a");
        let json = format!("2026-11-26 184{n} 회의 {n}.json");
        drive.files.push(file(&format!("J{n}"), &json, None));
        drive.files.push(file(&format!("A{n}"), &name, None));
        let side = serde_json::json!({
            "app": "seogi-android", "file": name,
            "started_at": "2026-11-26T18:42:52+09:00", "duration_ms": 1000,
            "title": format!("회의 {n}"), "event_title": format!("회의 {n}"),
            "sha256": sha_of(said),
        });
        drive
            .content
            .insert(format!("J{n}"), side.to_string().into_bytes());
        drive.content.insert(format!("A{n}"), bytes.to_vec());
    }

    #[test]
    fn a_look_brings_a_recording_in_once_with_its_checked_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let phone = Phone::new(dir.path(), Arc::new(Google::new(dir.path())));
        let mut drive = FakeDrive::default();
        put(&mut drive, 1, b"audio one", b"audio one");
        let library = FakeLibrary::default();

        let look = phone.pull_once(&drive, &library).unwrap();
        assert_eq!(look.brought_in, 1);
        let brought = lock(&library.brought);
        assert_eq!(brought[0].0, "2026-11-26 1841 회의 1.m4a");
        assert_eq!(brought[0].1, b"audio one");
        assert_eq!(brought[0].2.title, "회의 1");
        drop(brought);
        let key = key("A1", &sha_of(b"audio one"));
        assert_eq!(
            lock(&library.noted).get(&key).map(String::as_str),
            Some("m1")
        );
        // The recording and its JSON, to move into the meeting's folder.
        assert_eq!(
            lock(&library.from)["m1"],
            vec!["A1".to_string(), "J1".to_string()]
        );
        // The download is not left behind.
        assert!(std::fs::read_dir(dir.path().join(INCOMING))
            .map(|entries| entries.count() == 0)
            .unwrap_or(true));

        // The next look brings nothing and downloads nothing.
        let again = phone.pull_once(&drive, &library).unwrap();
        assert_eq!(again.brought_in, 0);
        assert_eq!(lock(&drive.downloads).len(), 1);
    }

    #[test]
    fn bytes_that_are_not_what_the_json_says_never_come_in() {
        let dir = tempfile::tempdir().unwrap();
        let phone = Phone::new(dir.path(), Arc::new(Google::new(dir.path())));
        let mut drive = FakeDrive::default();
        put(&mut drive, 1, b"half an upl", b"half an upload");
        let library = FakeLibrary::default();
        let look = phone.pull_once(&drive, &library).unwrap();
        assert_eq!(look.brought_in, 0);
        assert!(lock(&library.brought).is_empty());
        assert!(lock(&library.noted).is_empty());
        // It waits: the next look does not download it again at once.
        phone.pull_once(&drive, &library).unwrap();
        assert_eq!(lock(&drive.downloads).len(), 1);
    }

    #[test]
    fn one_recording_in_trouble_does_not_hold_up_the_others() {
        let dir = tempfile::tempdir().unwrap();
        let phone = Phone::new(dir.path(), Arc::new(Google::new(dir.path())));
        let mut drive = FakeDrive::default();
        put(&mut drive, 1, b"gone", b"gone");
        put(&mut drive, 2, b"fine", b"fine");
        drive.refuse.insert("A1".into(), 404);
        // A JSON with a start time that cannot be read is not the phone's.
        drive
            .files
            .push(file("J3", "2026-11-26 1843 회의 3.json", None));
        drive
            .files
            .push(file("A3", "2026-11-26 1843 회의 3.m4a", None));
        drive.content.insert(
            "J3".into(),
            r#"{"app":"seogi-android","file":"2026-11-26 1843 회의 3.m4a","started_at":"soon"}"#
                .as_bytes()
                .to_vec(),
        );
        let library = FakeLibrary::default();
        let look = phone.pull_once(&drive, &library).unwrap();
        assert_eq!(look.brought_in, 1);
        assert!(!look.local_trouble);
        assert_eq!(lock(&library.brought)[0].1, b"fine");
    }

    #[test]
    fn no_connection_stops_the_look_and_nothing_is_noted() {
        let dir = tempfile::tempdir().unwrap();
        let phone = Phone::new(dir.path(), Arc::new(Google::new(dir.path())));
        let mut drive = FakeDrive::default();
        put(&mut drive, 1, b"one", b"one");
        put(&mut drive, 2, b"two", b"two");
        drive.offline = true;
        let library = FakeLibrary::default();
        let error = phone.pull_once(&drive, &library).unwrap_err();
        assert!(error.downcast_ref::<google::Offline>().is_some());
        assert_eq!(lock(&drive.downloads).len(), 1);
        assert!(lock(&library.noted).is_empty());
        // Back online, both come in.
        drive.offline = false;
        assert_eq!(phone.pull_once(&drive, &library).unwrap().brought_in, 2);
    }

    #[test]
    fn a_recording_already_here_is_noted_without_a_download() {
        let dir = tempfile::tempdir().unwrap();
        let phone = Phone::new(dir.path(), Arc::new(Google::new(dir.path())));
        let mut drive = FakeDrive::default();
        put(&mut drive, 1, b"one", b"one");
        let library = FakeLibrary {
            known: HashSet::from([sha_of(b"one")]),
            ..FakeLibrary::default()
        };
        assert_eq!(phone.pull_once(&drive, &library).unwrap().brought_in, 0);
        assert!(lock(&drive.downloads).is_empty());
        assert_eq!(
            lock(&library.noted)
                .get(&key("A1", &sha_of(b"one")))
                .map(String::as_str),
            Some("")
        );
    }

    #[test]
    fn a_recording_that_cannot_be_kept_here_waits_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let phone = Phone::new(dir.path(), Arc::new(Google::new(dir.path())));
        let mut drive = FakeDrive::default();
        put(&mut drive, 1, b"one", b"one");
        let library = FakeLibrary {
            full: true,
            ..FakeLibrary::default()
        };
        let look = phone.pull_once(&drive, &library).unwrap();
        assert!(look.local_trouble);
        assert!(lock(&library.noted).is_empty());
        // Not downloaded again at the next look, and still said.
        assert!(phone.pull_once(&drive, &library).unwrap().local_trouble);
        assert_eq!(lock(&drive.downloads).len(), 1);
        // "지금 가져오기" tries it again at once.
        phone.look_now();
        phone.pull_once(&drive, &library).unwrap();
        assert_eq!(lock(&drive.downloads).len(), 2);
    }

    #[test]
    fn a_download_finished_after_disconnecting_is_not_brought_in() {
        let dir = tempfile::tempdir().unwrap();
        let phone = Phone::new(dir.path(), Arc::new(Google::new(dir.path())));
        let mut drive = FakeDrive::default();
        put(&mut drive, 1, b"one", b"one");
        put(&mut drive, 2, b"two", b"two");
        drive.disconnect_while_downloading = true;
        let library = FakeLibrary::default();
        assert!(phone.pull_once(&drive, &library).is_err());
        assert!(lock(&library.brought).is_empty());
        assert!(lock(&library.noted).is_empty());
        assert_eq!(lock(&drive.downloads).len(), 1);
    }
}
