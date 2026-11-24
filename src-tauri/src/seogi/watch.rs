//! Watched folders: recordings that appear there are brought in on their own.
//!
//! A folder is looked at every few seconds rather than listened to, because
//! the folders that matter (a phone's recordings synced through a cloud
//! drive) do not reliably announce their changes. A file is taken only once
//! it has stopped changing, so one still being recorded or still arriving
//! from the cloud is left alone.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The kinds of file a recorder makes.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "m4a", "mp3", "wav", "aac", "3gp", "3ga", "amr", "ogg", "oga", "opus", "flac", "wma", "mp4",
    "m4b", "mov", "webm", "caf", "aiff", "aif",
];

const LEDGER_NAME: &str = "ledger.json";
/// A file must have gone this long without changing.
const QUIET_FOR_MS: i64 = 10_000;

pub fn is_recording(path: &Path) -> bool {
    let hidden = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_none_or(|name| name.starts_with('.') || name.starts_with('~'));
    let audio = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()));
    !hidden && audio
}

/// What has already been brought in, so nothing is processed twice: not when
/// a file is renamed or moved, and not when the app is restarted.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    /// The SHA-256 of every recording brought in, and its meeting.
    #[serde(default)]
    pub hashes: HashMap<String, String>,
    /// Files in watched folders that have been looked at, by path, size and
    /// modification time, with their SHA-256: a file that has not changed is
    /// not read again. A file that was already there when watching began is
    /// here with no hash: it is left alone.
    #[serde(default)]
    pub seen: HashMap<String, String>,
    /// Watched folders whose earlier recordings have been noted in `seen`,
    /// by folder and the moment watching began.
    #[serde(default)]
    pub noted: HashSet<String>,
    /// The SHA-256 of recordings whose meetings the user moved to the trash:
    /// the same recording turning up in a watched folder again is not
    /// brought back in.
    #[serde(default)]
    pub deleted: HashSet<String>,
}

/// What came of noting what a watched folder already holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Noted {
    /// Done before; nothing to do.
    Already,
    /// Done now: this many recordings will be left alone.
    Now(usize),
    /// The folder cannot be listed yet (a drive not connected); try again.
    NotYet,
}

impl Ledger {
    pub fn load(data_dir: &Path) -> Self {
        std::fs::read_to_string(data_dir.join(LEDGER_NAME))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(data_dir)
            .with_context(|| format!("could not create {}", data_dir.display()))?;
        let json = serde_json::to_string(self)?;
        super::store::write_atomically(&data_dir.join(LEDGER_NAME), json.as_bytes())
    }

    /// Notes the recordings in `folder` made before `since_ms`, when watching
    /// began, as seen, so they are left alone. It is done once, at the first
    /// look at the folder; from then on a file is new because it was not
    /// there, whatever date it carries. A recording copied in from a phone
    /// or another folder keeps the date it was made, which may be days
    /// before the folder was watched.
    pub fn note_what_is_there(&mut self, folder: &Path, since_ms: i64) -> Noted {
        let name = format!("{}|{since_ms}", folder.display());
        if self.noted.contains(&name) {
            return Noted::Already;
        }
        let Ok(entries) = std::fs::read_dir(folder) else {
            return Noted::NotYet;
        };
        let mut left = 0;
        for path in entries.filter_map(|entry| entry.ok().map(|entry| entry.path())) {
            if !path.is_file() || !is_recording(&path) {
                continue;
            }
            let Some(look) = Look::at(&path) else {
                continue;
            };
            if look.modified_ms < since_ms {
                self.seen.entry(Self::key(&path, look)).or_default();
                left += 1;
            }
        }
        self.noted.insert(name);
        Noted::Now(left)
    }

    /// What identifies a file as "this one, unchanged".
    pub fn key(path: &Path, look: Look) -> String {
        format!("{}|{}|{}", path.display(), look.size, look.modified_ms)
    }
}

/// What a file looked like at one look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub size: u64,
    pub modified_ms: i64,
}

impl Look {
    pub fn at(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        let modified = meta.modified().ok()?;
        let modified_ms = modified
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        Some(Self {
            size: meta.len(),
            modified_ms,
        })
    }
}

/// Remembers what files looked like at the last look, to tell the ones that
/// have settled from the ones still being written.
#[derive(Debug, Default)]
pub struct Settling {
    last: HashMap<PathBuf, Look>,
}

impl Settling {
    /// The recordings directly in `folder` that have settled: not empty, the
    /// same size and time as at the last look, and untouched for a while.
    /// Oldest first. Which of them are new is the ledger's to say.
    pub fn settled(&mut self, folder: &Path, now_ms: i64) -> Vec<(PathBuf, Look)> {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return Vec::new();
        };
        let mut settled: Vec<(PathBuf, Look)> = Vec::new();
        for path in entries.filter_map(|entry| entry.ok().map(|entry| entry.path())) {
            if !path.is_file() || !is_recording(&path) {
                continue;
            }
            let Some(look) = Look::at(&path) else {
                continue;
            };
            let before = self.last.insert(path.clone(), look);
            if look.size > 0 && before == Some(look) && now_ms - look.modified_ms >= QUIET_FOR_MS {
                settled.push((path, look));
            }
        }
        settled.sort_by_key(|(_, look)| look.modified_ms);
        settled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_recordings_count_and_hidden_files_do_not() {
        assert!(is_recording(Path::new("/a/회의.M4A")));
        assert!(is_recording(Path::new("통화 녹음.3gp")));
        assert!(!is_recording(Path::new("/a/.회의.m4a")));
        assert!(!is_recording(Path::new("/a/~회의.m4a")));
        assert!(!is_recording(Path::new("/a/회의.m4a.part")));
        assert!(!is_recording(Path::new("/a/회의록.md")));
        assert!(!is_recording(Path::new("/a/회의")));
    }

    #[test]
    fn a_file_is_taken_only_after_it_stops_changing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("회의.m4a");
        std::fs::write(&file, b"12345").unwrap();
        std::fs::write(dir.path().join("메모.txt"), b"x").unwrap();
        std::fs::write(dir.path().join("빈 파일.m4a"), b"").unwrap();
        let written_ms = Look::at(&file).unwrap().modified_ms;
        let mut settling = Settling::default();

        // The first look only takes note of it.
        assert!(settling.settled(dir.path(), written_ms + 60_000).is_empty());
        // Unchanged at the second look, but written too recently.
        assert!(settling.settled(dir.path(), written_ms + 1_000).is_empty());
        // Unchanged and quiet for long enough: taken.
        let taken = settling.settled(dir.path(), written_ms + 60_000);
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].0, file);
        assert_eq!(taken[0].1.size, 5);

        // It grows (still syncing): not taken until it is quiet again.
        std::fs::write(&file, b"1234567890").unwrap();
        let grown_ms = Look::at(&file).unwrap().modified_ms;
        assert!(settling.settled(dir.path(), grown_ms + 60_000).is_empty());
        assert_eq!(settling.settled(dir.path(), grown_ms + 60_000).len(), 1);
    }

    /// Gives `path` a modification time of `ms` since 1970.
    fn dated(path: &Path, ms: i64) {
        let time = std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms as u64);
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(time).unwrap();
    }

    #[test]
    fn a_recording_copied_in_later_is_new_whatever_date_it_carries() {
        let dir = tempfile::tempdir().unwrap();
        let began_ms: i64 = 1_795_000_000_000;
        // In the folder before it was watched: left alone.
        let old = dir.path().join("지난달 회의.m4a");
        std::fs::write(&old, b"old").unwrap();
        dated(&old, began_ms - 86_400_000);
        // Not a recording: never counted.
        std::fs::write(dir.path().join("메모.txt"), b"x").unwrap();

        let mut ledger = Ledger::default();
        assert_eq!(
            ledger.note_what_is_there(dir.path(), began_ms),
            Noted::Now(1)
        );
        assert_eq!(
            ledger.note_what_is_there(dir.path(), began_ms),
            Noted::Already
        );
        let key = |path: &Path| Ledger::key(path, Look::at(path).unwrap());
        assert!(ledger.seen.contains_key(&key(&old)));

        // A recording made this morning, put in the folder this afternoon:
        // its date is from before watching began, and it is still new.
        let late = dir.path().join("오전 회의.m4a");
        std::fs::write(&late, b"late").unwrap();
        dated(&late, began_ms - 3 * 3_600_000);
        assert!(!ledger.seen.contains_key(&key(&late)));
        // And so is one made after.
        let fresh = dir.path().join("오후 회의.m4a");
        std::fs::write(&fresh, b"fresh").unwrap();
        dated(&fresh, began_ms + 60_000);
        assert!(!ledger.seen.contains_key(&key(&fresh)));

        // The old one saved again is a changed file: no longer left alone.
        std::fs::write(&old, b"old, edited").unwrap();
        assert!(!ledger.seen.contains_key(&key(&old)));

        // Watching the folder anew notes it anew.
        assert_eq!(
            ledger.note_what_is_there(dir.path(), began_ms + 7_200_000),
            Noted::Now(2)
        );
    }

    #[test]
    fn a_folder_that_cannot_be_listed_is_noted_when_it_can() {
        let dir = tempfile::tempdir().unwrap();
        let away = dir.path().join("연결 안 된 드라이브").join("녹음함");
        let mut ledger = Ledger::default();
        assert_eq!(ledger.note_what_is_there(&away, 1_000), Noted::NotYet);
        assert!(ledger.noted.is_empty());
        std::fs::create_dir_all(&away).unwrap();
        assert_eq!(ledger.note_what_is_there(&away, 1_000), Noted::Now(0));
        // A folder watched from the start of time leaves nothing alone.
        std::fs::write(away.join("회의.m4a"), b"x").unwrap();
        assert_eq!(ledger.note_what_is_there(&away, 0), Noted::Now(0));
    }

    #[test]
    fn the_ledger_survives_a_restart_and_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Ledger::load(dir.path()), Ledger::default());
        let mut ledger = Ledger::default();
        ledger
            .hashes
            .insert("abc".into(), "20261124-140000-0001".into());
        let look = Look {
            size: 5,
            modified_ms: 1_000,
        };
        ledger
            .seen
            .insert(Ledger::key(Path::new("/녹음함/a.m4a"), look), "abc".into());
        ledger.save(dir.path()).unwrap();
        assert_eq!(Ledger::load(dir.path()), ledger);
        // The same path with another size is another file.
        let grown = Look {
            size: 6,
            modified_ms: 1_000,
        };
        assert_ne!(
            Ledger::key(Path::new("/녹음함/a.m4a"), look),
            Ledger::key(Path::new("/녹음함/a.m4a"), grown)
        );
    }
}
