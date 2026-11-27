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
/// another: Qwen3 ASR 1.7B. Chosen by measurement on a real consultation of
/// 1 h 50 min, against another recorder's transcript of it: the fewest
/// characters wrong of the three local models (14.0 % on that recorder's
/// audio, 17.1 % on a laptop's microphone; Whisper turbo 15.1 / 17.6 %,
/// Fun-ASR Nano 18.1 / 20.5 %), and no lines in a foreign language over
/// unclear speech, where Whisper turbo wrote 52 / 31. It takes a little
/// longer (8.2 min for that recording; Whisper turbo 7.8, Fun-ASR Nano 5.1)
/// and is a bigger download (2.2 GB). The maintainer's choice.
///
/// Until 0.1.15 it was Whisper turbo ([`STAND_IN_SPEECH_MODELS`]). A user
/// who never picked a model has an empty `speech_model`, so moves with the
/// default; one who picked a model keeps it.
pub const DEFAULT_SPEECH_MODEL: &str = QWEN3_ASR_1_7B;
/// Qwen3 ASR 1.7B (2.2 GB): the most accurate of the measured three.
pub const QWEN3_ASR_1_7B: &str = "handy-computer/Qwen3-ASR-1.7B-gguf/Qwen3-ASR-1.7B-Q8_0.gguf";
/// Whisper large-v3 turbo (0.9 GB): the meetings' default until 0.1.15.
pub const WHISPER_TURBO: &str =
    "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf";
/// Fun-ASR Nano (0.9 GB): the fastest, and dictation's default.
pub const FUN_ASR_NANO: &str =
    "handy-computer/Fun-ASR-MLT-Nano-2512-gguf/Fun-ASR-MLT-Nano-2512-Q8_0.gguf";
/// The speech models the settings offer for meetings, and the ones a
/// recording is transcribed with while the chosen one is not on this
/// computer (still downloading, or its download failed), best first by the
/// same measurement. A recording is never kept waiting for a better model
/// when one of these is here.
pub const STAND_IN_SPEECH_MODELS: &[&str] = &[QWEN3_ASR_1_7B, WHISPER_TURBO, FUN_ASR_NANO];
/// The name of the folder meetings are kept in. A debug build keeps its own
/// (`서기 개발판`): it runs beside the installed app, and must not pick up the
/// installed app's meetings, work them again or repair its recordings.
pub const LIBRARY_NAME: &str = if cfg!(debug_assertions) {
    "서기 개발판"
} else {
    "서기"
};

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
    /// Dictation: a shortcut, speech, and the words typed where the cursor
    /// is. On for a new install.
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

    /// Takes the dictation settings from `current`, the saved ones: they
    /// change only through `Secretary::update_dictation`, so a screen that
    /// sends every setting back (holding a copy from before a change) cannot
    /// undo one.
    pub fn keep_dictation_of(&mut self, current: &Config) {
        self.dictation = current.dictation.clone();
    }

    /// The file as it is on disk, for moving a value that no longer lives
    /// here (`None` when there is no readable file).
    pub fn raw(data_dir: &Path) -> Option<serde_json::Value> {
        let text = std::fs::read_to_string(data_dir.join(FILE_NAME)).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// The speech model recordings are transcribed with.
    pub fn speech_model(&self) -> &str {
        if self.speech_model.trim().is_empty() {
            DEFAULT_SPEECH_MODEL
        } else {
            self.speech_model.trim()
        }
    }

    /// The model to transcribe a recording with now, given which models are
    /// on this computer (`here`): the chosen one when it is here, otherwise
    /// the first stand-in that is. `None` when no usable model is here yet;
    /// the recording then waits for the download.
    pub fn speech_model_now(&self, here: impl Fn(&str) -> bool) -> Option<SpeechModelNow> {
        let chosen = self.speech_model();
        if here(chosen) {
            return Some(SpeechModelNow {
                model: chosen.to_string(),
                standing_in_for: None,
            });
        }
        STAND_IN_SPEECH_MODELS
            .iter()
            .find(|model| **model != chosen && here(model))
            .map(|model| SpeechModelNow {
                model: model.to_string(),
                standing_in_for: Some(chosen.to_string()),
            })
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

/// The speech model a recording is transcribed with now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechModelNow {
    pub model: String,
    /// The chosen model, when it is not on this computer and `model` stands
    /// in for it.
    pub standing_in_for: Option<String>,
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

    const QWEN: &str = "handy-computer/Qwen3-ASR-1.7B-gguf/Qwen3-ASR-1.7B-Q8_0.gguf";
    const TURBO: &str =
        "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf";
    const FUNASR: &str =
        "handy-computer/Fun-ASR-MLT-Nano-2512-gguf/Fun-ASR-MLT-Nano-2512-Q8_0.gguf";

    /// Someone who never picked a speech model (0.1.15 saved an empty one)
    /// moves to Qwen3 ASR 1.7B; someone who picked one keeps it.
    #[test]
    fn only_users_on_the_old_default_move_to_the_new_speech_model() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Config::load(dir.path()).speech_model(), QWEN);

        let saved_by_0_1_15 = r#"{"language": "ko", "speech_model": "", "set_up": true}"#;
        std::fs::write(dir.path().join(FILE_NAME), saved_by_0_1_15).unwrap();
        assert_eq!(Config::load(dir.path()).speech_model(), QWEN);

        std::fs::write(dir.path().join(FILE_NAME), r#"{"set_up": true}"#).unwrap();
        assert_eq!(Config::load(dir.path()).speech_model(), QWEN);

        for picked in [TURBO, FUNASR, "someone/else-gguf/else-Q8_0.gguf"] {
            let text = format!(r#"{{"speech_model": "{picked}", "set_up": true}}"#);
            std::fs::write(dir.path().join(FILE_NAME), text).unwrap();
            let loaded = Config::load(dir.path());
            assert_eq!(loaded.speech_model(), picked);
            // Saving again keeps the choice, and keeps an empty one empty.
            loaded.save(dir.path()).unwrap();
            assert_eq!(Config::load(dir.path()).speech_model, picked);
        }
        Config::default().save(dir.path()).unwrap();
        assert_eq!(Config::load(dir.path()).speech_model, "");
    }

    /// A recording is transcribed with the chosen model when it is here,
    /// with the best one that is here while it is not, and waits only when
    /// none is.
    #[test]
    fn a_missing_speech_model_gives_way_to_one_that_is_here() {
        let config = Config::default();
        let with = |models: &'static [&'static str]| move |id: &str| models.contains(&id);
        let now = |model: &str, standing_in_for: Option<&str>| {
            Some(SpeechModelNow {
                model: model.to_string(),
                standing_in_for: standing_in_for.map(str::to_string),
            })
        };

        assert_eq!(
            config.speech_model_now(with(&[QWEN, TURBO])),
            now(QWEN, None)
        );
        // Downloading, or the download failed: the old default stands in.
        assert_eq!(
            config.speech_model_now(with(&[TURBO, FUNASR])),
            now(TURBO, Some(QWEN))
        );
        assert_eq!(
            config.speech_model_now(with(&[FUNASR])),
            now(FUNASR, Some(QWEN))
        );
        // Nothing usable: the recording waits for the download.
        assert_eq!(config.speech_model_now(with(&[])), None);
        assert_eq!(
            config.speech_model_now(with(&["someone/else-gguf/else-Q8_0.gguf"])),
            None
        );

        // A picked model that is not here gives way too, best first.
        let picked = Config {
            speech_model: FUNASR.to_string(),
            ..Config::default()
        };
        assert_eq!(
            picked.speech_model_now(with(&[FUNASR, QWEN])),
            now(FUNASR, None)
        );
        assert_eq!(
            picked.speech_model_now(with(&[TURBO, QWEN])),
            now(QWEN, Some(FUNASR))
        );
    }

    /// A file written by a newer version (a field this one does not know)
    /// or an older one (fields missing) loads with every known value kept,
    /// so going back a version neither fails nor resets anything.
    #[test]
    fn fields_from_another_version_are_ignored_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let text = r#"{"language": "en", "remind_minutes_before": 5,
            "something_new": {"a": 1},
            "dictation": {"enabled": false, "key_moved": true, "model_standing_in": true,
                          "also_new": [1, 2]}}"#;
        std::fs::write(dir.path().join(FILE_NAME), text).unwrap();
        let loaded = Config::load(dir.path());
        assert_eq!(loaded.language, "en");
        assert_eq!(loaded.remind_minutes_before, 5);
        assert!(!loaded.dictation.enabled);
        assert!(Config::raw(dir.path()).is_some());
    }

    /// The screen sends every setting back with a copy it read earlier; the
    /// dictation settings it holds must not undo the saved ones.
    #[test]
    fn the_settings_screen_cannot_undo_dictation() {
        let mut saved = Config::default();
        saved.dictation.enabled = false;
        saved.dictation.numbers = false;
        let mut from_screen = Config {
            remind: false,
            ..Config::default()
        };
        assert!(from_screen.dictation.enabled);
        from_screen.keep_dictation_of(&saved);
        assert_eq!(from_screen.dictation, saved.dictation);
        assert!(!from_screen.remind);
    }

    #[test]
    fn a_debug_build_keeps_its_meetings_apart() {
        // Tests are debug builds; the installed app's folder is "서기".
        assert!(Config::default().library().ends_with("서기 개발판"));
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
