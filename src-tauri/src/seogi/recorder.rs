//! Recording a meeting in the app.
//!
//! The microphone is written straight to a WAV file in the meeting's folder
//! as it arrives: 16 kHz, one channel, 16 bits, about 115 MB an hour. The
//! file's header is brought up to date every two seconds, so if the app or
//! the computer goes down mid-meeting the file on disk is a valid recording
//! of everything up to that moment, and [`repair`] recovers the rest that
//! reached the disk.

use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use handy_recorder::{AudioChunk, Channels, EndReason, Recorder, RecorderConfig, Sink};
use hound::{SampleFormat, WavSpec, WavWriter};

/// The rate recordings are made at: what speech models listen at.
pub const RATE: u32 = 16_000;
/// Frames in each piece the microphone hands over: a tenth of a second.
const CHUNK_FRAMES: usize = 1_600;
/// Pieces between two updates of the file's header: two seconds.
const CHUNKS_PER_FLUSH: u32 = 20;
/// This long with every sample exactly zero means no sound is arriving at
/// all (a muted or refused microphone), which a quiet room never does.
const DEAD_AFTER_CHUNKS: u32 = 30;

fn spec() -> WavSpec {
    WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    }
}

/// What the recording thread and the app share.
#[derive(Default)]
struct Shared {
    frames: AtomicU64,
    /// The loudest sample of the latest piece, as `f32` bits.
    level: AtomicU32,
    /// Pieces in a row that were exactly silent.
    dead_chunks: AtomicU32,
    failed: AtomicBool,
    write_error: Mutex<Option<String>>,
}

/// Writes what the microphone hears to the file.
struct WavSink {
    writer: Option<WavWriter<BufWriter<File>>>,
    shared: Arc<Shared>,
    since_flush: u32,
}

impl WavSink {
    fn create(path: &Path, shared: Arc<Shared>) -> Result<Self> {
        let writer = WavWriter::create(path, spec())
            .with_context(|| format!("could not create {}", path.display()))?;
        Ok(Self {
            writer: Some(writer),
            shared,
            since_flush: 0,
        })
    }

    fn fail(&mut self, error: hound::Error) {
        self.shared.failed.store(true, Ordering::Release);
        *self
            .shared
            .write_error
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(error.to_string());
        // What is on disk stays as it is; nothing more is written to it.
        if let Some(writer) = self.writer.take() {
            std::mem::forget(writer);
        }
    }

    /// Completes the file. Returns the frames written.
    fn finish(mut self) -> Result<u64> {
        if let Some(writer) = self.writer.take() {
            writer
                .finalize()
                .context("could not finish the recording file")?;
        }
        Ok(self.shared.frames.load(Ordering::Acquire))
    }
}

impl Sink for WavSink {
    fn process_chunk(&mut self, chunk: AudioChunk<'_>) {
        let real = &chunk.samples[..chunk.valid_frames * chunk.channels as usize];
        let peak = real.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
        self.shared.level.store(peak.to_bits(), Ordering::Release);
        if peak == 0.0 {
            self.shared.dead_chunks.fetch_add(1, Ordering::AcqRel);
        } else {
            self.shared.dead_chunks.store(0, Ordering::Release);
        }

        let Some(writer) = self.writer.as_mut() else {
            return;
        };
        for &sample in real {
            let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
            if let Err(e) = writer.write_sample(value) {
                self.fail(e);
                return;
            }
        }
        self.shared
            .frames
            .fetch_add(chunk.valid_frames as u64, Ordering::AcqRel);

        self.since_flush += 1;
        if self.since_flush >= CHUNKS_PER_FLUSH {
            self.since_flush = 0;
            // Rewrites the header for what has been written so far.
            if let Err(e) = writer.flush() {
                self.fail(e);
            }
        }
    }
}

/// A recording in progress.
pub struct Session {
    recorder: Recorder<WavSink>,
    shared: Arc<Shared>,
    /// The microphone's name, as the system gives it.
    pub device: String,
    pub path: PathBuf,
}

/// How a recording ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub frames: u64,
    /// Frames the microphone delivered that could not be kept.
    pub dropped_frames: u64,
    /// Why it ended, when it was not because it was stopped.
    pub trouble: Option<String>,
}

impl Finished {
    pub fn duration_ms(&self) -> u64 {
        self.frames * 1000 / u64::from(RATE)
    }
}

impl Session {
    /// Opens the microphone (`device` is an id from [`microphones`]; `None`
    /// for the system's default) and starts writing to `path`.
    ///
    /// `on_failure` is called, on another thread, if the microphone stops on
    /// its own: unplugged, or the computer slept. The recording up to then is
    /// on disk; call [`Session::stop`] to complete the file.
    pub fn start(
        path: &Path,
        device: Option<String>,
        on_failure: impl FnOnce(String) + Send + 'static,
    ) -> Result<Self> {
        let shared = Arc::new(Shared::default());
        let config = RecorderConfig {
            device,
            sample_rate: Some(RATE),
            channels: Channels::MixToMono,
            frames_per_chunk: Some(CHUNK_FRAMES),
            take_headset: false,
        };
        let failed = shared.clone();
        let recorder = Recorder::open_with_failure_handler(config, move |error| {
            failed.failed.store(true, Ordering::Release);
            on_failure(error.to_string());
        })
        .map_err(|e| anyhow!("{e}"))
        .context("could not open the microphone")?;
        let device = recorder.info().device.name.clone();

        let sink = WavSink::create(path, shared.clone())?;
        recorder
            .start(sink)
            .map_err(|e| anyhow!("{e}"))
            .context("could not start recording")?;
        Ok(Self {
            recorder,
            shared,
            device,
            path: path.to_path_buf(),
        })
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.shared.frames.load(Ordering::Acquire) * 1000 / u64::from(RATE)
    }

    /// How loud the latest tenth of a second was, 0 to 1.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.shared.level.load(Ordering::Acquire)).clamp(0.0, 1.0)
    }

    /// No sound at all is arriving: the microphone is muted, or the system
    /// has not let this app use it.
    pub fn is_dead(&self) -> bool {
        self.shared.dead_chunks.load(Ordering::Acquire) >= DEAD_AFTER_CHUNKS
    }

    /// The microphone stopped on its own or the file could not be written.
    pub fn has_failed(&self) -> bool {
        self.shared.failed.load(Ordering::Acquire)
    }

    /// Ends the recording and completes the file.
    pub fn stop(self) -> Result<Finished> {
        let stopped = self
            .recorder
            .stop()
            .map_err(|e| anyhow!("{e}"))
            .context("could not stop the recording")?;
        let mut trouble = match &stopped.end_reason {
            EndReason::StopCalled => None,
            EndReason::RecorderFailed(error) => Some(error.to_string()),
            EndReason::SinkPanicked(message) => Some(message.clone()),
        };
        if let Some(error) = self
            .shared
            .write_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            trouble = Some(error);
        }
        let frames = stopped.sink.finish()?;
        Ok(Finished {
            frames,
            dropped_frames: stopped.dropped_frames,
            trouble,
        })
    }
}

/// A microphone the system offers.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Microphone {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub fn microphones() -> Result<Vec<Microphone>> {
    Ok(handy_recorder::list_input_devices()
        .map_err(|e| anyhow!("{e}"))?
        .into_iter()
        .filter(|device| !device.is_monitor)
        .map(|device| Microphone {
            id: device.id,
            name: device.name,
            is_default: device.is_default,
        })
        .collect())
}

/// Makes a recording that was cut off (a crash, a power cut) whole again:
/// its header is rewritten to cover every sample that reached the disk.
/// Returns the frames it holds. The samples themselves are not touched.
pub fn repair(path: &Path) -> Result<u64> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("could not open {}", path.display()))?;
    let length = file.metadata()?.len();
    let mut head = [0u8; 128];
    let read = file.read(&mut head)?;
    let head = &head[..read];
    if read < 44 || &head[..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return Err(anyhow!("{} is not a recording made here", path.display()));
    }
    let data_at = head
        .windows(4)
        .position(|tag| tag == b"data")
        .ok_or_else(|| anyhow!("{} has no audio part", path.display()))? as u64;
    let audio_start = data_at + 8;
    // Whole 16-bit samples only.
    let audio_bytes = (length.saturating_sub(audio_start)) / 2 * 2;
    let end = audio_start + audio_bytes;

    let riff_size = u32::try_from(end - 8).unwrap_or(u32::MAX);
    let data_size = u32::try_from(audio_bytes).unwrap_or(u32::MAX);
    file.seek(SeekFrom::Start(4))?;
    file.write_all(&riff_size.to_le_bytes())?;
    file.seek(SeekFrom::Start(data_at + 4))?;
    file.write_all(&data_size.to_le_bytes())?;
    file.flush()?;
    if end < length {
        // Half a sample at the very end, from the moment it was cut off.
        file.set_len(end)?;
    }
    Ok(audio_bytes / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(sink: &mut WavSink, chunks: u32, value: f32) {
        let samples = vec![value; CHUNK_FRAMES];
        for _ in 0..chunks {
            sink.process_chunk(AudioChunk {
                samples: &samples,
                sample_rate: RATE,
                channels: 1,
                valid_frames: CHUNK_FRAMES,
            });
        }
    }

    #[test]
    fn a_stopped_recording_holds_every_sample() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("녹음.wav");
        let shared = Arc::new(Shared::default());
        let mut sink = WavSink::create(&path, shared.clone()).unwrap();
        feed(&mut sink, 25, 0.5);
        // The last piece of a recording is padded; only the real part counts.
        let samples = vec![0.25f32; CHUNK_FRAMES];
        sink.process_chunk(AudioChunk {
            samples: &samples,
            sample_rate: RATE,
            channels: 1,
            valid_frames: 100,
        });
        assert_eq!(sink.finish().unwrap(), 25 * CHUNK_FRAMES as u64 + 100);

        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec(), spec());
        let read: Vec<i16> = reader.samples::<i16>().map(Result::unwrap).collect();
        assert_eq!(read.len(), 25 * CHUNK_FRAMES + 100);
        assert_eq!(read[0], (0.5 * f32::from(i16::MAX)) as i16);
        assert_eq!(*read.last().unwrap(), (0.25 * f32::from(i16::MAX)) as i16);
        assert_eq!(f32::from_bits(shared.level.load(Ordering::Acquire)), 0.25);
    }

    #[test]
    fn a_recording_cut_off_mid_way_is_valid_up_to_the_last_two_seconds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("녹음.wav");
        let mut sink = WavSink::create(&path, Arc::new(Shared::default())).unwrap();
        // Four and a half seconds, then the app dies: nothing is finished.
        feed(&mut sink, 45, 0.5);
        std::mem::forget(sink);

        // Without any repair the file already opens and holds four seconds.
        let frames = hound::WavReader::open(&path).unwrap().duration();
        assert_eq!(frames, 40 * CHUNK_FRAMES as u32);

        // Repair covers whatever else reached the disk and never less.
        let repaired = repair(&path).unwrap();
        assert!(repaired >= 40 * CHUNK_FRAMES as u64);
        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(u64::from(reader.duration()), repaired);
        assert!(reader
            .samples::<i16>()
            .all(|s| s.unwrap() == (0.5 * f32::from(i16::MAX)) as i16));
    }

    #[test]
    fn repair_restores_a_header_that_was_never_updated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("녹음.wav");
        let mut sink = WavSink::create(&path, Arc::new(Shared::default())).unwrap();
        feed(&mut sink, 20, 0.5);
        std::mem::forget(sink);
        // Samples that reached the disk after the header was last written,
        // and half a sample from the moment of the cut.
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(&[1, 0, 2, 0, 3]).unwrap();
        drop(file);

        assert_eq!(repair(&path).unwrap(), 20 * CHUNK_FRAMES as u64 + 2);
        let mut reader = hound::WavReader::open(&path).unwrap();
        let read: Vec<i16> = reader.samples::<i16>().map(Result::unwrap).collect();
        assert_eq!(read[read.len() - 2..], [1, 2]);

        std::fs::write(dir.path().join("메모.txt"), "wav 가 아님").unwrap();
        assert!(repair(&dir.path().join("메모.txt")).is_err());
    }

    #[test]
    fn exact_silence_for_three_seconds_means_no_sound_is_arriving() {
        let dir = tempfile::tempdir().unwrap();
        let shared = Arc::new(Shared::default());
        let mut sink = WavSink::create(&dir.path().join("a.wav"), shared.clone()).unwrap();
        feed(&mut sink, DEAD_AFTER_CHUNKS, 0.0);
        assert!(shared.dead_chunks.load(Ordering::Acquire) >= DEAD_AFTER_CHUNKS);
        // A quiet room is not exact silence.
        feed(&mut sink, 1, 0.0001);
        assert_eq!(shared.dead_chunks.load(Ordering::Acquire), 0);
    }
}
