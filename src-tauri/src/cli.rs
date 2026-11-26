// Derived from Handy (https://github.com/cjpais/Handy), Copyright (c) 2025 CJ Pais, MIT License. See THIRD_PARTY_NOTICES.md.

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone, Default)]
#[command(name = "seogi", about = "서기 (Seogi) - AI 회의 서기")]
pub struct CliArgs {
    /// Start with the main window hidden
    #[arg(long)]
    pub start_hidden: bool,

    /// Disable the system tray icon
    #[arg(long)]
    pub no_tray: bool,

    /// Toggle transcription on/off (sent to running instance)
    #[arg(long)]
    pub toggle_transcription: bool,

    /// Toggle transcription with post-processing on/off (sent to running instance)
    #[arg(long)]
    pub toggle_post_process: bool,

    /// Cancel the current operation (sent to running instance)
    #[arg(long)]
    pub cancel: bool,

    /// Enable debug mode with verbose logging
    #[arg(long)]
    pub debug: bool,

    /// Transcribe this WAV (16 kHz mono) headlessly and exit. Runs the same
    /// batch transcription path as the app — no mic, no silence removal, no
    /// download (the model must already be installed). Long audio is still
    /// chunked at pauses, as in the app.
    #[arg(short = 'f', long, value_name = "WAV")]
    pub transcribe_file: Option<PathBuf>,

    /// Model id to load for --transcribe-file (default: the selected model).
    #[arg(long)]
    pub model: Option<String>,

    /// Hard-select the compute device for --transcribe-file by its registry
    /// index (see --list-devices). Omit to use the persisted accelerator
    /// setting. transcribe-cpp (whisper-family) models only.
    #[arg(long, value_name = "N")]
    pub device_index: Option<usize>,

    /// List the transcribe-cpp compute devices (with indices) and exit.
    #[arg(long)]
    pub list_devices: bool,

    /// List the available models (with ids) and exit. Pass an id to --model.
    /// Honors --json for machine-readable output.
    #[arg(long)]
    pub list_models: bool,

    /// Repeat the transcription N times (best_ms reports the fastest run).
    #[arg(long, value_name = "N")]
    pub repeat: Option<usize>,

    /// Emit --transcribe-file results as JSON.
    #[arg(long)]
    pub json: bool,

    /// Run a recording file (any format) through Seogi's pipeline headlessly
    /// and exit: speech detection, speakers, transcription. Uses --model, or
    /// the selected model.
    #[arg(long, value_name = "FILE")]
    pub seogi_transcribe: Option<PathBuf>,

    /// A vocabulary file or folder for --seogi-transcribe.
    #[arg(long, value_name = "PATH")]
    pub seogi_vocab: Option<PathBuf>,

    /// Skip telling speakers apart in --seogi-transcribe.
    #[arg(long)]
    pub seogi_no_diarize: bool,

    /// Where --seogi-transcribe writes transcript.json/.txt/.srt (default:
    /// print the text), and where minutes.json/.md go.
    #[arg(long, value_name = "DIR")]
    pub seogi_out: Option<PathBuf>,

    /// Also write minutes after --seogi-transcribe (needs --seogi-out).
    #[arg(long)]
    pub seogi_minutes: bool,

    /// Write minutes from a transcript.json made earlier, without
    /// transcribing again (needs --seogi-out). A meeting.json works too, and
    /// brings its people, customer, place and calendar event.
    #[arg(long, value_name = "JSON")]
    pub seogi_minutes_from: Option<PathBuf>,

    /// The summary model: ollama:<model>, claude[:<model>] or codex[:<model>].
    #[arg(long, value_name = "SPEC", default_value = "ollama:gemma3:12b")]
    pub seogi_llm: String,

    /// The minutes template id; by default the content decides.
    #[arg(long, value_name = "ID")]
    pub seogi_template: Option<String>,

    /// Times marked as important, comma separated: 00:01:10,00:12:30.
    #[arg(long, value_name = "TIMES")]
    pub seogi_marks: Option<String>,

    /// Ask the transcript given with --seogi-minutes-from a question and
    /// print the answer with the words it rests on, instead of writing
    /// minutes.
    #[arg(long, value_name = "QUESTION")]
    pub seogi_ask: Option<String>,

    /// Run the whole secretary headlessly against this library folder (one
    /// folder per meeting is made in it) and exit when the work is done.
    /// Combine with --seogi-import, --seogi-watch or --seogi-record. The
    /// user's own settings and library are not touched.
    #[arg(long, value_name = "DIR")]
    pub seogi_library: Option<PathBuf>,

    /// Bring this recording into --seogi-library. May be given several times.
    #[arg(long, value_name = "FILE")]
    pub seogi_import: Vec<PathBuf>,

    /// Watch this folder for recordings, as the app does.
    #[arg(long, value_name = "DIR")]
    pub seogi_watch: Option<PathBuf>,

    /// Record from the microphone for this many seconds.
    #[arg(long, value_name = "SECONDS")]
    pub seogi_record: Option<u64>,

    /// A calendar for the headless secretary: an .ics file or a secret iCal
    /// address.
    #[arg(long, value_name = "FILE_OR_URL")]
    pub seogi_calendar: Option<String>,

    /// How long to wait for a watched folder to yield a recording.
    #[arg(long, value_name = "SECONDS", default_value_t = 60)]
    pub seogi_wait: u64,
}
