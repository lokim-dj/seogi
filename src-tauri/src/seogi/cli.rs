//! Headless runs of the pipeline, for checking it without the window and for
//! the evaluation scripts under `eval/`.
//!
//!     seogi --seogi-transcribe meeting.m4a --seogi-out out/ --seogi-minutes
//!     seogi --seogi-minutes-from out/transcript.json --seogi-out out/ --seogi-llm claude:sonnet
//!     seogi --seogi-minutes-from meeting.json --seogi-out out/   # with its people, place, event
//!
//! and of the whole secretary, as the app runs it, against a library folder
//! of its own:
//!
//!     seogi --seogi-library lib/ --seogi-import meeting.m4a
//!     seogi --seogi-library lib/ --seogi-watch inbox/ --seogi-wait 120
//!     seogi --seogi-library lib/ --seogi-record 10

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use tauri::AppHandle;

use super::llm::Provider;
use super::minutes;
use super::pipeline::{self, Monitor, Request, Stage};
use super::render::{self, Header};
use super::templates;
use super::transcript::{parse_clock, Transcript};
use super::vocab::Vocabulary;
use super::{audio_in, chunks::RATE};
use crate::cli::CliArgs;

/// Prints progress to stderr, one line per stage and each tenth of it.
struct ConsoleMonitor {
    cancelled: AtomicBool,
    last: std::sync::Mutex<Option<(Stage, u32)>>,
}

impl Monitor for ConsoleMonitor {
    fn progress(&self, stage: Stage, fraction: f32) {
        let tenth = (fraction.clamp(0.0, 1.0) * 10.0) as u32;
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if *last != Some((stage, tenth)) {
            *last = Some((stage, tenth));
            eprintln!("[{stage:?}] {}%", tenth * 10);
        }
    }

    fn cancel_flag(&self) -> &AtomicBool {
        &self.cancelled
    }
}

/// Runs what the arguments ask for and returns the process exit code:
/// 0 done, 1 failed, 2 bad input.
pub fn run(app: &AppHandle, args: &CliArgs) -> i32 {
    match run_inner(app, args) {
        Ok(()) => 0,
        Err(Failure::Input(e)) => {
            eprintln!("error: {e:#}");
            2
        }
        Err(Failure::Run(e)) => {
            eprintln!("error: {e:#}");
            1
        }
    }
}

enum Failure {
    /// The arguments or the files they name are wrong.
    Input(anyhow::Error),
    /// The work itself failed.
    Run(anyhow::Error),
}

fn run_inner(app: &AppHandle, args: &CliArgs) -> Result<(), Failure> {
    if let Some(library) = &args.seogi_library {
        return run_secretary(app, args, library);
    }
    let wants_minutes = args.seogi_minutes || args.seogi_minutes_from.is_some();
    if wants_minutes && args.seogi_out.is_none() && args.seogi_ask.is_none() {
        return Err(Failure::Input(anyhow!("minutes need --seogi-out")));
    }
    let provider = Provider::parse(&args.seogi_llm).ok_or_else(|| {
        Failure::Input(anyhow!(
            "--seogi-llm takes ollama:<model>, claude[:<model>] or codex[:<model>]"
        ))
    })?;
    let marks: Vec<u64> = match &args.seogi_marks {
        Some(text) => text
            .split(',')
            .map(|time| {
                parse_clock(time)
                    .ok_or_else(|| Failure::Input(anyhow!("'{time}' is not a time like 00:12:30")))
            })
            .collect::<Result<_, _>>()?,
        None => Vec::new(),
    };

    // A meeting.json given to --seogi-minutes-from brings what the user told
    // about the meeting too: the people, the customer, the place, the event.
    let mut meeting: Option<super::store::Meeting> = None;
    let (transcript, audio_ms) = match (&args.seogi_minutes_from, &args.seogi_transcribe) {
        (Some(json), _) => {
            let text = std::fs::read_to_string(json)
                .with_context(|| format!("cannot read {}", json.display()))
                .map_err(Failure::Input)?;
            let value: serde_json::Value = serde_json::from_str(&text)
                .context("it is not a transcript.json")
                .map_err(Failure::Input)?;
            let transcript: Transcript =
                serde_json::from_value(value.get("transcript").cloned().unwrap_or(value.clone()))
                    .context("it is not a transcript.json")
                    .map_err(Failure::Input)?;
            let audio_ms = value
                .pointer("/stats/audio_ms")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_else(|| transcript.duration_ms());
            if value.get("started_at_ms").is_some() {
                meeting = serde_json::from_value(value)
                    .context("it looks like a meeting.json but cannot be read as one")
                    .map(Some)
                    .map_err(Failure::Input)?;
            }
            (transcript, audio_ms)
        }
        (None, Some(file)) => transcribe(app, args, file)?,
        (None, None) => return Err(Failure::Input(anyhow!("nothing to do"))),
    };

    if let Some(question) = &args.seogi_ask {
        let no_secrets = |_: &str| None;
        let answer = super::ask::answer(&transcript, question, &provider, &no_secrets)
            .map_err(Failure::Run)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&answer).map_err(|e| Failure::Run(e.into()))?
        );
        return Ok(());
    }
    if !wants_minutes {
        return Ok(());
    }
    let out_dir = args.seogi_out.as_deref().expect("checked above");
    let started = Instant::now();
    let all_templates = templates::built_in();
    let cancelled = AtomicBool::new(false);
    let no_secrets = |_: &str| None;
    let mut people: Vec<String> = Vec::new();
    let (mut recorded_on, mut context, mut customer) =
        (String::new(), String::new(), String::new());
    let mut template_id = args.seogi_template.clone();
    if let Some(meeting) = &meeting {
        people = meeting.people.clone();
        if !meeting.customer.is_empty() && !people.contains(&meeting.customer) {
            people.push(meeting.customer.clone());
        }
        customer = meeting.customer.clone();
        recorded_on = meeting.recorded_on();
        context = meeting.context();
        if template_id.is_none() && !meeting.template_id.is_empty() {
            template_id = Some(meeting.template_id.clone());
        }
    }
    let input = minutes::Input {
        transcript: &transcript,
        templates: &all_templates,
        template_id: template_id.as_deref(),
        marks_ms: &marks,
        mark_notes: &[],
        people: &people,
        customer: &customer,
        recorded_on: &recorded_on,
        context: &context,
        feedback_rules: super::feedback::EXAMPLE_RULES,
        provider: &provider,
        secrets: &no_secrets,
    };
    let last_tenth = std::sync::Mutex::new(u32::MAX);
    let progress = |fraction: f32| {
        let tenth = (fraction * 10.0) as u32;
        let mut last = last_tenth.lock().unwrap_or_else(|e| e.into_inner());
        if *last != tenth {
            *last = tenth;
            eprintln!("[Minutes] {}%", tenth * 10);
        }
    };
    let output = minutes::write(&input, &progress, &cancelled)
        .map_err(Failure::Run)?
        .ok_or_else(|| Failure::Run(anyhow!("cancelled")))?;

    let mut named = transcript.clone();
    for (id, name) in &output.speaker_names {
        if let Some(speaker) = named.speakers.iter_mut().find(|s| s.id == *id) {
            speaker.name = name.clone();
        }
    }
    named.merge_same_names(true);
    let header = match &meeting {
        Some(meeting) => Header {
            transcribed_by: transcript.model.clone(),
            ..meeting.header()
        },
        None => Header {
            duration_ms: audio_ms,
            mid_talk: super::facts::starts_mid_talk(&transcript),
            transcribed_by: transcript.model.clone(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            ..Default::default()
        },
    };
    let markdown = render::minutes_markdown(&output.minutes, &named, &header, &marks);
    let elapsed = started.elapsed().as_secs_f64();
    let json = serde_json::json!({
        "minutes": output.minutes,
        "speaker_names": output.speaker_names,
        "elapsed_secs": elapsed,
    });
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(out_dir)?;
        std::fs::write(
            out_dir.join("minutes.json"),
            serde_json::to_string_pretty(&json).map_err(std::io::Error::other)?,
        )?;
        std::fs::write(out_dir.join("minutes.md"), &markdown)
    };
    write()
        .with_context(|| format!("cannot write to {}", out_dir.display()))
        .map_err(Failure::Run)?;
    eprintln!(
        "minutes in {:.1}s by {}: {} verified, {} to check, {} dropped",
        elapsed,
        output.minutes.written_by,
        output.minutes.verified,
        output.minutes.to_check,
        output.minutes.dropped
    );
    Ok(())
}

/// Runs the secretary as the app does, but against `library` and with
/// settings of its own, until everything given to it is done.
fn run_secretary(app: &AppHandle, args: &CliArgs, library: &Path) -> Result<(), Failure> {
    use super::app::Secretary;
    use super::config::{Config, WatchedFolder};
    use super::store::{Origin, Status};
    use std::time::{Duration, Instant};

    let provider = Provider::parse(&args.seogi_llm).ok_or_else(|| {
        Failure::Input(anyhow!(
            "--seogi-llm takes ollama:<model>, claude[:<model>] or codex[:<model>]"
        ))
    })?;
    std::fs::create_dir_all(library)
        .with_context(|| format!("cannot create {}", library.display()))
        .map_err(Failure::Input)?;
    let mut config = Config {
        library_dir: library.display().to_string(),
        summary: provider,
        diarize: !args.seogi_no_diarize,
        speech_model: args.model.clone().unwrap_or_default(),
        vocabulary_path: args
            .seogi_vocab
            .as_ref()
            .map_or(String::new(), |path| path.display().to_string()),
        ..Config::default()
    };
    if let Some(calendar) = &args.seogi_calendar {
        config.calendar_urls.push(calendar.clone());
        config.remind = false;
    }
    if let Some(folder) = &args.seogi_watch {
        config.watched.push(WatchedFolder {
            path: folder.display().to_string(),
            enabled: true,
            template_id: args.seogi_template.clone().unwrap_or_default(),
            since_ms: 0,
        });
    }
    // This run's settings and ledger stay with its library.
    let secretary = Secretary::new(app, library.join(".seogi"), config, true);
    secretary.read_calendars();
    secretary.start();

    for file in &args.seogi_import {
        let imported = secretary
            .import(
                file,
                Origin::Imported,
                args.seogi_template.as_deref().unwrap_or(""),
            )
            .map_err(Failure::Input)?;
        eprintln!(
            "{} {} as {}",
            if imported.duplicate {
                "already have"
            } else {
                "queued"
            },
            file.display(),
            imported.id
        );
    }
    if let Some(seconds) = args.seogi_record {
        let id = secretary
            .start_recording(None, None)
            .map_err(Failure::Run)?;
        eprintln!("recording {id} for {seconds} s");
        for _ in 0..seconds {
            std::thread::sleep(Duration::from_secs(1));
            if let Some(state) = secretary.recording() {
                eprintln!(
                    "  {:>4.1} s  level {:.3}{}",
                    state.elapsed_ms as f64 / 1_000.0,
                    state.level,
                    if state.no_sound {
                        "  (no sound arriving)"
                    } else {
                        ""
                    }
                );
            }
        }
        secretary.stop_recording().map_err(Failure::Run)?;
    }

    // A watched folder needs a few looks before a file counts as settled.
    let deadline = Instant::now() + Duration::from_secs(args.seogi_wait);
    while args.seogi_watch.is_some() && secretary.meetings().is_empty() && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(500));
    }
    while !secretary.is_settled() {
        std::thread::sleep(Duration::from_millis(500));
    }
    // Minutes that failed are tried again after a minute or more; wait for
    // that as long as --seogi-wait allows.
    let deadline = Instant::now() + Duration::from_secs(args.seogi_wait);
    let waiting_on_minutes = |secretary: &Secretary| {
        secretary
            .meetings()
            .iter()
            .any(|meeting| meeting.status == Status::Transcribed)
    };
    while (waiting_on_minutes(&secretary) || !secretary.is_settled()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
    }

    let meetings = secretary.meetings();
    for meeting in &meetings {
        println!(
            "{:?}\t{}\t{}\t{} tasks, {} verified, {} to check\t{}{}",
            meeting.status,
            meeting.id,
            if meeting.title.is_empty() {
                "(no title)"
            } else {
                &meeting.title
            },
            meeting.actions,
            meeting.verified,
            meeting.to_check,
            meeting.folder,
            if meeting.error.is_empty() {
                String::new()
            } else {
                format!("\t[{}]", meeting.error)
            }
        );
    }
    if meetings.is_empty() {
        return Err(Failure::Run(anyhow!("nothing arrived")));
    }
    if meetings
        .iter()
        .all(|meeting| meeting.status == Status::Done)
    {
        Ok(())
    } else {
        Err(Failure::Run(anyhow!("not every recording was finished")))
    }
}

fn transcribe(app: &AppHandle, args: &CliArgs, file: &Path) -> Result<(Transcript, u64), Failure> {
    let started = Instant::now();
    let pcm = audio_in::decode(file)
        .with_context(|| format!("cannot read {}", file.display()))
        .map_err(Failure::Input)?;
    eprintln!(
        "decoded {:.1}s of audio in {:.1}s",
        pcm.len() as f64 / RATE as f64,
        started.elapsed().as_secs_f64()
    );
    let vocabulary = match args.seogi_vocab.as_deref().map(Vocabulary::load) {
        Some(Ok(vocabulary)) => vocabulary,
        Some(Err(e)) => return Err(Failure::Input(e.context("cannot read the vocabulary"))),
        None => Vocabulary::default(),
    };

    let monitor = ConsoleMonitor {
        cancelled: AtomicBool::new(false),
        last: std::sync::Mutex::new(None),
    };
    let request = Request {
        pcm: &pcm,
        language: "ko",
        vocabulary: &vocabulary,
        mentions: &[],
        diarize: !args.seogi_no_diarize,
        model_id: args.model.as_deref(),
    };
    let outcome = pipeline::transcribe(app, &request, &monitor)
        .map_err(Failure::Run)?
        .ok_or_else(|| Failure::Run(anyhow!("cancelled")))?;

    let total_secs = started.elapsed().as_secs_f64();
    let audio_secs = pcm.len() as f64 / RATE as f64;
    eprintln!(
        "done in {:.1}s ({:.1}x real time): {} lines, {} speaker(s), {} chunk(s), {} failed, {} dropped",
        total_secs,
        audio_secs / total_secs.max(1e-6),
        outcome.transcript.segments.len(),
        outcome.transcript.speakers.len(),
        outcome.stats.chunks,
        outcome.stats.failed_chunks,
        outcome.stats.dropped_lines
    );

    match &args.seogi_out {
        Some(dir) => write_outputs(dir, &outcome, total_secs)
            .with_context(|| format!("cannot write to {}", dir.display()))
            .map_err(Failure::Run)?,
        None => print!("{}", outcome.transcript.to_text()),
    }
    Ok((outcome.transcript, outcome.stats.audio_ms))
}

fn write_outputs(dir: &Path, outcome: &pipeline::Outcome, total_secs: f64) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::json!({
        "transcript": outcome.transcript,
        "stats": outcome.stats,
        "elapsed_secs": total_secs,
    });
    std::fs::write(
        dir.join("transcript.json"),
        serde_json::to_string_pretty(&json).map_err(std::io::Error::other)?,
    )?;
    std::fs::write(dir.join("transcript.txt"), outcome.transcript.to_text())?;
    std::fs::write(dir.join("transcript.srt"), outcome.transcript.to_srt())?;
    Ok(())
}
