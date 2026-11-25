//! Seogi's settings, kept in their own file in the app's data folder.
//!
//! Paths in here (the vocabulary, watched folders, calendar addresses) are
//! the user's own and stay on their computer.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::dictation::DictationConfig;
use super::llm::Provider;

const FILE_NAME: &str = "seogi.json";
/// The name of the folder new users are offered for phone recordings.
pub const INBOX_NAME: &str = "녹음함";
/// The speech model recordings are transcribed with unless the user picks
/// another. Chosen by measurement (see `eval/`): on the practice recordings
/// it is as accurate as the larger Whisper model and half again as fast.
pub const DEFAULT_SPEECH_MODEL: &str =
    "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf";
/// The name of the folder meetings are kept in.
pub const LIBRARY_NAME: &str = "서기";

/// A folder watched for new recordings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WatchedFolder {
    pub path: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// The minutes template for recordings from this folder; empty lets the
    /// calendar or the content decide.
    #[serde(default)]
    pub template_id: String,
    /// When watching began (milliseconds since 1970 UTC). Recordings in the
    /// folder from before then are left for the user to bring in; anything
    /// that arrives later is taken, whatever date the file itself carries.
    /// 0 takes everything.
    #[serde(default)]
    pub since_ms: i64,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct Config {
    /// Where meetings are kept: one folder each, holding the original
    /// recording, the minutes and the transcript. Empty for the default,
    /// `Documents/서기`.
    pub library_dir: String,
    pub watched: Vec<WatchedFolder>,
    /// The language recordings are in.
    pub language: String,
    /// Tell speakers apart.
    pub diarize: bool,
    /// The speech model for recordings; empty for [`DEFAULT_SPEECH_MODEL`].
    pub speech_model: String,
    /// The model that writes minutes.
    pub summary: Provider,
    /// The user's vocabulary: a file or a folder. Empty for
    /// `vocabulary.txt` in the app's data folder.
    pub vocabulary_path: String,
    /// The microphone for recordings made in the app; empty for the
    /// system's default.
    pub input_device: String,
    /// Secret iCal addresses of the user's calendars.
    pub calendar_urls: Vec<String>,
    /// The calendars of macOS's Calendar app to read (by their id there).
    /// Empty reads none.
    pub mac_calendars: Vec<String>,
    /// Show a reminder when a calendar event starts.
    pub remind: bool,
    /// Minutes before an event's start to show the reminder.
    pub remind_minutes_before: u32,
    /// Whether the first-run choices have been made.
    pub set_up: bool,
    /// The keys that start and stop a recording from anywhere, and that mark
    /// the moment as important. Empty for none.
    pub record_shortcut: String,
    pub mark_shortcut: String,
    /// Calendar events (by their id) the user does not want to be reminded
    /// of.
    pub muted_events: Vec<String>,
    /// Put a copy of the minutes beside a recording that came from a
    /// watched folder. That folder is how a phone and the computer share
    /// files, so it is where the minutes are looked for on the phone.
    pub minutes_beside_source: bool,
    /// Dictation: a shortcut or the floating key, speech, and the words
    /// typed where the cursor is. On for a new install.
    pub dictation: DictationConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            library_dir: String::new(),
            watched: Vec::new(),
            language: "ko".to_string(),
            diarize: true,
            speech_model: String::new(),
            summary: Provider::default_ollama(),
            vocabulary_path: String::new(),
            input_device: String::new(),
            calendar_urls: Vec::new(),
            mac_calendars: Vec::new(),
            remind: true,
            remind_minutes_before: 1,
            set_up: false,
            record_shortcut: "Ctrl+Alt+R".to_string(),
            mark_shortcut: "Ctrl+Alt+M".to_string(),
            muted_events: Vec::new(),
            minutes_beside_source: true,
            dictation: DictationConfig::default(),
        }
    }
}

impl Config {
    /// Reads the settings from `data_dir`. A missing or unreadable file gives
    /// the defaults: the app must start either way.
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join(FILE_NAME);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                log::warn!("{} could not be read ({e}); using defaults", path.display());
                Config::default()
            }),
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(data_dir)
            .with_context(|| format!("could not create {}", data_dir.display()))?;
        let json = serde_json::to_string_pretty(self)?;
        super::store::write_atomically(&data_dir.join(FILE_NAME), json.as_bytes())
    }

    /// The speech model recordings are transcribed with.
    pub fn speech_model(&self) -> &str {
        if self.speech_model.trim().is_empty() {
            DEFAULT_SPEECH_MODEL
        } else {
            self.speech_model.trim()
        }
    }

    /// Where meetings are kept.
    pub fn library(&self) -> PathBuf {
        if self.library_dir.trim().is_empty() {
            default_library()
        } else {
            PathBuf::from(self.library_dir.trim())
        }
    }

    /// The vocabulary file or folder.
    pub fn vocabulary(&self, data_dir: &Path) -> PathBuf {
        if self.vocabulary_path.trim().is_empty() {
            data_dir.join("vocabulary.txt")
        } else {
            PathBuf::from(self.vocabulary_path.trim())
        }
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// `Documents/서기` in the user's home.
pub fn default_library() -> PathBuf {
    home()
        .map(|home| home.join("Documents"))
        .unwrap_or_else(std::env::temp_dir)
        .join(LIBRARY_NAME)
}

/// A Google Drive for desktop account on this computer. Each person uses
/// their own: the app looks for what is there and never assumes an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DriveAccount {
    /// Whose it is, as far as the system says: the account's address on
    /// macOS, the drive's letter on Windows.
    pub account: String,
    /// The folder for phone recordings in that Drive, whether or not it
    /// has been made yet.
    pub inbox: String,
    pub inbox_exists: bool,
}

/// Every Google Drive for desktop account found on this computer, by
/// looking at where Drive puts them. No sign-in is involved.
pub fn drive_accounts() -> Vec<DriveAccount> {
    let mut mounts: Vec<(String, PathBuf)> = Vec::new();
    if cfg!(target_os = "macos") {
        // ~/Library/CloudStorage/GoogleDrive-<account>/
        if let Some(cloud) = home().map(|home| home.join("Library").join("CloudStorage")) {
            mounts.extend(
                std::fs::read_dir(cloud)
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                    .filter_map(|path| {
                        let account = path
                            .file_name()?
                            .to_str()?
                            .strip_prefix("GoogleDrive-")?
                            .to_string();
                        Some((account, path))
                    }),
            );
        }
    } else if cfg!(windows) {
        // Drive for desktop mounts each account as a drive letter, G: for
        // the first unless it is taken. Windows does not say whose it is.
        mounts.extend(
            ('D'..='Z').map(|letter| (format!("{letter}:"), PathBuf::from(format!("{letter}:\\")))),
        );
    }
    mounts.sort();
    drive_accounts_in(&mounts)
}

/// The accounts among `mounts` (who, and where their Drive is mounted): the
/// ones that have a "My Drive" in them.
fn drive_accounts_in(mounts: &[(String, PathBuf)]) -> Vec<DriveAccount> {
    const MY_DRIVE: [&str; 2] = ["내 드라이브", "My Drive"];
    mounts
        .iter()
        .filter_map(|(account, mount)| {
            let my_drive = MY_DRIVE
                .iter()
                .map(|name| mount.join(name))
                .find(|path| path.is_dir())?;
            let inbox = my_drive.join(INBOX_NAME);
            Some(DriveAccount {
                account: account.clone(),
                inbox_exists: inbox.is_dir(),
                inbox: inbox.display().to_string(),
            })
        })
        .collect()
}

/// Where to offer a `녹음함` when no Google Drive is on this computer: the
/// home folder. Nothing is created here; the caller does that once the user
/// agrees.
pub fn local_inbox() -> PathBuf {
    home().unwrap_or_else(std::env::temp_dir).join(INBOX_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_a_round_trip_and_tolerate_old_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.watched.push(WatchedFolder {
            path: "/tmp/녹음함".into(),
            enabled: true,
            template_id: "consult".into(),
            since_ms: 0,
        });
        config.summary = Provider::ClaudeCli {
            model: "sonnet".into(),
        };
        config.save(dir.path()).unwrap();
        assert_eq!(Config::load(dir.path()), config);

        // A file from an older version, missing most keys, still loads.
        std::fs::write(dir.path().join(FILE_NAME), r#"{"language": "en"}"#).unwrap();
        let loaded = Config::load(dir.path());
        assert_eq!(loaded.language, "en");
        assert!(loaded.diarize);
        // A broken file gives the defaults instead of stopping the app.
        std::fs::write(dir.path().join(FILE_NAME), "{ not json").unwrap();
        assert_eq!(Config::load(dir.path()), Config::default());
    }

    /// Dictation is on for a new install; a file that already says either
    /// way keeps what it says.
    #[test]
    fn dictation_is_on_by_default_and_kept_from_a_saved_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Config::load(dir.path()).dictation.enabled);

        std::fs::write(dir.path().join(FILE_NAME), r#"{"language": "ko"}"#).unwrap();
        assert!(Config::load(dir.path()).dictation.enabled);

        for saved in [false, true] {
            let text = format!(r#"{{"dictation": {{"enabled": {saved}}}}}"#);
            std::fs::write(dir.path().join(FILE_NAME), text).unwrap();
            assert_eq!(Config::load(dir.path()).dictation.enabled, saved);
        }
    }

    #[test]
    fn empty_paths_mean_the_defaults() {
        let config = Config::default();
        assert!(config.library().ends_with(LIBRARY_NAME));
        assert_eq!(
            config.vocabulary(Path::new("/data")),
            PathBuf::from("/data/vocabulary.txt")
        );
        assert!(local_inbox().ends_with(INBOX_NAME));
    }

    #[test]
    fn every_drive_account_is_found_with_its_own_recordings_folder() {
        let dir = tempfile::tempdir().unwrap();
        let personal = dir.path().join("GoogleDrive-me@example.com");
        let work = dir.path().join("GoogleDrive-me@work.example");
        let empty = dir.path().join("GoogleDrive-signed-out@example.com");
        std::fs::create_dir_all(personal.join("내 드라이브")).unwrap();
        std::fs::create_dir_all(work.join("My Drive").join(INBOX_NAME)).unwrap();
        std::fs::create_dir_all(&empty).unwrap();

        let accounts = drive_accounts_in(&[
            ("me@example.com".to_string(), personal.clone()),
            ("me@work.example".to_string(), work.clone()),
            ("signed-out@example.com".to_string(), empty),
        ]);
        // An account with no "My Drive" in it is not offered.
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].account, "me@example.com");
        assert!(!accounts[0].inbox_exists);
        assert!(accounts[0].inbox.ends_with(INBOX_NAME));
        assert!(accounts[0].inbox.contains("내 드라이브"));
        assert!(accounts[1].inbox_exists);
        assert!(accounts[1].inbox.contains("My Drive"));
    }
}
