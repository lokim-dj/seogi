//! The commands the screens call. Together with the events in `app`, this is
//! the whole boundary between Seogi's backend and its UI: a screen can be
//! thrown away and built anew against this file alone.

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use super::app::{
    Banner, CalendarCheck, CalendarStatus, MeetingInfo, RecordingState, Secretary, Upcoming,
};
use super::calendar::{self, Event, TitleRule};
use super::config::{self, Config};
use super::dictation::{self, Dictation, DictationStatus};
use super::llm::{self, Provider};
use super::mac_calendar::{self, MacCalendar};
use super::mic_button;
use super::phone::PhoneStatus;
use super::recorder::{self, Microphone};
use super::sign_in::{self, SignIn, SignIns, Tool};
use super::store::{Found, Meeting, MeetingSummary, Origin, MINUTES_NAME};
use super::templates::{self, Template};
use super::vocab::Vocabulary;
use super::{audio_in, watch};
use crate::managers::model::ModelManager;

fn text(error: anyhow::Error) -> String {
    format!("{error:#}")
}

/// Runs work that reads or writes a lot off the thread the screen needs.
async fn off_screen<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

/// Every meeting, newest first.
#[tauri::command]
#[specta::specta]
pub fn seogi_meetings(secretary: State<'_, Arc<Secretary>>) -> Vec<MeetingSummary> {
    secretary.meetings()
}

/// The meetings that have `query` in their title, minutes, transcript or
/// people, each with the lines it is in.
#[tauri::command]
#[specta::specta]
pub async fn seogi_search(
    secretary: State<'_, Arc<Secretary>>,
    query: String,
) -> Result<Vec<Found>, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || Ok(secretary.search(&query))).await
}

/// One meeting with its transcript and minutes.
#[tauri::command]
#[specta::specta]
pub async fn seogi_meeting(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
) -> Result<Meeting, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || secretary.meeting(&id).map_err(text)).await
}

/// The minutes as the Markdown document saved in the meeting's folder; empty
/// when they are not written yet.
#[tauri::command]
#[specta::specta]
pub async fn seogi_minutes_document(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
) -> Result<String, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        let folder = secretary.folder(&id).map_err(text)?;
        Ok(std::fs::read_to_string(folder.join(MINUTES_NAME)).unwrap_or_default())
    })
    .await
}

/// What became of one file given to [`seogi_import`].
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct ImportResult {
    pub path: String,
    /// The meeting it became, or already was.
    pub id: String,
    pub duplicate: bool,
    /// Why it could not be brought in; empty when it was.
    pub error: String,
}

/// Brings recording files in. Folders are looked through one level deep.
#[tauri::command]
#[specta::specta]
pub async fn seogi_import(
    secretary: State<'_, Arc<Secretary>>,
    paths: Vec<String>,
) -> Result<Vec<ImportResult>, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        let mut files: Vec<std::path::PathBuf> = Vec::new();
        for path in paths.iter().map(Path::new) {
            if path.is_dir() {
                let mut inside: Vec<_> = std::fs::read_dir(path)
                    .map_err(|e| e.to_string())?
                    .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                    .filter(|file| file.is_file() && watch::is_recording(file))
                    .collect();
                inside.sort();
                files.extend(inside);
            } else {
                files.push(path.to_path_buf());
            }
        }
        Ok(files
            .iter()
            .map(|file| {
                let path = file.display().to_string();
                match secretary.import(file, Origin::Imported, "") {
                    Ok(imported) => ImportResult {
                        path,
                        id: imported.id,
                        duplicate: imported.duplicate,
                        error: String::new(),
                    },
                    Err(e) => ImportResult {
                        path,
                        id: String::new(),
                        duplicate: false,
                        error: text(e),
                    },
                }
            })
            .collect())
    })
    .await
}

/// Starts recording from the microphone. Returns the new meeting's id.
#[tauri::command]
#[specta::specta]
pub async fn seogi_start_recording(
    secretary: State<'_, Arc<Secretary>>,
    title: Option<String>,
    event_uid: Option<String>,
    event_start_ms: Option<i64>,
) -> Result<String, String> {
    let secretary = secretary.inner().clone();
    // A recording started from a calendar event belongs to that event,
    // whenever it is started.
    let event = event_uid.zip(event_start_ms);
    off_screen(move || secretary.start_recording(title, event).map_err(text)).await
}

/// Writes a note beside a mark of the recording in progress.
#[tauri::command]
#[specta::specta]
pub async fn seogi_set_mark_note(
    secretary: State<'_, Arc<Secretary>>,
    index: u32,
    note: String,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || secretary.set_mark_note(index as usize, &note).map_err(text)).await
}

/// Sets what the user knows about a meeting; fields left out stay as they
/// are. Refused while the meeting is being worked on.
#[tauri::command]
#[specta::specta]
pub async fn seogi_set_meeting_info(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    info: MeetingInfo,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || secretary.set_meeting_info(&id, info).map_err(text)).await
}

/// Ticks a task of the minutes off, or back on.
#[tauri::command]
#[specta::specta]
pub async fn seogi_set_action_done(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    section: String,
    index: u32,
    done: bool,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        secretary
            .set_action_done(&id, &section, index as usize, done)
            .map_err(text)
    })
    .await
}

/// Gives a task of the minutes the user's words; the minutes' own are kept.
#[tauri::command]
#[specta::specta]
pub async fn seogi_edit_task(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    section: String,
    index: u32,
    text: String,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        secretary
            .edit_task(&id, &section, index as usize, &text)
            .map_err(self::text)
    })
    .await
}

/// Takes a task out of the minutes, kept so it can be put back.
#[tauri::command]
#[specta::specta]
pub async fn seogi_remove_task(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    section: String,
    index: u32,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        secretary
            .remove_task(&id, &section, index as usize)
            .map_err(text)
    })
    .await
}

/// Puts a task taken out back where it was; `removed` is its place among
/// the section's tasks taken out.
#[tauri::command]
#[specta::specta]
pub async fn seogi_restore_task(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    section: String,
    removed: u32,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        secretary
            .restore_task(&id, &section, removed as usize)
            .map_err(text)
    })
    .await
}

/// Adds a task the user writes to a task section of the minutes.
#[tauri::command]
#[specta::specta]
pub async fn seogi_add_task(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    section: String,
    text: String,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || secretary.add_task(&id, &section, &text).map_err(self::text)).await
}

/// Gives a stretch of the transcript (a paragraph on the screen: the line
/// starting at `from_ms` up to `to_ms`) the user's words. The words as
/// transcribed are kept, and `전사.txt` keeps them.
#[tauri::command]
#[specta::specta]
pub async fn seogi_edit_lines(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    from_ms: u64,
    to_ms: u64,
    text: String,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        secretary
            .edit_lines(&id, from_ms, to_ms, &text)
            .map_err(self::text)
    })
    .await
}

/// How many places in a meeting fixing the misheard words `from` (picked
/// at character `at` of the line starting at `at_ms`) as `to` would fix;
/// `to` is empty while the user has not written it yet.
#[tauri::command]
#[specta::specta]
pub async fn seogi_word_places(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    at_ms: u64,
    at: u32,
    from: String,
    to: String,
) -> Result<u32, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        secretary
            .word_places(&id, at_ms, at as usize, &from, &to)
            .map_err(text)
    })
    .await
}

/// Reads the misheard words `from` as `to` in a meeting, where they were
/// picked and everywhere else they safely can be; with `remember`, the
/// vocabulary learns them for meetings from now on. The words as
/// transcribed and `전사.txt` stay.
#[tauri::command]
#[specta::specta]
pub async fn seogi_fix_word(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    at_ms: u64,
    at: u32,
    from: String,
    to: String,
    remember: bool,
) -> Result<super::app::WordFixOutcome, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        secretary
            .fix_word(&id, at_ms, at as usize, &from, &to, remember)
            .map_err(text)
    })
    .await
}

/// Undoes a fix of misheard words right after it was made: the lines as
/// they were, and with `forget` the vocabulary without the words again.
#[tauri::command]
#[specta::specta]
pub async fn seogi_unfix_words(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    before: Vec<super::transcript::Segment>,
    from: String,
    to: String,
    forget: bool,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        secretary
            .unfix_words(&id, &before, &from, &to, forget)
            .map_err(text)
    })
    .await
}

/// Reads words fixed from `from` to `to` in one paragraph on the screen
/// (the line starting at `from_ms` up to `to_ms`) as transcribed again.
#[tauri::command]
#[specta::specta]
pub async fn seogi_unfix_word(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    from_ms: u64,
    to_ms: u64,
    from: String,
    to: String,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        secretary
            .unfix_word(&id, from_ms, to_ms, &from, &to)
            .map_err(text)
    })
    .await
}

/// The user's vocabulary as one list, newest additions first.
#[tauri::command]
#[specta::specta]
pub async fn seogi_vocabulary(
    secretary: State<'_, Arc<Secretary>>,
) -> Result<super::vocab::WordList, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || secretary.vocabulary_words().map_err(text)).await
}

/// Adds or changes a word of the user's vocabulary (`old` is its name
/// before, when it is renamed) with the ways it is misheard.
#[tauri::command]
#[specta::specta]
pub async fn seogi_save_word(
    secretary: State<'_, Arc<Secretary>>,
    old: Option<String>,
    term: String,
    misheard: Vec<super::vocab::Misheard>,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        secretary
            .save_word(old.as_deref(), &term, &misheard)
            .map_err(text)
    })
    .await
}

/// Takes a word out of the user's vocabulary.
#[tauri::command]
#[specta::specta]
pub async fn seogi_remove_word(
    secretary: State<'_, Arc<Secretary>>,
    term: String,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || secretary.remove_word(&term).map_err(text)).await
}

/// What the banner window shows now, if anything.
#[tauri::command]
#[specta::specta]
pub fn seogi_banner(secretary: State<'_, Arc<Secretary>>) -> Option<Banner> {
    secretary.banner()
}

/// A button of the banner: `record`, `mute`, `open`, or `close`.
#[tauri::command]
#[specta::specta]
pub async fn seogi_banner_action(
    secretary: State<'_, Arc<Secretary>>,
    action: String,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || secretary.banner_action(&action).map_err(text)).await
}

/// Today's calendar events that have not ended.
#[tauri::command]
#[specta::specta]
pub fn seogi_today(secretary: State<'_, Arc<Secretary>>) -> Vec<Upcoming> {
    secretary.today()
}

/// Whether a calendar address can be read, before it is saved.
#[tauri::command]
#[specta::specta]
pub async fn seogi_check_calendar(source: String) -> Result<CalendarCheck, String> {
    off_screen(move || Ok(Secretary::check_calendar(&source))).await
}

/// Has Ollama download the local summary model. Returns when it is there.
#[tauri::command]
#[specta::specta]
pub async fn seogi_pull_summary_model(secretary: State<'_, Arc<Secretary>>) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || match secretary.config().summary {
        Provider::Ollama { base_url, model } => llm::ollama_pull(&base_url, &model).map_err(text),
        _ => Err("the summary model is not a local one".to_string()),
    })
    .await
}

/// Ends the recording; it is transcribed and written up on its own.
#[tauri::command]
#[specta::specta]
pub async fn seogi_stop_recording(secretary: State<'_, Arc<Secretary>>) -> Result<String, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || secretary.stop_recording().map_err(text)).await
}

/// Marks this moment of the recording as important.
#[tauri::command]
#[specta::specta]
pub async fn seogi_mark(secretary: State<'_, Arc<Secretary>>) -> Result<u64, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || secretary.mark().map_err(text)).await
}

/// The recording in progress, if any. Ask a few times a second for the
/// elapsed time and the level meter.
#[tauri::command]
#[specta::specta]
pub async fn seogi_recording(
    secretary: State<'_, Arc<Secretary>>,
) -> Result<Option<RecordingState>, String> {
    Ok(secretary.recording())
}

#[tauri::command]
#[specta::specta]
pub async fn seogi_microphones() -> Result<Vec<Microphone>, String> {
    off_screen(|| recorder::microphones().map_err(text)).await
}

#[tauri::command]
#[specta::specta]
pub fn seogi_config(secretary: State<'_, Arc<Secretary>>) -> Config {
    secretary.config()
}

#[tauri::command]
#[specta::specta]
pub async fn seogi_set_config(
    secretary: State<'_, Arc<Secretary>>,
    mut config: Config,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        // Only a model its tool lists is kept.
        config.summary = config.summary.tidied(&llm::codex_models());
        secretary.set_config(config).map_err(text)
    })
    .await
}

/// Starts watching a folder for new recordings, making it if need be.
#[tauri::command]
#[specta::specta]
pub async fn seogi_watch_folder(
    secretary: State<'_, Arc<Secretary>>,
    path: String,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || secretary.watch_folder(Path::new(&path)).map_err(text)).await
}

/// Tries a meeting again; `rewrite` writes its minutes afresh.
#[tauri::command]
#[specta::specta]
pub async fn seogi_retry(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    rewrite: bool,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || secretary.retry(&id, rewrite).map_err(text)).await
}

/// Candidate words for a meeting from documents (PDF, text, pictures).
#[tauri::command]
#[specta::specta]
pub async fn seogi_terms_from_documents(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    paths: Vec<String>,
) -> Result<super::documents::TermsFound, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        let paths: Vec<std::path::PathBuf> = paths.iter().map(std::path::PathBuf::from).collect();
        secretary.terms_from_documents(&id, &paths).map_err(text)
    })
    .await
}

/// Adds words to the vocabulary that is used for every meeting. Returns
/// how many were new.
#[tauri::command]
#[specta::specta]
pub async fn seogi_add_to_vocabulary(
    secretary: State<'_, Arc<Secretary>>,
    terms: Vec<String>,
) -> Result<u32, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        secretary
            .add_to_vocabulary(&terms)
            .map(|added| added as u32)
            .map_err(text)
    })
    .await
}

/// How the talking went in a meeting, measured from its transcript as it
/// is now (after any speaker was renamed or moved).
#[tauri::command]
#[specta::specta]
pub async fn seogi_talk(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
) -> Result<super::talk::TalkStats, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        let meeting = secretary.meeting(&id).map_err(text)?;
        Ok(meeting
            .transcript
            .as_ref()
            .map(super::talk::measure)
            .unwrap_or_default())
    })
    .await
}

/// Transcribes a meeting again from its recording, with what is now known
/// about it, and writes its minutes afresh.
#[tauri::command]
#[specta::specta]
pub async fn seogi_retranscribe(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || secretary.retranscribe(&id).map_err(text)).await
}

#[tauri::command]
#[specta::specta]
pub async fn seogi_rename_speaker(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    speaker: u32,
    name: String,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || secretary.rename_speaker(&id, speaker, &name).map_err(text)).await
}

/// Names a speaker throughout the meeting, from a moment on, or in one
/// stretch; the tasks given to the old name there follow.
#[tauri::command]
#[specta::specta]
pub async fn seogi_name_speaker(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    speaker: u32,
    name: String,
    scope: super::transcript::NameScope,
    from_ms: u64,
    to_ms: u64,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        secretary
            .name_speaker(&id, speaker, &name, scope, from_ms, to_ms)
            .map_err(text)
    })
    .await
}

/// Puts back a naming of speakers just made: who said what and who was
/// given each task as kept before it, while the meeting is still as it
/// was just after it.
#[tauri::command]
#[specta::specta]
pub async fn seogi_restore_naming(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    kept: super::app::Naming,
    now: super::app::Naming,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || secretary.restore_naming(&id, &kept, &now).map_err(text)).await
}

/// Shows the meeting's folder in Finder or Explorer.
#[tauri::command]
#[specta::specta]
pub fn seogi_open_folder(
    app: AppHandle,
    secretary: State<'_, Arc<Secretary>>,
    id: String,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let folder = secretary.folder(&id).map_err(text)?;
    app.opener()
        .open_path(folder.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// The minutes templates: the built-in ones with the user's own over them.
#[tauri::command]
#[specta::specta]
pub fn seogi_templates(secretary: State<'_, Arc<Secretary>>) -> Vec<Template> {
    templates::load(&secretary.data_dir().join("templates"))
}

/// A few seconds of the meeting's original recording, as a WAV file in
/// base64 (`data:audio/wav;base64,...` plays it): for listening to the
/// words an entry of the minutes rests on. At most a minute.
#[tauri::command]
#[specta::specta]
pub async fn seogi_audio_snippet(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    start_ms: u64,
    end_ms: u64,
) -> Result<String, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        let folder = secretary.folder(&id).map_err(text)?;
        let meeting = secretary.meeting(&id).map_err(text)?;
        let audio = folder.join(&meeting.audio_file);
        super::snippet::wav(&audio, start_ms, end_ms)
            .map(|bytes| super::snippet::base64(&bytes))
            .map_err(text)
    })
    .await
}

/// Saves the user's own API key under `name` (`openai`, `anthropic`,
/// `gemini`) in the operating system's keychain.
#[tauri::command]
#[specta::specta]
pub async fn seogi_set_key(
    secretary: State<'_, Arc<Secretary>>,
    name: String,
    value: String,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        super::secrets::set(&name, &value).map_err(text)?;
        // Minutes waiting for a way to be written may have one now.
        secretary.minutes_may_go();
        Ok(())
    })
    .await
}

/// Whether a key is saved under `name`. The key itself is never handed to
/// the screen.
#[tauri::command]
#[specta::specta]
pub async fn seogi_has_key(name: String) -> Result<bool, String> {
    off_screen(move || Ok(super::secrets::has(&name))).await
}

#[tauri::command]
#[specta::specta]
pub async fn seogi_remove_key(name: String) -> Result<(), String> {
    off_screen(move || super::secrets::remove(&name).map_err(text)).await
}

/// Asks a meeting's recording a question. The answer comes with the words
/// it rests on and when they were said; one whose words are not in the
/// transcript is marked `unsupported`.
#[tauri::command]
#[specta::specta]
pub async fn seogi_ask(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    question: String,
) -> Result<super::ask::Answer, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || secretary.ask(&id, &question).map_err(text)).await
}

/// The calendar's events in the next `hours` hours.
#[tauri::command]
#[specta::specta]
pub fn seogi_upcoming(secretary: State<'_, Arc<Secretary>>, hours: u32) -> Vec<Event> {
    secretary.upcoming(hours)
}

/// Reads the calendars again now.
#[tauri::command]
#[specta::specta]
pub async fn seogi_refresh_calendar(secretary: State<'_, Arc<Secretary>>) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        secretary.read_calendars();
        Ok(())
    })
    .await
}

/// How each saved calendar answered when last read, in the order of the
/// settings' addresses.
#[tauri::command]
#[specta::specta]
pub fn seogi_calendar_status(secretary: State<'_, Arc<Secretary>>) -> CalendarStatus {
    secretary.calendar_status()
}

/// Whether Seogi may read the calendars of macOS's Calendar app:
/// `granted`, `denied`, `restricted`, `not_determined` or `unsupported`.
#[tauri::command]
#[specta::specta]
pub fn seogi_mac_calendar_access() -> String {
    mac_calendar::access().to_string()
}

/// Asks the system whether Seogi may read the Calendar app's calendars (the
/// system shows its question the first time) and returns the answer.
#[tauri::command]
#[specta::specta]
pub async fn seogi_mac_calendar_request() -> Result<String, String> {
    off_screen(move || mac_calendar::request().map(str::to_string).map_err(text)).await
}

/// The calendars in macOS's Calendar app.
#[tauri::command]
#[specta::specta]
pub async fn seogi_mac_calendars() -> Result<Vec<MacCalendar>, String> {
    off_screen(move || mac_calendar::calendars().map_err(text)).await
}

/// Moves a meeting's folder to the system's trash (it can be brought back
/// from there). `busy` when it is being recorded or worked on.
#[tauri::command]
#[specta::specta]
pub async fn seogi_delete_meeting(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
) -> Result<(), String> {
    let secretary = Arc::clone(&secretary);
    off_screen(move || secretary.delete(&id).map_err(|e| format!("{e:#}"))).await
}

/// Brings back the meeting moved to the trash in the last minute.
#[tauri::command]
#[specta::specta]
pub async fn seogi_undo_delete(secretary: State<'_, Arc<Secretary>>) -> Result<(), String> {
    let secretary = Arc::clone(&secretary);
    off_screen(move || secretary.undo_delete().map_err(|e| format!("{e:#}"))).await
}

/// Turns reminders for a calendar event off or back on.
#[tauri::command]
#[specta::specta]
pub async fn seogi_mute_event(
    secretary: State<'_, Arc<Secretary>>,
    uid: String,
    muted: bool,
) -> Result<(), String> {
    let secretary = secretary.inner().clone();
    off_screen(move || secretary.mute_event(&uid, muted).map_err(text)).await
}

/// How calendar titles are read: the user's rules, or the starting ones.
#[tauri::command]
#[specta::specta]
pub fn seogi_calendar_rules(secretary: State<'_, Arc<Secretary>>) -> Vec<TitleRule> {
    calendar::load_rules(secretary.data_dir())
}

#[tauri::command]
#[specta::specta]
pub fn seogi_set_calendar_rules(
    secretary: State<'_, Arc<Secretary>>,
    rules: Vec<TitleRule>,
) -> Result<(), String> {
    calendar::save_rules(secretary.data_dir(), &rules).map_err(text)
}

/// Opens the file of title rules in the system's editor, writing the
/// starting rules first if the user has none yet.
#[tauri::command]
#[specta::specta]
pub fn seogi_open_calendar_rules(
    app: AppHandle,
    secretary: State<'_, Arc<Secretary>>,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let data_dir = secretary.data_dir();
    let file = data_dir.join("calendar-rules.json");
    if !file.exists() {
        calendar::save_rules(data_dir, &calendar::default_rules()).map_err(text)?;
    }
    app.opener()
        .open_path(file.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// A speech model the settings offer for meetings.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct SpeechModelOption {
    pub id: String,
    pub downloaded: bool,
    pub downloading: bool,
    /// The download, in bytes; 0 when unknown.
    pub bytes: u64,
}

/// What Seogi needs to do its work, and which of it is in place.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct Readiness {
    /// The speech model recordings are transcribed with.
    pub speech_model: String,
    pub speech_model_ready: bool,
    /// While the speech model is not here: the one recordings are
    /// transcribed with meanwhile. `None` when it is here, or when no other
    /// is (recordings then wait for the download).
    pub speech_model_stand_in: Option<String>,
    /// The speech model's download, in bytes; 0 when unknown.
    pub speech_model_bytes: u64,
    /// The user put a model of their own in `speech_model`, rather than
    /// following the default.
    pub speech_model_chosen: bool,
    /// The models the settings offer for meetings, best first.
    pub speech_models: Vec<SpeechModelOption>,
    /// The model that tells speakers apart.
    pub diarizer_ready: bool,
    /// ffmpeg is installed. Needed only for file kinds the built-in decoder
    /// cannot read (3gp, amr, wma).
    pub ffmpeg: bool,
    pub ollama_running: bool,
    pub ollama_models: Vec<String>,
    pub claude_cli: bool,
    pub codex_cli: bool,
    /// The model that writes minutes, and whether it can be reached.
    pub summary: String,
    pub summary_ready: bool,
    /// The chosen model was refused, so the tool's own choice writes the
    /// minutes until another is picked.
    pub summary_model_refused: bool,
    /// When the chosen way cannot be used now, the one that writes the
    /// minutes instead; `None` when the chosen one can, or when nothing can
    /// (then `summary_ready` is false).
    pub summary_fallback: Option<Provider>,
    /// The models offered for each signed-in tool, the recommended one
    /// first.
    pub claude_models: Vec<llm::ModelChoice>,
    pub codex_models: Vec<llm::ModelChoice>,
    pub vocabulary_path: String,
    pub vocabulary_terms: u32,
    pub library: String,
    /// The Google Drive accounts on this computer, each with the folder
    /// for phone recordings it has or would get.
    pub drives: Vec<config::DriveAccount>,
    /// Where that folder would go on a computer with no Google Drive.
    pub local_inbox: String,
    /// `granted`, `denied`, `not_determined` or `unknown`.
    pub microphone: String,
}

#[tauri::command]
#[specta::specta]
pub async fn seogi_readiness(
    secretary: State<'_, Arc<Secretary>>,
    models: State<'_, Arc<ModelManager>>,
) -> Result<Readiness, String> {
    let secretary = secretary.inner().clone();
    let models = models.inner().clone();
    off_screen(move || {
        let config = secretary.config();
        let speech_model = config.speech_model().to_string();
        let info = models.get_model_info(&speech_model);
        let speech_model_ready = info.as_ref().is_some_and(|model| model.is_downloaded);
        let speech_model_bytes = info.map_or(0, |model| model.size_mb * 1024 * 1024);
        let speech_model_stand_in = config
            .speech_model_now(|id| {
                models
                    .get_model_info(id)
                    .is_some_and(|model| model.is_downloaded)
            })
            .filter(|now| now.standing_in_for.is_some())
            .map(|now| now.model);
        let speech_model_chosen = !config.speech_model.trim().is_empty();
        let speech_models = config::STAND_IN_SPEECH_MODELS
            .iter()
            .filter_map(|id| models.get_model_info(id))
            .map(|model| SpeechModelOption {
                id: model.id,
                downloaded: model.is_downloaded,
                downloading: model.is_downloading,
                bytes: model.size_mb * 1024 * 1024,
            })
            .collect();

        let ollama_url = match &config.summary {
            Provider::Ollama { base_url, .. } => base_url.clone(),
            _ => "http://localhost:11434".to_string(),
        };
        let ollama_models = llm::ollama_models(&ollama_url);
        let claude_cli = llm::find_claude_cli().is_some();
        let codex_cli = llm::find_codex_cli().is_some();
        let summary_ready = match &config.summary {
            Provider::Ollama { model, .. } => ollama_models
                .iter()
                .any(|name| name == model || name.strip_suffix(":latest") == Some(model.as_str())),
            // Installed is not enough: the tool must be signed in too.
            Provider::ClaudeCli { .. } => {
                claude_cli && sign_in::status(Tool::Claude) == SignIn::SignedIn
            }
            Provider::CodexCli { .. } => {
                codex_cli && sign_in::status(Tool::Codex) == SignIn::SignedIn
            }
            // A key is needed; whether it works shows when it is used.
            Provider::Anthropic { key, .. } | Provider::OpenAiCompatible { key, .. } => {
                key.is_empty() || super::secrets::has(key)
            }
        };

        let summary_fallback = if summary_ready {
            None
        } else {
            llm::Ways::look(&config.summary, &|name| super::secrets::has(name))
                .to_try(&config.summary)
                .into_iter()
                .find(|provider| *provider != config.summary)
        };

        let vocabulary_path = config.vocabulary(secretary.data_dir());
        let vocabulary_terms = Vocabulary::load(&vocabulary_path).map_or(0, |vocabulary| {
            (vocabulary.core.len() + vocabulary.entries.len()) as u32
        });
        use handy_recorder::Permission;
        let microphone = match handy_recorder::permission_status() {
            Permission::Granted => "granted",
            Permission::Denied => "denied",
            Permission::NotDetermined => "not_determined",
            _ => "unknown",
        };
        Ok(Readiness {
            speech_model,
            speech_model_ready,
            speech_model_stand_in,
            speech_model_bytes,
            speech_model_chosen,
            speech_models,
            diarizer_ready: models.diarizer_path().is_some(),
            ffmpeg: audio_in::find_ffmpeg().is_some(),
            ollama_running: !ollama_models.is_empty(),
            ollama_models,
            claude_cli,
            codex_cli,
            summary: config.summary.label(),
            summary_ready,
            summary_model_refused: llm::model_refused(&config.summary),
            summary_fallback,
            claude_models: llm::claude_models(),
            codex_models: llm::codex_models(),
            vocabulary_path: vocabulary_path.display().to_string(),
            vocabulary_terms,
            library: config.library().display().to_string(),
            drives: config::drive_accounts(),
            local_inbox: config::local_inbox().display().to_string(),
            microphone: microphone.to_string(),
        })
    })
    .await
}

/// The models the company behind `provider` offers to the user's own key,
/// as its model list answers; only the ones that write text. Empty when
/// there is no key yet. An error is the company's answer.
#[tauri::command]
#[specta::specta]
pub async fn seogi_key_models(provider: Provider) -> Result<Vec<String>, String> {
    off_screen(move || llm::key_models(&provider, &|name| super::secrets::get(name)).map_err(text))
        .await
}

/// Whether Claude Code and Codex are installed and signed in. Quick: each
/// tool answers from what it has stored, so the screen may ask every few
/// seconds while a sign-in is open.
#[tauri::command]
#[specta::specta]
pub async fn seogi_sign_ins(secretary: State<'_, Arc<Secretary>>) -> Result<SignIns, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        let now = sign_in::statuses();
        // A sign-in may be the way minutes waiting to be written needed.
        if now.claude == SignIn::SignedIn || now.codex == SignIn::SignedIn {
            secretary.minutes_may_go();
        }
        Ok(now)
    })
    .await
}

/// Opens `tool`'s own sign-in in a terminal window; the browser opens from
/// there. `farewell` is shown in that window once it is done. The error
/// `not_installed` means the tool is not on this computer.
#[tauri::command]
#[specta::specta]
pub async fn seogi_sign_in(tool: Tool, farewell: String) -> Result<(), String> {
    off_screen(move || sign_in::open_sign_in(tool, &farewell).map_err(text)).await
}

/// Where bringing in the phone's recordings from Google Drive stands.
#[tauri::command]
#[specta::specta]
pub fn seogi_phone_status(secretary: State<'_, Arc<Secretary>>) -> PhoneStatus {
    secretary.phone_status()
}

/// Connects Google for the phone's recordings: opens the sign-in in the
/// browser and waits until the user is done there. Fails with `cancelled`,
/// `timed_out`, `denied`, `unavailable` or `already_connecting`, or with
/// what went wrong.
#[tauri::command]
#[specta::specta]
pub async fn seogi_phone_connect(
    app: AppHandle,
    secretary: State<'_, Arc<Secretary>>,
) -> Result<PhoneStatus, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        use tauri_plugin_opener::OpenerExt;
        secretary
            .phone_connect(|url| {
                app.opener()
                    .open_url(url, None::<&str>)
                    .map_err(|e| anyhow::anyhow!("the browser could not be opened ({e})"))
            })
            .map_err(text)?;
        Ok(secretary.phone_status())
    })
    .await
}

/// Stops waiting for the browser.
#[tauri::command]
#[specta::specta]
pub fn seogi_phone_cancel_connect(secretary: State<'_, Arc<Secretary>>) {
    secretary.phone_cancel_connect();
}

/// Forgets the Google sign-in on this computer. Meetings already brought in
/// and everything in Drive stay as they are.
#[tauri::command]
#[specta::specta]
pub async fn seogi_phone_disconnect(
    secretary: State<'_, Arc<Secretary>>,
) -> Result<PhoneStatus, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        secretary.phone_disconnect().map_err(text)?;
        Ok(secretary.phone_status())
    })
    .await
}

/// Looks at the phone's folder now ("지금 가져오기").
#[tauri::command]
#[specta::specta]
pub fn seogi_phone_look_now(secretary: State<'_, Arc<Secretary>>) {
    secretary.phone_look_now();
}

/// Opens the official page on installing `tool` in the browser. Seogi does
/// not run an installer itself.
#[tauri::command]
#[specta::specta]
pub fn seogi_open_install_page(app: AppHandle, tool: Tool) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(tool.install_page(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Downloads the model that tells speakers apart, if it is not here yet.
/// Progress arrives through the model download events.
#[tauri::command]
#[specta::specta]
pub async fn seogi_download_diarizer(models: State<'_, Arc<ModelManager>>) -> Result<(), String> {
    models
        .download_diarizer()
        .await
        .map_err(|e| format!("{e:#}"))
}

/// Downloads a speech model for meetings, once: a download already under
/// way is left to finish. Before it starts, the disk must have room for it
/// and some to spare. An error is a code the screen has words for:
/// `no_space:<bytes missing>`, or a reason.
#[tauri::command]
#[specta::specta]
pub async fn seogi_download_speech_model(
    app: AppHandle,
    models: State<'_, Arc<ModelManager>>,
    model: String,
) -> Result<(), String> {
    let models = models.inner().clone();
    let Some(info) = models.get_model_info(&model) else {
        return Err(format!("unknown model {model}"));
    };
    if info.is_downloaded || info.is_downloading {
        return Ok(());
    }
    // Two presses before the first progress report start one download.
    static STARTED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    {
        let mut started = STARTED.lock().unwrap_or_else(|e| e.into_inner());
        if started.contains(&model) {
            return Ok(());
        }
        started.push(model.clone());
    }
    let result = download_speech_model(&app, &models, &model, &info).await;
    STARTED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|id| *id != model);
    result
}

async fn download_speech_model(
    app: &AppHandle,
    models: &ModelManager,
    model: &str,
    info: &crate::managers::model::ModelInfo,
) -> Result<(), String> {
    use super::disk;
    use tauri::Emitter;
    let cache = hf_hub::Cache::from_env().path().clone();
    let needed = (info.size_mb * 1024 * 1024).saturating_sub(info.partial_size);
    let missing =
        |needed: u64| disk::free_bytes(&cache).and_then(|free| disk::shortfall(free, needed));
    if let Some(short) = missing(needed) {
        return Err(format!("no_space:{short}"));
    }
    if let Err(e) = models.download_model(model).await {
        let error = format!("{e:#}");
        log::error!("The speech model {model} could not be downloaded: {error}");
        let _ = app.emit(
            "model-download-failed",
            serde_json::json!({ "model_id": &model, "error": &error }),
        );
        let left = models.get_model_info(model).map_or(needed, |now| {
            (now.size_mb * 1024 * 1024).saturating_sub(now.partial_size)
        });
        if disk::is_disk_full(&error) {
            return Err(format!(
                "no_space:{}",
                missing(left).unwrap_or(disk::SPARE_BYTES)
            ));
        }
        if let Some(short) = missing(left) {
            return Err(format!("no_space:{short}"));
        }
        return Err(error);
    }
    Ok(())
}

// ----- dictation --------------------------------------------------------------

/// The dictation choices: Seogi's and the engine's together.
#[tauri::command]
#[specta::specta]
pub fn seogi_dictation(app: AppHandle, secretary: State<'_, Arc<Secretary>>) -> Dictation {
    dictation::read(&app, &secretary.config().dictation)
}

/// Saves the dictation choices and puts them into effect. An error is a
/// code: `shortcut_taken:<handy|record|mark>`, `shortcut_empty`,
/// `shortcut_refused`, or a reason.
#[tauri::command]
#[specta::specta]
pub async fn seogi_set_dictation(app: AppHandle, dictation: Dictation) -> Result<(), String> {
    off_screen(move || dictation::write(&app, dictation)).await
}

/// What the dictation settings need to show besides the choices: the
/// system's permission, the models, the ways to tidy, recent dictations.
#[tauri::command]
#[specta::specta]
pub async fn seogi_dictation_status(app: AppHandle) -> Result<DictationStatus, String> {
    off_screen(move || Ok(dictation::status(&app))).await
}

/// The floating key was pressed: starts or ends a dictation.
#[tauri::command]
#[specta::specta]
pub fn seogi_dictation_toggle(app: AppHandle) {
    dictation::toggle(&app);
}

/// Copies the last dictation's words, from the note that says they could
/// not be typed.
#[tauri::command]
#[specta::specta]
pub fn seogi_copy_last_dictation(app: AppHandle) -> Result<(), String> {
    dictation::copy_last(&app)
}

/// Downloads the default dictation model for a new install, from the
/// first-run screen. Dictation moves to it once it is here.
#[tauri::command]
#[specta::specta]
pub fn seogi_download_dictation_model(app: AppHandle) {
    dictation::download_default_model(&app);
}

/// Forgets dictations from the recent list (by id), or every one with
/// `None`. They are gone from this computer; returns how many went.
#[tauri::command]
#[specta::specta]
pub async fn seogi_forget_dictations(app: AppHandle, ids: Option<Vec<i64>>) -> Result<u32, String> {
    dictation::forget(&app, ids).await.map(|n| n as u32)
}

/// The floating key's menu, with its two lines in the user's language.
#[tauri::command]
#[specta::specta]
pub fn seogi_mic_menu(app: AppHandle, hide: String, settings: String) -> Result<(), String> {
    mic_button::show_menu(&app, &hide, &settings).map_err(text)
}

/// The floating key is being dragged: the system moves it, and the × to drop
/// it on shows until the button is let go.
#[tauri::command]
#[specta::specta]
pub async fn seogi_mic_drag(app: AppHandle) {
    mic_button::drag(&app);
}

/// Opens the system's page where Seogi is allowed to type into other apps.
/// Only opens it: the user turns Seogi on there themselves.
#[tauri::command]
#[specta::specta]
pub fn seogi_open_typing_permission() -> Result<(), String> {
    open_privacy_pane("Privacy_Accessibility")
}

/// Opens the system's page where Seogi is allowed to read calendars. Only
/// opens it: the user turns Seogi on there themselves.
#[tauri::command]
#[specta::specta]
pub fn seogi_open_calendar_permission() -> Result<(), String> {
    open_privacy_pane("Privacy_Calendars")
}

fn open_privacy_pane(pane: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(format!(
                "x-apple.systempreferences:com.apple.preference.security?{pane}"
            ))
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pane;
        Ok(())
    }
}

/// Whether the system will open Seogi at login, as the system has it (the
/// setting may want it while System Settings has it switched off).
#[tauri::command]
#[specta::specta]
pub fn seogi_login_item(app: AppHandle) -> crate::autostart::LoginItem {
    crate::autostart::login_item(&app)
}

/// Opens the system's list of apps that open at login, where only the user
/// can switch Seogi back on.
#[tauri::command]
#[specta::specta]
pub fn seogi_open_login_items() {
    crate::autostart::open_login_items();
}

/// Shows one notification the way Seogi shows its own. The first one is when
/// macOS asks whether Seogi may show notifications, so the first-run screen
/// sends it when the user asks, not in the middle of a meeting. The words
/// come from the screen.
#[tauri::command]
#[specta::specta]
pub fn seogi_try_notification(app: AppHandle, title: String, body: String) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| e.to_string())
}

/// Shows one of a meeting's files selected in Finder or Explorer: `minutes`
/// (회의록.md), `recording` (the original recording), or the one-page
/// summary saved as `card_png` or `card_pdf`.
#[tauri::command]
#[specta::specta]
pub fn seogi_reveal(
    app: AppHandle,
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    what: String,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let folder = secretary.folder(&id).map_err(text)?;
    let path = match what.as_str() {
        "minutes" => folder.join(MINUTES_NAME),
        "recording" => folder.join(secretary.meeting(&id).map_err(text)?.audio_file),
        "card_png" => folder.join(card_file("png")?),
        "card_pdf" => folder.join(card_file("pdf")?),
        _ => folder.clone(),
    };
    // Minutes not written yet: the folder they will be in.
    let path = if path.exists() { path } else { folder };
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| e.to_string())
}

/// The name the one-page summary ("한 장 요약") is saved under in a
/// meeting's folder, as a phone picture (`png`) or an A4 page (`pdf`).
fn card_file(kind: &str) -> Result<String, String> {
    match kind {
        "png" | "pdf" => Ok(format!("한 장 요약.{kind}")),
        _ => Err(format!("not a kind of summary: {kind}")),
    }
}

/// The largest summary picture taken: far more than a 1080×1350 picture or
/// an A4 page needs, so a mistake cannot fill the disk.
const CARD_MAX_BYTES: usize = 40 * 1024 * 1024;

/// Decodes the base64 the screen sends a picture or page in.
fn card_bytes(data: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|e| e.to_string())?;
    if bytes.is_empty() || bytes.len() > CARD_MAX_BYTES {
        return Err(format!("a summary of {} bytes", bytes.len()));
    }
    Ok(bytes)
}

/// Saves the one-page summary the screen drew (`data`, base64) into the
/// meeting's folder as 한 장 요약.png or 한 장 요약.pdf, replacing one saved
/// before; nothing else in the folder is touched. Returns the file's path.
#[tauri::command]
#[specta::specta]
pub async fn seogi_save_summary_card(
    secretary: State<'_, Arc<Secretary>>,
    id: String,
    kind: String,
    data: String,
) -> Result<String, String> {
    let secretary = secretary.inner().clone();
    off_screen(move || {
        let name = card_file(&kind)?;
        let bytes = card_bytes(&data)?;
        let path = secretary.folder(&id).map_err(text)?.join(name);
        super::store::write_atomically(&path, &bytes).map_err(text)?;
        Ok(path.display().to_string())
    })
    .await
}

/// Puts a PNG picture (`data`, base64) on the clipboard, to paste into a
/// chat.
#[tauri::command]
#[specta::specta]
pub fn seogi_copy_image(app: AppHandle, data: String) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let bytes = card_bytes(&data)?;
    let image = tauri::image::Image::from_bytes(&bytes).map_err(|e| e.to_string())?;
    app.clipboard()
        .write_image(&image)
        .map_err(|e| e.to_string())
}

/// Opens the system's trash, where a deleted meeting went.
#[tauri::command]
#[specta::specta]
pub fn seogi_open_trash() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let opened = std::process::Command::new("open")
        .args(["-a", "Finder"])
        .arg(
            std::env::var_os("HOME")
                .map(|home| Path::new(&home).join(".Trash"))
                .ok_or("no home folder")?,
        )
        .spawn();
    #[cfg(target_os = "windows")]
    let opened = std::process::Command::new("explorer.exe")
        .arg("shell:RecycleBinFolder")
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let opened = std::process::Command::new("xdg-open")
        .arg("trash:///")
        .spawn();
    opened.map(|_| ()).map_err(|e| e.to_string())
}

/// What one short request to the minutes model gave.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SummaryTry {
    pub ok: bool,
    /// How long it took, in milliseconds.
    pub ms: u32,
    /// The model the answer came from, as the minutes footer names it.
    pub model: String,
    /// Why it failed, in the words of `llm::trouble_code`; empty when it
    /// worked.
    pub trouble: String,
}

/// Asks the minutes model one tiny question, to show that the chosen model
/// answers and how quickly.
#[tauri::command]
#[specta::specta]
pub async fn seogi_try_summary(secretary: State<'_, Arc<Secretary>>) -> Result<SummaryTry, String> {
    let provider = secretary.config().summary;
    off_screen(move || {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "ok": { "type": "boolean" } },
            "required": ["ok"],
        });
        let request = llm::Request {
            system: "Answer with the JSON object {\"ok\": true}.",
            user: "ok?",
            schema: &schema,
        };
        let started = std::time::Instant::now();
        let secrets = |name: &str| super::secrets::get(name);
        let answer = llm::complete(&provider, &request, &secrets);
        let ms = started.elapsed().as_millis() as u32;
        Ok(match answer {
            Ok(_) => SummaryTry {
                ok: true,
                ms,
                model: llm::in_use(&provider).label(),
                trouble: String::new(),
            },
            Err(e) => SummaryTry {
                ok: false,
                ms,
                model: llm::in_use(&provider).label(),
                trouble: llm::trouble_code(&format!("{e:#}")).to_string(),
            },
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_summary_is_saved_only_as_png_or_pdf() {
        assert_eq!(card_file("png").unwrap(), "한 장 요약.png");
        assert_eq!(card_file("pdf").unwrap(), "한 장 요약.pdf");
        assert!(card_file("md").is_err());
        assert!(card_file("../x").is_err());
    }

    #[test]
    fn a_summary_must_be_some_bytes_of_base64() {
        assert_eq!(card_bytes("aGk=").unwrap(), b"hi");
        assert!(card_bytes("").is_err());
        assert!(card_bytes("not base64!").is_err());
    }
}
