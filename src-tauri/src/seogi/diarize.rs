//! Speaker diarization: who spoke when.
//!
//! The diarization model runs in a child process, the app's own executable
//! started with [`WORKER_FLAG`]. A native crash or a hung GPU driver then ends
//! only that process, a cancel is a kill, and all of the model's memory goes
//! back to the system when it exits. The parent hands it the audio in a
//! scratch file and reads the turns back from another.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};

use super::chunks::{Turn, RATE};

/// Hidden first argument that turns the executable into a diarization worker.
pub const WORKER_FLAG: &str = "--seogi-diarize-worker";
/// Worker argument: use the CPU only.
const CPU_ONLY_FLAG: &str = "--cpu-only";

/// How often the parent checks on the worker.
const POLL: Duration = Duration::from_millis(150);

/// What the worker writes when it is done.
#[derive(Debug, Serialize, Deserialize)]
struct WorkerOutput {
    turns: Vec<WorkerTurn>,
    /// The most speakers the model can tell apart.
    max_speakers: i32,
    backend: String,
    elapsed_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkerTurn {
    start_ms: i64,
    end_ms: i64,
    speaker: i32,
}

/// Whether this process was started as a diarization worker. Checked in
/// `main` before anything else of the app starts.
pub fn is_worker_invocation() -> bool {
    std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == WORKER_FLAG)
}

/// The worker: `<exe> --seogi-diarize-worker <model.gguf> <audio.f32> <out.json> [--cpu-only]`.
/// Returns the process exit code.
pub fn run_worker() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let [model, audio, out, rest @ ..] = args.as_slice() else {
        eprintln!("diarize worker: expected <model> <audio> <out>");
        return 2;
    };
    let cpu_only = rest.iter().any(|arg| arg == CPU_ONLY_FLAG);
    match diarize_in_worker(Path::new(model), Path::new(audio), cpu_only) {
        Ok(output) => {
            let written = serde_json::to_vec(&output)
                .map_err(anyhow::Error::from)
                .and_then(|json| std::fs::write(out, json).map_err(Into::into));
            match written {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("diarize worker: could not write the result: {e}");
                    1
                }
            }
        }
        Err(e) => {
            eprintln!("diarize worker: {e:#}");
            1
        }
    }
}

fn diarize_in_worker(model_path: &Path, audio_path: &Path, cpu_only: bool) -> Result<WorkerOutput> {
    use transcribe_cpp::{
        Backend, BackendMask, DiarizeExtension, DiarizeOptions, Model, ModelOptions, Role,
        SortformerDiarizeOptions, SortformerPreset,
    };

    let started = Instant::now();
    transcribe_cpp::init_logging();
    if cpu_only {
        transcribe_cpp::init_backends_with(None::<&Path>, BackendMask::CPU)
    } else {
        transcribe_cpp::init_backends_default()
    }
    .map_err(|e| anyhow!("could not start the compute backend: {e}"))?;

    let pcm = read_pcm(audio_path)?;
    let model = Model::load_with(
        model_path,
        &ModelOptions {
            backend: if cpu_only {
                Backend::Cpu
            } else {
                Backend::Auto
            },
            device: None,
            ..Default::default()
        },
    )
    .map_err(|e| anyhow!("could not load the diarization model: {e}"))?;
    if !model.roles().contains(Role::Diarize) {
        bail!(
            "the model at {} is not a diarization model",
            model_path.display()
        );
    }
    let max_speakers = model.diarize_info().map_or(0, |info| info.max_speakers);
    let backend = model.backend();

    let mut session = model
        .diarize_session()
        .map_err(|e| anyhow!("could not start diarization: {e}"))?;
    // The 10-second-lookahead operating point told speakers apart best on
    // the practice sets (86% of speech time right, against 85% for the
    // model's default and 80% for the 30-second one), so it is the default.
    // SEOGI_DIARIZE_PRESET picks another for experiments.
    let preset = match std::env::var("SEOGI_DIARIZE_PRESET").as_deref() {
        Ok("default") => SortformerPreset::Default,
        Ok("very") => SortformerPreset::VeryHighLatency,
        Ok("low") => SortformerPreset::LowLatency,
        _ => SortformerPreset::HighLatency,
    };
    let options = DiarizeOptions {
        family: Some(DiarizeExtension::Sortformer(SortformerDiarizeOptions {
            preset: Some(preset),
        })),
    };
    let segments = session
        .run(&pcm, &options)
        .map_err(|e| anyhow!("diarization failed: {e}"))?;

    Ok(WorkerOutput {
        turns: segments
            .into_iter()
            .map(|segment| WorkerTurn {
                start_ms: segment.t0_ms,
                end_ms: segment.t1_ms,
                speaker: segment.speaker_id,
            })
            .collect(),
        max_speakers,
        backend,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

/// Raw little-endian f32 samples, 16 kHz mono.
fn read_pcm(path: &Path) -> Result<Vec<f32>> {
    let bytes =
        std::fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
    if bytes.len() % 4 != 0 {
        bail!("the audio scratch file is not whole samples");
    }
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect())
}

fn write_pcm(path: &Path, pcm: &[f32]) -> Result<()> {
    use std::io::Write;
    let file = std::fs::File::create(path)
        .with_context(|| format!("could not create {}", path.display()))?;
    let mut out = std::io::BufWriter::with_capacity(1 << 20, file);
    for sample in pcm {
        out.write_all(&sample.to_le_bytes())?;
    }
    out.flush()?;
    Ok(())
}

/// Removes its files when the diarization ends, however it ends.
struct Scratch {
    audio: PathBuf,
    result: PathBuf,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.audio);
        let _ = std::fs::remove_file(&self.result);
    }
}

/// The outcome of diarizing one recording.
#[derive(Debug, Clone)]
pub struct Diarization {
    /// Speaker turns in samples, in order of start. Turns may overlap.
    pub turns: Vec<Turn>,
    /// The most speakers the model can tell apart.
    pub max_speakers: u32,
}

/// Diarizes `pcm` (16 kHz mono) with the model at `model_path`. `scratch_dir`
/// holds the two temporary files. Returns `Ok(None)` when `cancelled` was set
/// while it ran. If the GPU run fails, it is tried once more on the CPU.
pub fn diarize(
    model_path: &Path,
    pcm: &[f32],
    scratch_dir: &Path,
    cancelled: &AtomicBool,
) -> Result<Option<Diarization>> {
    std::fs::create_dir_all(scratch_dir)
        .with_context(|| format!("could not create {}", scratch_dir.display()))?;
    let stamp = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    );
    let scratch = Scratch {
        audio: scratch_dir.join(format!("diarize-{stamp}.f32")),
        result: scratch_dir.join(format!("diarize-{stamp}.json")),
    };
    write_pcm(&scratch.audio, pcm)?;

    let output = match run_worker_process(model_path, &scratch, false, cancelled)? {
        WorkerRun::Done(output) => output,
        WorkerRun::Cancelled => return Ok(None),
        WorkerRun::Failed(first_error) => {
            warn!("Diarization failed ({first_error}); trying once more on the CPU");
            match run_worker_process(model_path, &scratch, true, cancelled)? {
                WorkerRun::Done(output) => output,
                WorkerRun::Cancelled => return Ok(None),
                WorkerRun::Failed(error) => bail!("speaker diarization failed: {error}"),
            }
        }
    };

    let total = pcm.len();
    let mut turns: Vec<Turn> = output
        .turns
        .iter()
        .filter(|turn| turn.speaker > 0 && turn.end_ms > turn.start_ms)
        .map(|turn| Turn {
            start: ((turn.start_ms.max(0) as usize) * RATE / 1000).min(total),
            end: ((turn.end_ms.max(0) as usize) * RATE / 1000).min(total),
            speaker: turn.speaker as u32,
        })
        .filter(|turn| turn.end > turn.start)
        .collect();
    turns.sort_by_key(|turn| (turn.start, turn.speaker));
    info!(
        "Diarized {:.0}s of audio into {} turns on {} in {:.1}s",
        total as f64 / RATE as f64,
        turns.len(),
        output.backend,
        output.elapsed_ms as f64 / 1000.0
    );
    Ok(Some(Diarization {
        turns,
        max_speakers: output.max_speakers.max(0) as u32,
    }))
}

enum WorkerRun {
    Done(WorkerOutput),
    Cancelled,
    /// The worker exited with an error or crashed; the text says how.
    Failed(String),
}

fn run_worker_process(
    model_path: &Path,
    scratch: &Scratch,
    cpu_only: bool,
    cancelled: &AtomicBool,
) -> Result<WorkerRun> {
    let exe = std::env::current_exe().context("could not find the app's own executable")?;
    let mut command = Command::new(exe);
    command
        .arg(WORKER_FLAG)
        .arg(model_path)
        .arg(&scratch.audio)
        .arg(&scratch.result)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if cpu_only {
        command.arg(CPU_ONLY_FLAG);
    }
    super::platform::hide_console(&mut command);
    let _ = std::fs::remove_file(&scratch.result);

    let mut child = command
        .spawn()
        .context("could not start the diarization worker")?;
    // Drained on its own thread: a worker that fills the pipe would block.
    let stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut text = String::new();
        if let Some(mut stderr) = stderr {
            let _ = stderr.read_to_string(&mut text);
        }
        text
    });

    let status = loop {
        if cancelled.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Ok(WorkerRun::Cancelled);
        }
        match child
            .try_wait()
            .context("could not wait for the diarization worker")?
        {
            Some(status) => break status,
            None => std::thread::sleep(POLL),
        }
    };
    let stderr = reader.join().unwrap_or_default();

    if !status.success() {
        // The worker's own last line says what went wrong; a crash leaves none.
        let reason = stderr
            .lines()
            .rev()
            .find(|line| line.contains("diarize worker:"))
            .map(str::to_string)
            .unwrap_or_else(|| format!("the worker ended with {status}"));
        return Ok(WorkerRun::Failed(reason));
    }
    let json = std::fs::read(&scratch.result).context("the diarization worker left no result")?;
    let output: WorkerOutput =
        serde_json::from_slice(&json).context("the diarization result could not be read")?;
    Ok(WorkerRun::Done(output))
}
