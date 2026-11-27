//! From a recording's audio to its transcript.
//!
//! 1. Find where someone is speaking (voice activity detection).
//! 2. Find who is speaking when (diarization), if the model is installed.
//! 3. Cut the speech into chunks of one speaker, each no longer than the
//!    speech model's window.
//! 4. Transcribe every chunk as its own run, with the vocabulary hints, so
//!    the hints reach all of a long recording and not only its first window.
//! 5. Drop what a model makes up over silence, undo loops, apply the user's
//!    corrections (kept beside the words as transcribed, as word fixes),
//!    and note which lines the model was unsure of.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use transcribe_cpp::{RunOptions, TimestampKind};

use super::chunks::{self, Chunk, Rules, Turn, RATE};
use super::diarize;
use super::transcript::{
    collapse_loops, is_hallucinated_line, number_speakers, Corrected, Segment, Transcript, WordFix,
};
use super::vocab::{Span, Vocabulary};
use crate::audio_toolkit::chunking::speech_segments;
use crate::audio_toolkit::{normalize_transcription_output, SileroVad, SpeechScorer};
use crate::engine_supervisor::{DecodeWindow, EngineError, RunOutput};
use crate::managers::model::ModelManager;
use crate::managers::transcription::{JobModel, TranscriptionManager};

/// The most hint terms given to the speech model. It fits what it can in its
/// prompt and drops the rest from the end.
const MAX_HINTS: usize = 80;
/// Tries per chunk when a dictation's cancel interrupts it.
const MAX_CHUNK_ATTEMPTS: usize = 3;
/// Audio scored for speech at a time, so progress moves and a cancel is seen.
const VAD_STEP: usize = 30 * RATE;
/// A line is marked unsure when the speech model's mean token probability
/// for it is below this,
const UNSURE_BELOW: f32 = 0.55;
/// or its text is this repetitive (Whisper itself retries at 2.4).
const UNSURE_COMPRESSION: f32 = 2.2;

/// Where a recording is in the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    DetectingSpeech,
    Diarizing,
    Transcribing,
}

/// What the pipeline asks of whoever runs it.
pub trait Monitor: Sync {
    /// `fraction` of `stage` is done (0 to 1).
    fn progress(&self, stage: Stage, fraction: f32);
    /// Set to stop the pipeline; it returns `Ok(None)` soon after.
    fn cancel_flag(&self) -> &AtomicBool;
    /// Called before each chunk: blocks while something with priority (a
    /// dictation) is using the speech engine.
    fn wait_for_engine(&self) {}

    fn cancelled(&self) -> bool {
        self.cancel_flag()
            .load(std::sync::atomic::Ordering::Acquire)
    }
}

pub struct Request<'a> {
    /// The recording, 16 kHz mono.
    pub pcm: &'a [f32],
    /// The language spoken (`ko`).
    pub language: &'a str,
    pub vocabulary: &'a Vocabulary,
    /// What is known about this recording beforehand: its title, attendees,
    /// calendar event. Names in it are hinted first, and vocabulary entries
    /// it mentions (a customer, a site) are hinted for this recording.
    pub mentions: &'a [String],
    /// Tell speakers apart when the diarization model is installed.
    pub diarize: bool,
    /// The speech model; `None` for the one selected for dictation.
    pub model_id: Option<&'a str>,
}

/// What happened along the way, for the log and for honest reporting.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Stats {
    pub audio_ms: u64,
    /// How much of the audio is speech.
    pub speech_ms: u64,
    pub chunks: u32,
    /// Chunks the speech model could not transcribe, and so are missing.
    pub failed_chunks: u32,
    /// Lines dropped as made up over silence.
    pub dropped_lines: u32,
    /// Places in the text the user's vocabulary corrected.
    pub corrected: u32,
    pub diarized: bool,
    /// Why speakers were not told apart, when they were asked to be.
    pub diarize_skipped: Option<String>,
    /// The most speakers the diarization model can tell apart.
    pub max_speakers: u32,
    pub vad_ms: u64,
    pub diarize_ms: u64,
    pub transcribe_ms: u64,
    /// The chosen speech model, when it was not on this computer yet and
    /// the model in the transcript stood in for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stood_in_for: Option<String>,
}

pub struct Outcome {
    pub transcript: Transcript,
    pub stats: Stats,
}

/// Runs the pipeline. `Ok(None)` if the monitor cancelled it.
pub fn transcribe(
    app: &AppHandle,
    request: &Request<'_>,
    monitor: &dyn Monitor,
) -> Result<Option<Outcome>> {
    let pcm = request.pcm;
    let mut stats = Stats {
        audio_ms: (pcm.len() * 1000 / RATE) as u64,
        ..Default::default()
    };

    // 1. Where is speech?
    let started = Instant::now();
    let Some(speech) = detect_speech(app, pcm, monitor)? else {
        return Ok(None);
    };
    stats.vad_ms = started.elapsed().as_millis() as u64;
    stats.speech_ms = (speech.iter().map(Range::len).sum::<usize>() * 1000 / RATE) as u64;
    if speech.is_empty() {
        info!("The recording has no speech");
        return Ok(Some(Outcome {
            transcript: Transcript {
                language: request.language.to_string(),
                ..Default::default()
            },
            stats,
        }));
    }

    // 2. Who speaks when?
    let started = Instant::now();
    let mut turns: Vec<Turn> = Vec::new();
    if request.diarize {
        monitor.progress(Stage::Diarizing, 0.0);
        match diarizer_path(app) {
            Some(model) => {
                match diarize::diarize(&model, pcm, &scratch_dir(app)?, monitor.cancel_flag()) {
                    Ok(Some(diarization)) => {
                        stats.diarized = true;
                        stats.max_speakers = diarization.max_speakers;
                        turns = diarization.turns;
                    }
                    Ok(None) => return Ok(None),
                    // The transcript is still worth having without speakers.
                    Err(e) => {
                        warn!("Continuing without speakers: {e:#}");
                        stats.diarize_skipped = Some(format!("{e:#}"));
                    }
                }
            }
            None => {
                stats.diarize_skipped = Some("the speaker model is not installed".to_string());
            }
        }
        monitor.progress(Stage::Diarizing, 1.0);
    }
    stats.diarize_ms = started.elapsed().as_millis() as u64;

    // 3 and 4. Cut, then transcribe each chunk.
    let started = Instant::now();
    let tm = Arc::clone(&app.state::<Arc<TranscriptionManager>>());
    let model = tm.begin_recording_job(request.model_id)?;
    let job = Job {
        tm: &tm,
        model: &model,
    };
    let decoded = decode_chunks(&job, request, &speech, &turns, monitor, &mut stats);
    tm.end_recording_job(&model);
    let Some((mut segments, corrections)) = decoded? else {
        return Ok(None);
    };
    stats.transcribe_ms = started.elapsed().as_millis() as u64;

    let speakers = number_speakers(&mut segments);
    info!(
        "Transcribed {:.0}s of audio ({:.0}s of speech) into {} lines by {} speaker(s): \
         speech {:.1}s, speakers {:.1}s, text {:.1}s",
        stats.audio_ms as f64 / 1000.0,
        stats.speech_ms as f64 / 1000.0,
        segments.len(),
        speakers.len().max(1),
        stats.vad_ms as f64 / 1000.0,
        stats.diarize_ms as f64 / 1000.0,
        stats.transcribe_ms as f64 / 1000.0,
    );
    Ok(Some(Outcome {
        transcript: Transcript {
            language: request.language.to_string(),
            model: model.id.clone(),
            segments,
            speakers,
            corrections,
        },
        stats,
    }))
}

/// The stretches of `pcm` where someone speaks, in samples. `None` if
/// cancelled.
fn detect_speech(
    app: &AppHandle,
    pcm: &[f32],
    monitor: &dyn Monitor,
) -> Result<Option<Vec<Range<usize>>>> {
    monitor.progress(Stage::DetectingSpeech, 0.0);
    let model = app
        .path()
        .resolve(
            "resources/models/silero-vad-v6.2-F32.gguf",
            tauri::path::BaseDirectory::Resource,
        )
        .context("could not find the speech detection model")?;
    let mut detector =
        SileroVad::new(&model, 0.5).context("could not load the speech detection model")?;
    let frame = crate::audio_toolkit::VoiceActivityDetector::frame_samples(&detector);

    let mut probs: Vec<f32> = Vec::with_capacity(pcm.len() / frame + 1);
    let whole = pcm.len() / frame * frame;
    let step = VAD_STEP / frame * frame;
    let mut at = 0;
    while at < whole {
        if monitor.cancelled() {
            return Ok(None);
        }
        let end = (at + step).min(whole);
        probs.extend(detector.score(&pcm[at..end])?);
        at = end;
        monitor.progress(Stage::DetectingSpeech, at as f32 / pcm.len().max(1) as f32);
    }
    if whole < pcm.len() {
        let mut tail = pcm[whole..].to_vec();
        tail.resize(frame, 0.0);
        probs.extend(detector.score(&tail)?);
    }
    monitor.progress(Stage::DetectingSpeech, 1.0);
    Ok(Some(speech_segments(&probs, frame, pcm.len())))
}

fn diarizer_path(app: &AppHandle) -> Option<PathBuf> {
    app.try_state::<Arc<ModelManager>>()?.diarizer_path()
}

/// Where the pipeline keeps files it deletes again.
fn scratch_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(app
        .path()
        .app_cache_dir()
        .context("could not find the app's cache folder")?
        .join("scratch"))
}

struct Job<'a> {
    tm: &'a TranscriptionManager,
    model: &'a JobModel,
}

/// What every chunk of a recording is decoded with.
#[derive(Clone, Copy)]
struct Decoding<'a> {
    job: &'a Job<'a>,
    request: &'a Request<'a>,
    options: &'a RunOptions,
    monitor: &'a dyn Monitor,
}

/// Every chunk's lines, in order, and what the vocabulary corrected in them.
/// `None` if cancelled.
fn decode_chunks(
    job: &Job<'_>,
    request: &Request<'_>,
    speech: &[Range<usize>],
    turns: &[Turn],
    monitor: &dyn Monitor,
    stats: &mut Stats,
) -> Result<Option<(Vec<Segment>, Vec<Corrected>)>> {
    let model = job.model;
    // A model without timestamps gives one line per chunk, so its chunks are
    // kept shorter: the lines stay readable and their times stay useful.
    let timed = model.timestamps != TimestampKind::None;
    let window_secs = if timed { 30.0 } else { 20.0 };
    let plan = chunks::plan(speech, turns, request.pcm, &Rules::for_window(window_secs));
    stats.chunks = plan.len() as u32;

    let hints = if model.supports_vocabulary {
        request.vocabulary.hints(request.mentions, MAX_HINTS)
    } else {
        Vec::new()
    };
    let language = model
        .languages
        .iter()
        .any(|code| code == request.language)
        .then(|| request.language.to_string());
    let options = RunOptions {
        // The finest the model has: token rows, where it gives them, carry
        // its confidence in each piece of text.
        timestamps: model.timestamps,
        language,
        vocabulary: hints,
        ..Default::default()
    };
    info!(
        "Transcribing {} chunks with '{}' ({}): language={:?}, {} hint terms, timestamps={:?}",
        plan.len(),
        model.id,
        model.arch,
        options.language,
        options.vocabulary.len(),
        model.timestamps
    );

    let total: usize = plan
        .iter()
        .map(|chunk| chunk.range.len())
        .sum::<usize>()
        .max(1);
    let mut done = 0usize;
    let mut segments: Vec<Segment> = Vec::new();
    let mut corrections: Vec<Corrected> = Vec::new();
    let with = Decoding {
        job,
        request,
        options: &options,
        monitor,
    };
    monitor.progress(Stage::Transcribing, 0.0);
    for chunk in &plan {
        let Some(lines) = decode_chunk(&with, chunk, stats, &mut corrections, 0)? else {
            return Ok(None);
        };
        for mut line in lines {
            line.overlap = overlaps_another_speaker(&line, turns);
            segments.push(line);
        }
        done += chunk.range.len();
        monitor.progress(Stage::Transcribing, done as f32 / total as f32);

        // More than a few missing chunks means the engine is broken, not
        // that a stretch of audio was hard: stop, the transcript would not
        // be worth trusting.
        let tolerated = (stats.chunks / 10).max(2);
        if stats.failed_chunks > tolerated {
            return Err(anyhow!(
                "the speech model failed on {} parts of the recording; the last error is in the log",
                stats.failed_chunks
            ));
        }
    }
    Ok(Some((segments, corrections)))
}

/// One chunk's lines. A chunk the model cannot get through (it stops when
/// its output starts looping) is cut in two and tried again: the halves give
/// the model a different run to decode. `None` if cancelled.
fn decode_chunk(
    with: &Decoding<'_>,
    chunk: &Chunk,
    stats: &mut Stats,
    corrections: &mut Vec<Corrected>,
    depth: u32,
) -> Result<Option<Vec<Segment>>> {
    let Decoding {
        job,
        request,
        options,
        monitor,
    } = *with;
    let mut attempts = 0;
    let result = loop {
        monitor.wait_for_engine();
        if monitor.cancelled() {
            return Ok(None);
        }
        attempts += 1;
        let pcm = request.pcm[chunk.range.clone()].to_vec();
        match job.tm.run_recording_chunk(pcm, options, job.model) {
            // A dictation's cancel caught this chunk: it is not our cancel.
            Err(EngineError::Cancelled) if attempts < MAX_CHUNK_ATTEMPTS => {
                warn!("A chunk was interrupted; trying it again");
            }
            other => break other,
        }
    };

    match result {
        Ok(output) => Ok(Some(lines_of(
            chunk,
            &output,
            request.vocabulary,
            stats,
            corrections,
        ))),
        Err(error) => {
            let halves =
                (depth < 2 && chunk.range.len() > 4 * RATE).then(|| halve(chunk, request.pcm));
            match halves {
                Some((first, second)) => {
                    warn!(
                        "Chunk {:.1}-{:.1}s failed ({error}); trying it in two halves",
                        first.range.start as f64 / RATE as f64,
                        second.range.end as f64 / RATE as f64
                    );
                    let mut lines = Vec::new();
                    for half in [first, second] {
                        match decode_chunk(with, &half, stats, corrections, depth + 1)? {
                            Some(more) => lines.extend(more),
                            None => return Ok(None),
                        }
                    }
                    Ok(Some(lines))
                }
                None => {
                    warn!(
                        "Leaving out {:.1}-{:.1}s: {error}",
                        chunk.range.start as f64 / RATE as f64,
                        chunk.range.end as f64 / RATE as f64
                    );
                    stats.failed_chunks += 1;
                    Ok(Some(Vec::new()))
                }
            }
        }
    }
}

/// `chunk` cut at the quietest point of its middle third.
fn halve(chunk: &Chunk, pcm: &[f32]) -> (Chunk, Chunk) {
    const WINDOW: usize = RATE / 20;
    let third = chunk.range.len() / 3;
    let search = (chunk.range.start + third)..(chunk.range.end - third);
    let mut cut = chunk.range.start + chunk.range.len() / 2;
    let mut quietest = f32::MAX;
    let mut at = search.start;
    while at + WINDOW <= search.end.min(pcm.len()) {
        let energy: f32 = pcm[at..at + WINDOW].iter().map(|s| s * s).sum();
        if energy < quietest {
            quietest = energy;
            cut = at + WINDOW / 2;
        }
        at += WINDOW / 2;
    }
    (
        Chunk {
            range: chunk.range.start..cut,
            speaker: chunk.speaker,
        },
        Chunk {
            range: cut..chunk.range.end,
            speaker: chunk.speaker,
        },
    )
}

/// The lines of one chunk's run, timed in the whole recording and cleaned.
fn lines_of(
    chunk: &Chunk,
    output: &RunOutput,
    vocabulary: &Vocabulary,
    stats: &mut Stats,
    corrections: &mut Vec<Corrected>,
) -> Vec<Segment> {
    let transcript = &output.transcript;
    let (start, end) = (chunk.start_ms(), chunk.end_ms());
    let rows: Vec<&transcribe_cpp::Segment> = transcript
        .segments
        .iter()
        .filter(|row| !row.text.trim().is_empty())
        .collect();
    let timed = !rows.is_empty() && rows.iter().any(|row| row.t1_ms > row.t0_ms);

    let mut lines: Vec<Segment> = if timed {
        rows.into_iter()
            .map(|row| {
                let line_start = (start + row.t0_ms.max(0) as u64).min(end);
                let (confidence, unsure) = sureness(&output.decode, Some(row.t0_ms..row.t1_ms));
                Segment {
                    start_ms: line_start,
                    end_ms: (start + row.t1_ms.max(0) as u64).clamp(line_start, end),
                    speaker: chunk.speaker,
                    text: row.text.trim().to_string(),
                    confidence,
                    unsure,
                    overlap: false,
                    edited: None,
                    fixes: Vec::new(),
                }
            })
            .collect()
    } else if transcript.text.trim().is_empty() {
        Vec::new()
    } else {
        let (confidence, unsure) = sureness(&output.decode, None);
        vec![Segment {
            start_ms: start,
            end_ms: end,
            speaker: chunk.speaker,
            text: transcript.text.trim().to_string(),
            confidence,
            unsure,
            overlap: false,
            edited: None,
            fixes: Vec::new(),
        }]
    };

    lines.retain_mut(|line| {
        if is_hallucinated_line(&line.text) {
            stats.dropped_lines += 1;
            return false;
        }
        let looped = collapse_loops(&line.text);
        // A loop that had to be undone is itself a reason to listen.
        line.unsure |= looped != line.text;
        let spans = apply_vocabulary(line, &looped, vocabulary);
        stats.corrected += spans.len() as u32;
        let at = line.start_ms;
        for span in spans {
            match corrections
                .iter_mut()
                .find(|c| c.from == span.from && c.to == span.to)
            {
                Some(logged) => {
                    if !logged.at_ms.contains(&at) {
                        logged.at_ms.push(at);
                    }
                }
                None => corrections.push(Corrected {
                    from: span.from,
                    to: span.to,
                    at_ms: vec![at],
                }),
            }
        }
        !line.text.is_empty()
    });
    lines
}

/// Applies the user's vocabulary to a transcribed line, `looped` being its
/// words with loops undone. The corrected words go in `text`, cleaned after
/// correcting, as they always have (so `전사.txt` keeps them). Each place is
/// also kept as a word fix that remembers what was heard, for the screen to
/// show; a line whose cleaning and correcting do not give the same words in
/// either order keeps the corrections without the marks. Returns what was
/// corrected, once for every place.
pub(crate) fn apply_vocabulary(
    line: &mut Segment,
    looped: &str,
    vocabulary: &Vocabulary,
) -> Vec<Span> {
    let spans = vocabulary.spans(looped);
    line.text = normalize_transcription_output(&replaced(looped, &spans));
    line.edited = None;
    line.fixes = Vec::new();
    if spans.is_empty() {
        return spans;
    }
    let plain = normalize_transcription_output(looped);
    let again = vocabulary.spans(&plain);
    if replaced(&plain, &again) == line.text {
        let mut shift: i64 = 0;
        line.fixes = again
            .iter()
            .map(|span| {
                let at = plain[..span.start].chars().count() as i64 + shift;
                shift += span.to.chars().count() as i64 - span.from.chars().count() as i64;
                WordFix {
                    at: at as u32,
                    from: span.from.clone(),
                    to: span.to.clone(),
                    auto: true,
                }
            })
            .collect();
    }
    spans
}

/// `text` with every span's words put in.
fn replaced(text: &str, spans: &[Span]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    for span in spans {
        out.push_str(&text[from..span.start]);
        out.push_str(&span.to);
        from = span.end;
    }
    out.push_str(&text[from..]);
    out
}

/// How sure the speech model was of the text in `span` of a run (the whole
/// run when `None`), and whether that is unsure enough to mark: the mean
/// token probability of the decode window the span sits in, and that
/// window's signs of trouble. `(None, false)` when the model reports nothing.
fn sureness(windows: &[DecodeWindow], span: Option<Range<i64>>) -> (Option<f32>, bool) {
    let window = match &span {
        // The window holding most of the line.
        Some(span) => windows.iter().max_by_key(|window| {
            (window.t1_ms.min(span.end) - window.t0_ms.max(span.start)).max(0)
        }),
        // No timing: the run is as sure as its least sure window.
        None => windows
            .iter()
            .min_by(|a, b| a.avg_logprob.total_cmp(&b.avg_logprob)),
    };
    let Some(window) = window else {
        return (None, false);
    };
    let confidence = window.avg_logprob.exp().clamp(0.0, 1.0);
    let unsure = confidence < UNSURE_BELOW
        || window.compression_ratio > UNSURE_COMPRESSION
        || window.fallbacks > 0;
    (Some(confidence), unsure)
}

/// Whether someone else's turn overlaps most of this line: the words may be
/// a mix of two people.
fn overlaps_another_speaker(line: &Segment, turns: &[Turn]) -> bool {
    if line.speaker == 0 || line.end_ms <= line.start_ms {
        return false;
    }
    let start = line.start_ms as usize * RATE / 1000;
    let end = line.end_ms as usize * RATE / 1000;
    let other: usize = turns
        .iter()
        .filter(|turn| turn.start < end && turn.end > start)
        .map(|turn| turn.end.min(end) - turn.start.max(start))
        .sum::<usize>()
        // The line's own speaker covers it once.
        .saturating_sub(end - start);
    other * 2 > end - start
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vocabulary_s_corrections_are_in_the_words_and_remember_what_was_heard() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("v.txt");
        std::fs::write(&file, "걸레바지 => 걸레받이\n우물 천정 => 우물천장\n").unwrap();
        let vocabulary = Vocabulary::load(&file).unwrap();
        let mut line = Segment::default();
        let spans = apply_vocabulary(
            &mut line,
            "그  걸레바지는 흰색, 바지는 검정 우물 천정",
            &vocabulary,
        );
        assert_eq!(spans.len(), 2);
        // Corrected, then cleaned, as before: what 전사.txt is written from.
        assert_eq!(line.text, "그 걸레받이는 흰색, 바지는 검정 우물천장");
        assert_eq!(line.edited, None);
        assert_eq!(line.said(), line.text);
        assert_eq!(
            line.fixes,
            vec![
                WordFix {
                    at: 2,
                    from: "걸레바지".into(),
                    to: "걸레받이".into(),
                    auto: true
                },
                WordFix {
                    at: 19,
                    from: "우물 천정".into(),
                    to: "우물천장".into(),
                    auto: true
                },
            ]
        );
        let mut plain = Segment::default();
        assert!(apply_vocabulary(&mut plain, "바지는 검정", &vocabulary).is_empty());
        assert!(plain.fixes.is_empty() && plain.edited.is_none());
        // Words cleaning would change across a correction: corrected the
        // same as before, without the marks.
        let mut stutter = Segment::default();
        std::fs::write(&file, "우물천장탄 => 우물천장\n").unwrap();
        let vocabulary = Vocabulary::load(&file).unwrap();
        let spans = apply_vocabulary(&mut stutter, "우물천장 우물천장탄", &vocabulary);
        assert_eq!(spans.len(), 1);
        assert_eq!(
            stutter.text,
            normalize_transcription_output("우물천장 우물천장")
        );
    }
}
