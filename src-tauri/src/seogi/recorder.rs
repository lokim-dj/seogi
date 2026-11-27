//! Recording a meeting in the app.
//!
//! The microphone is written straight to a WAV file in the meeting's folder
//! as it arrives: 16 kHz, one channel, 16 bits, about 115 MB an hour. The
//! file's header is brought up to date every two seconds, so if the app or
//! the computer goes down mid-meeting the file on disk is a valid recording
//! of everything up to that moment, and [`repair`] recovers the rest that
//! reached the disk.
//!
//! A microphone that goes away mid-meeting does not end the meeting. Closing
//! a laptop's lid cuts its built-in microphone off for a few seconds, and a
//! headset can drop. The session keeps the file open and opens the same
//! microphone, or else the system's default one, again every second and a
//! half; when sound comes in again it is written straight after what came
//! before, and the stretch that was not heard is kept as a [`Gap`]. Only
//! after ten minutes without a microphone does it give up
//! ([`Event::GaveUp`]).

use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{JoinHandle, ThreadId};
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, bail, Context, Result};
use handy_recorder::{AudioChunk, Channels, EndReason, Recorder, RecorderConfig, Sink};
use hound::{SampleFormat, WavSpec, WavWriter};

pub use super::store::Gap;

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

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// How a session looks for a microphone that went away.
#[derive(Debug, Clone, Copy)]
pub struct Reconnect {
    /// Between two tries to open a microphone.
    pub retry_every: Duration,
    /// Without any microphone this long, the session gives up.
    pub give_up_after: Duration,
}

impl Default for Reconnect {
    fn default() -> Self {
        Self {
            retry_every: Duration::from_millis(1_500),
            give_up_after: Duration::from_secs(10 * 60),
        }
    }
}

/// What happened to the microphone, told as it happens, on the session's own
/// thread. Do the work elsewhere: [`Session::stop`] waits for that thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The microphone went away (`reason` is the system's). The session is
    /// trying to open it again; the recording stays open.
    Lost { reason: String },
    /// Sound is coming in again, from `device`, after `gap`.
    Back { gap: Gap, device: String },
    /// No microphone came back in time. Call [`Session::stop`] to complete
    /// the file with what was recorded.
    GaveUp { reason: String },
    /// The broken microphone did not give the recording back, so nothing
    /// more can be written to it. Call [`Session::stop`]: the file is made
    /// whole from what reached the disk.
    Broken { reason: String },
}

/// What wakes the session's thread.
#[derive(Default)]
struct Signal {
    /// A microphone failed: which one (see `Inner::current`) and why.
    failed: Option<(u64, String)>,
    /// Sound came back after this gap.
    back: Option<Gap>,
    stopping: bool,
}

/// What the recording thread, the session's thread and the app share.
#[derive(Default)]
struct Shared {
    frames: AtomicU64,
    /// The loudest sample of the latest piece, as `f32` bits.
    level: AtomicU32,
    /// Pieces in a row that were exactly silent.
    dead_chunks: AtomicU32,
    failed: AtomicBool,
    write_error: Mutex<Option<String>>,
    /// The microphone is away and being looked for.
    reconnecting: AtomicBool,
    /// The broken microphone has been let go of: the next sound is from a
    /// new one, so it is the microphone's return. Until then a late piece
    /// from the broken one is only written, not taken for a return.
    awaiting_return: AtomicBool,
    /// When it went away, while it is, by the wall clock.
    lost_since: Mutex<Option<SystemTime>>,
    /// Every stretch it was away and came back from.
    gaps: Mutex<Vec<Gap>>,
    signal: Mutex<Signal>,
    wake: Condvar,
    clock: Clock,
}

/// What time it is. The wall clock, because a gap is mostly the computer
/// asleep with its lid shut, and `Instant` does not count sleep on macOS;
/// a test's own clock otherwise.
#[derive(Clone)]
struct Clock(Arc<dyn Fn() -> SystemTime + Send + Sync>);

impl Default for Clock {
    fn default() -> Self {
        Self(Arc::new(SystemTime::now))
    }
}

impl Clock {
    fn now(&self) -> SystemTime {
        (self.0)()
    }

    /// Time since `then`; nothing when the clock was set back.
    fn since(&self, then: SystemTime) -> Duration {
        self.now().duration_since(then).unwrap_or_default()
    }
}

impl Shared {
    fn with_clock(clock: Clock) -> Self {
        Self {
            clock,
            ..Self::default()
        }
    }

    fn elapsed_ms(&self) -> u64 {
        self.frames.load(Ordering::Acquire) * 1000 / u64::from(RATE)
    }

    /// A microphone reports that it failed.
    fn report_failure(&self, which: u64, reason: String) {
        lock(&self.signal).failed = Some((which, reason));
        self.wake.notify_all();
    }

    /// The first sound from a microphone opened after one went away.
    fn came_back(&self) {
        if !self.awaiting_return.swap(false, Ordering::AcqRel) {
            return;
        }
        let Some(since) = lock(&self.lost_since).take() else {
            return;
        };
        let gap = Gap {
            at_ms: self.elapsed_ms(),
            gap_ms: self.clock.since(since).as_millis() as u64,
        };
        lock(&self.gaps).push(gap);
        self.reconnecting.store(false, Ordering::Release);
        lock(&self.signal).back = Some(gap);
        self.wake.notify_all();
    }
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
        *lock(&self.shared.write_error) = Some(error.to_string());
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
        if self.shared.awaiting_return.load(Ordering::Acquire) {
            self.shared.came_back();
        }
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

/// What a failed microphone is reported with.
type OnFailure = Box<dyn FnOnce(String) + Send + 'static>;

/// Opens a microphone: the system's, or a test's fake one.
trait Opener: Send + Sync + 'static {
    /// `device` is an id from [`microphones`]; `None` for the default.
    fn open(&self, device: Option<&str>, on_failure: OnFailure) -> Result<Recorder<WavSink>>;
}

fn config(device: Option<&str>) -> RecorderConfig {
    RecorderConfig {
        device: device.map(str::to_string),
        sample_rate: Some(RATE),
        channels: Channels::MixToMono,
        frames_per_chunk: Some(CHUNK_FRAMES),
        take_headset: false,
    }
}

/// The computer's microphones.
struct SystemMicrophones;

impl Opener for SystemMicrophones {
    fn open(&self, device: Option<&str>, on_failure: OnFailure) -> Result<Recorder<WavSink>> {
        Recorder::open_with_failure_handler(config(device), move |error| {
            on_failure(error.to_string())
        })
        .map_err(|e| anyhow!("{e}"))
    }
}

/// Which recorder holds the file.
enum Live {
    Running(Box<Recorder<WavSink>>),
    /// No microphone: the file waits for one.
    Waiting(WavSink),
    /// The file was lost with a microphone that could not give it back.
    Gone,
}

struct Inner {
    shared: Arc<Shared>,
    opener: Box<dyn Opener>,
    /// The microphone the user chose; `None` for the default.
    chosen: Option<String>,
    /// The microphone's name, as the system gives it.
    device: Mutex<String>,
    live: Mutex<Live>,
    /// Which microphone is the current one: a failure reported by an older
    /// one is not this one's.
    current: AtomicU64,
    /// Why the microphone went away last.
    last_loss: Mutex<String>,
    timing: Reconnect,
    events: Box<dyn Fn(Event) + Send + Sync + 'static>,
}

impl Inner {
    /// Opens a microphone (`device`) for the recording, as microphone number
    /// `which`.
    fn open(&self, which: u64, device: Option<&str>) -> Result<Recorder<WavSink>> {
        let shared = Arc::downgrade(&self.shared);
        self.opener.open(
            device,
            Box::new(move |reason| {
                if let Some(shared) = shared.upgrade() {
                    shared.report_failure(which, reason);
                }
            }),
        )
    }

    /// Tries the chosen microphone, then the default one, and starts
    /// writing to `sink` from the first that opens. Hands `sink` back when
    /// none does.
    fn reopen(&self, mut sink: WavSink) -> std::result::Result<(), WavSink> {
        let mut tries = vec![self.chosen.as_deref()];
        if self.chosen.is_some() {
            tries.push(None);
        }
        for device in tries {
            let which = self.current.fetch_add(1, Ordering::AcqRel) + 1;
            let recorder = match self.open(which, device) {
                Ok(recorder) => recorder,
                Err(e) => {
                    log::debug!("The microphone does not open yet: {e:#}");
                    continue;
                }
            };
            let name = recorder.info().device.name.clone();
            match recorder.start(sink) {
                Ok(()) => {
                    *lock(&self.device) = name;
                    *lock(&self.live) = Live::Running(Box::new(recorder));
                    return Ok(());
                }
                Err(e) => {
                    log::debug!("The microphone opened but does not record yet: {e}");
                    sink = e.sink;
                }
            }
        }
        Err(sink)
    }

    fn stopping(&self) -> bool {
        lock(&self.shared.signal).stopping
    }

    /// Waits `time`, or less when the session is being stopped.
    fn pause(&self, time: Duration) {
        let signal = lock(&self.shared.signal);
        let _ = self
            .shared
            .wake
            .wait_timeout_while(signal, time, |signal| !signal.stopping);
    }

    /// The microphone failed: looks for one until sound can come in again
    /// (true), or until the session is stopped or gives up (false).
    fn reconnect(&self, reason: String) -> bool {
        if !self.shared.reconnecting.swap(true, Ordering::AcqRel) {
            // A new outage; one whose reopened microphone failed again
            // before any sound is the same one.
            *lock(&self.shared.lost_since) = Some(self.shared.clock.now());
            *lock(&self.last_loss) = reason.clone();
            self.shared.level.store(0, Ordering::Release);
            log::warn!("The microphone went away mid-recording: {reason}");
            (self.events)(Event::Lost {
                reason: reason.clone(),
            });
        }
        let since = lock(&self.shared.lost_since).unwrap_or_else(|| self.shared.clock.now());
        let held = std::mem::replace(&mut *lock(&self.live), Live::Gone);
        let mut sink = match held {
            // Everything it heard before failing is written by now.
            Live::Running(recorder) => match recorder.stop() {
                Ok(stopped) => stopped.sink,
                Err(e) => {
                    log::error!("The recording could not be taken back from the microphone: {e}");
                    (self.events)(Event::Broken {
                        reason: format!("{reason}; the recording could not be kept: {e}"),
                    });
                    return false;
                }
            },
            Live::Waiting(sink) => sink,
            Live::Gone => return false,
        };
        // The broken microphone is let go of: whatever sound comes next is
        // from one opened from here on.
        self.shared.awaiting_return.store(true, Ordering::Release);
        loop {
            if self.stopping() {
                *lock(&self.live) = Live::Waiting(sink);
                return false;
            }
            if self.shared.clock.since(since) >= self.timing.give_up_after {
                *lock(&self.live) = Live::Waiting(sink);
                log::warn!(
                    "No microphone for {} s; the recording ends",
                    self.timing.give_up_after.as_secs()
                );
                (self.events)(Event::GaveUp { reason });
                return false;
            }
            match self.reopen(sink) {
                Ok(()) => return true,
                Err(back) => sink = back,
            }
            self.pause(self.timing.retry_every);
        }
    }

    /// The session's thread: waits for a microphone to fail or come back.
    fn watch(&self) {
        enum Woke {
            Failed(String),
            Back(Gap),
        }
        loop {
            let woke = {
                let mut signal = lock(&self.shared.signal);
                loop {
                    if signal.stopping {
                        return;
                    }
                    if let Some(gap) = signal.back.take() {
                        break Woke::Back(gap);
                    }
                    if let Some((which, reason)) = signal.failed.take() {
                        if which == self.current.load(Ordering::Acquire) {
                            break Woke::Failed(reason);
                        }
                        // An older microphone, already let go of.
                        continue;
                    }
                    signal = self
                        .shared
                        .wake
                        .wait(signal)
                        .unwrap_or_else(|e| e.into_inner());
                }
            };
            match woke {
                Woke::Back(gap) => {
                    let device = lock(&self.device).clone();
                    log::info!(
                        "The microphone is back ({device}) after {} ms, {} ms in",
                        gap.gap_ms,
                        gap.at_ms
                    );
                    (self.events)(Event::Back { gap, device });
                }
                Woke::Failed(reason) => {
                    if !self.reconnect(reason) {
                        return;
                    }
                }
            }
        }
    }
}

/// A recording in progress.
pub struct Session {
    inner: Arc<Inner>,
    watcher: Option<JoinHandle<()>>,
    watcher_id: Option<ThreadId>,
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
    /// Where the microphone was away, in the order it happened. One it had
    /// not come back from when the recording ended is the last.
    pub gaps: Vec<Gap>,
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
    /// `events` hears, on the session's own thread, when the microphone goes
    /// away mid-recording (unplugged, a lid closed, the computer slept),
    /// when it comes back, and when the session gives up looking for one.
    /// The recording up to then is always on disk.
    pub fn start(
        path: &Path,
        device: Option<String>,
        events: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Self> {
        Self::start_with(
            path,
            device,
            Box::new(SystemMicrophones),
            Reconnect::default(),
            Clock::default(),
            Box::new(events),
        )
    }

    fn start_with(
        path: &Path,
        device: Option<String>,
        opener: Box<dyn Opener>,
        timing: Reconnect,
        clock: Clock,
        events: Box<dyn Fn(Event) + Send + Sync + 'static>,
    ) -> Result<Self> {
        let inner = Arc::new(Inner {
            shared: Arc::new(Shared::with_clock(clock)),
            opener,
            chosen: device,
            device: Mutex::new(String::new()),
            live: Mutex::new(Live::Gone),
            current: AtomicU64::new(1),
            last_loss: Mutex::new(String::new()),
            timing,
            events,
        });
        let recorder = inner
            .open(1, inner.chosen.as_deref())
            .context("could not open the microphone")?;
        *lock(&inner.device) = recorder.info().device.name.clone();

        let sink = WavSink::create(path, inner.shared.clone())?;
        recorder
            .start(sink)
            .map_err(|e| anyhow!("{e}"))
            .context("could not start recording")?;
        *lock(&inner.live) = Live::Running(Box::new(recorder));

        let watching = inner.clone();
        let watcher = std::thread::Builder::new()
            .name("seogi-recording".into())
            .spawn(move || watching.watch())
            .context("could not start watching the microphone")?;
        Ok(Self {
            inner,
            watcher_id: Some(watcher.thread().id()),
            watcher: Some(watcher),
            path: path.to_path_buf(),
        })
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.inner.shared.elapsed_ms()
    }

    /// The microphone's name, as the system gives it. It may change when
    /// the session had to fall back to the default microphone.
    pub fn device(&self) -> String {
        lock(&self.inner.device).clone()
    }

    /// How loud the latest tenth of a second was, 0 to 1.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.inner.shared.level.load(Ordering::Acquire)).clamp(0.0, 1.0)
    }

    /// No sound at all is arriving: the microphone is muted, or the system
    /// has not let this app use it.
    pub fn is_dead(&self) -> bool {
        !self.is_reconnecting()
            && self.inner.shared.dead_chunks.load(Ordering::Acquire) >= DEAD_AFTER_CHUNKS
    }

    /// The microphone went away and is being looked for.
    pub fn is_reconnecting(&self) -> bool {
        self.inner.shared.reconnecting.load(Ordering::Acquire)
    }

    /// Where the microphone was away and came back so far.
    pub fn gaps(&self) -> Vec<Gap> {
        lock(&self.inner.shared.gaps).clone()
    }

    /// [`Session::gaps`], and while the microphone is away the stretch it
    /// is away for, with no length yet (`gap_ms` 0): what a meeting keeps
    /// so that a crash meanwhile still knows where it went.
    pub fn gaps_so_far(&self) -> Vec<Gap> {
        let mut gaps = self.gaps();
        if self.is_reconnecting() {
            gaps.push(Gap {
                at_ms: self.elapsed_ms(),
                gap_ms: 0,
            });
        }
        gaps
    }

    /// The file could not be written.
    pub fn has_failed(&self) -> bool {
        self.inner.shared.failed.load(Ordering::Acquire)
    }

    /// Stops the session's thread; it ends at once unless it is trying to
    /// open a microphone right now.
    fn stop_watching(&mut self) {
        lock(&self.inner.shared.signal).stopping = true;
        self.inner.shared.wake.notify_all();
        if let Some(watcher) = self.watcher.take() {
            // Its own events must not wait for it.
            if Some(std::thread::current().id()) != self.watcher_id {
                let _ = watcher.join();
            }
        }
    }

    /// Ends the recording and completes the file.
    pub fn stop(mut self) -> Result<Finished> {
        self.stop_watching();
        let shared = self.inner.shared.clone();
        let held = std::mem::replace(&mut *lock(&self.inner.live), Live::Gone);
        let (sink, dropped_frames, mut trouble) = match held {
            Live::Running(recorder) => {
                let stopped = recorder
                    .stop()
                    .map_err(|e| anyhow!("{e}"))
                    .context("could not stop the recording")?;
                let trouble = match &stopped.end_reason {
                    EndReason::StopCalled => None,
                    EndReason::RecorderFailed(error) => Some(error.to_string()),
                    EndReason::SinkPanicked(message) => Some(message.clone()),
                };
                (stopped.sink, stopped.dropped_frames, trouble)
            }
            Live::Waiting(sink) => (sink, 0, None),
            Live::Gone => bail!("the recording was lost with the microphone"),
        };
        let mut gaps = lock(&shared.gaps).clone();
        // Ended while the microphone was away: that stretch is the last gap.
        if shared.reconnecting.load(Ordering::Acquire) {
            if let Some(since) = lock(&shared.lost_since).take() {
                gaps.push(Gap {
                    at_ms: shared.elapsed_ms(),
                    gap_ms: (shared.clock.since(since).as_millis() as u64).max(1),
                });
            }
            trouble = Some(lock(&self.inner.last_loss).clone());
        }
        if let Some(error) = lock(&shared.write_error).take() {
            trouble = Some(error);
        }
        let frames = sink.finish()?;
        Ok(Finished {
            frames,
            dropped_frames,
            trouble,
            gaps,
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop_watching();
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
    use handy_recorder::testing::{FakeError, FakeMic};
    use std::sync::mpsc;
    use std::time::Instant;

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

    // ----- a microphone that goes away and comes back ----------------------

    /// The fake microphone: a test plays its callbacks, stops them, makes
    /// the device go away and opens fail, and lets it come back.
    struct Fake {
        mic: FakeMic,
        /// Opens that are to fail, as a closed lid's would.
        away: Arc<AtomicBool>,
        opens: Arc<AtomicU32>,
    }

    impl Opener for Fake {
        fn open(&self, device: Option<&str>, on_failure: OnFailure) -> Result<Recorder<WavSink>> {
            self.opens.fetch_add(1, Ordering::AcqRel);
            if self.away.load(Ordering::Acquire) {
                self.mic.fail_next_open(FakeError::DeviceNotAvailable);
            }
            self.mic
                .open(
                    config(device),
                    Some(Box::new(move |error| on_failure(error.to_string()))),
                    Duration::from_secs(2),
                )
                .map_err(|e| anyhow!("{e}"))
        }
    }

    struct Rig {
        mic: FakeMic,
        away: Arc<AtomicBool>,
        opens: Arc<AtomicU32>,
        events: mpsc::Receiver<Event>,
        session: Session,
        _dir: tempfile::TempDir,
    }

    const QUICK: Reconnect = Reconnect {
        retry_every: Duration::from_millis(20),
        give_up_after: Duration::from_secs(30),
    };

    fn rig(device: Option<&str>, timing: Reconnect) -> Rig {
        rig_with_clock(device, timing, Clock::default())
    }

    /// A clock a test moves by hand, as the time a computer sleeps passes
    /// without any real time passing for the test.
    #[derive(Clone)]
    struct HandClock(Arc<Mutex<SystemTime>>);

    impl HandClock {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(
                SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
            )))
        }

        fn clock(&self) -> Clock {
            let now = self.0.clone();
            Clock(Arc::new(move || *lock(&now)))
        }

        fn advance(&self, time: Duration) {
            *lock(&self.0) += time;
        }
    }

    fn rig_with_clock(device: Option<&str>, timing: Reconnect, clock: Clock) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let mic = FakeMic::new(RATE, 1);
        let away = Arc::new(AtomicBool::new(false));
        let opens = Arc::new(AtomicU32::new(0));
        let (send, events) = mpsc::channel();
        let send = Mutex::new(send);
        let session = Session::start_with(
            &dir.path().join("녹음.wav"),
            device.map(str::to_string),
            Box::new(Fake {
                mic: mic.clone(),
                away: away.clone(),
                opens: opens.clone(),
            }),
            timing,
            clock,
            Box::new(move |event| {
                let _ = lock(&send).send(event);
            }),
        )
        .unwrap();
        Rig {
            mic,
            away,
            opens,
            events,
            session,
            _dir: dir,
        }
    }

    impl Rig {
        /// Plays `chunks` tenths of a second of sound through the callback.
        fn talk(&self, chunks: usize, value: f32) {
            for _ in 0..chunks {
                assert!(self.mic.push(&vec![value; CHUNK_FRAMES]));
            }
        }

        /// The callbacks stop and the system says the device is gone, as
        /// when a lid closes; opening it fails until [`Rig::lid_open`].
        fn lid_closed(&self) {
            self.away.store(true, Ordering::Release);
            assert!(self.mic.report_error(FakeError::DeviceNotAvailable));
        }

        fn lid_open(&self) {
            self.away.store(false, Ordering::Release);
        }

        fn next(&self) -> Event {
            self.events
                .recv_timeout(Duration::from_secs(5))
                .expect("an event")
        }

        /// Which microphone is the current one.
        fn which(&self) -> u64 {
            self.session.inner.current.load(Ordering::Acquire)
        }

        /// Waits until a microphone newer than number `before` records into
        /// the file. (Newer ones are only tried once the failed one was let
        /// go of.)
        fn wait_until_reopened(&self, before: u64) {
            let until = Instant::now() + Duration::from_secs(5);
            loop {
                let reopened = self.which() > before
                    && matches!(*lock(&self.session.inner.live), Live::Running(_))
                    && self.mic.is_streaming();
                if reopened {
                    return;
                }
                assert!(
                    Instant::now() < until,
                    "the microphone was not opened again"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        /// Waits until the recording holds `frames`.
        fn wait_for_frames(&self, frames: u64) {
            let until = Instant::now() + Duration::from_secs(5);
            while self.session.inner.shared.frames.load(Ordering::Acquire) < frames {
                assert!(Instant::now() < until, "the sound was not written");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    fn samples(path: &Path) -> Vec<i16> {
        hound::WavReader::open(path)
            .unwrap()
            .samples::<i16>()
            .map(Result::unwrap)
            .collect()
    }

    fn level(value: f32) -> i16 {
        (value * f32::from(i16::MAX)) as i16
    }

    /// Stops the session. A real microphone's callback keeps running while
    /// it stops; the fake's runs only when pushed, so empty blocks stand in
    /// for it (they add no sound).
    fn stop(mic: &FakeMic, session: Session) -> Finished {
        std::thread::scope(|scope| {
            let stopping = scope.spawn(move || session.stop());
            while !stopping.is_finished() {
                mic.push(&[]);
                std::thread::sleep(Duration::from_millis(2));
            }
            stopping.join().unwrap().unwrap()
        })
    }

    #[test]
    fn a_lid_closed_for_a_moment_keeps_the_meeting_and_one_file() {
        let rig = rig(None, QUICK);
        rig.talk(10, 0.5);
        rig.wait_for_frames(10 * CHUNK_FRAMES as u64);

        let before = rig.which();
        rig.lid_closed();
        assert!(matches!(rig.next(), Event::Lost { .. }));
        assert!(rig.session.is_reconnecting());
        assert!(!rig.session.is_dead(), "away is not muted");
        // A few tries fail while the lid is shut; none of them ends it.
        let until = Instant::now() + Duration::from_secs(5);
        while rig.opens.load(Ordering::Acquire) < 4 {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(rig.session.is_reconnecting());

        rig.lid_open();
        rig.wait_until_reopened(before);
        rig.talk(5, 0.25);
        let Event::Back { gap, device } = rig.next() else {
            panic!("the microphone should be back");
        };
        assert_eq!(gap.at_ms, 1_000, "the gap is where the sound stopped");
        assert!(gap.gap_ms > 0);
        assert_eq!(device, "Fake Mic");
        assert!(!rig.session.is_reconnecting());
        rig.wait_for_frames(15 * CHUNK_FRAMES as u64);

        let path = rig.session.path.clone();
        let finished = stop(&rig.mic, rig.session);
        assert_eq!(finished.trouble, None);
        assert_eq!(finished.gaps, vec![gap]);
        // The second stretch follows the first in the same file.
        let read = samples(&path);
        assert_eq!(read.len(), 15 * CHUNK_FRAMES);
        assert!(read[..10 * CHUNK_FRAMES].iter().all(|&s| s == level(0.5)));
        assert!(read[10 * CHUNK_FRAMES..].iter().all(|&s| s == level(0.25)));
    }

    #[test]
    fn a_lid_closed_twice_is_two_gaps() {
        let rig = rig(None, QUICK);
        let chunk = CHUNK_FRAMES as u64;
        for round in 0..2u64 {
            // A second of talk, the lid shut a moment, a tenth of talk.
            rig.talk(10, 0.5);
            rig.wait_for_frames((11 * round + 10) * chunk);
            let before = rig.which();
            rig.lid_closed();
            assert!(matches!(rig.next(), Event::Lost { .. }));
            std::thread::sleep(Duration::from_millis(50));
            rig.lid_open();
            rig.wait_until_reopened(before);
            rig.talk(1, 0.5);
            assert!(matches!(rig.next(), Event::Back { .. }));
            rig.wait_for_frames((11 * round + 11) * chunk);
        }
        let finished = stop(&rig.mic, rig.session);
        assert_eq!(finished.gaps.len(), 2);
        assert_eq!(finished.gaps[0].at_ms, 1_000);
        assert_eq!(finished.gaps[1].at_ms, 2_100);
        assert_eq!(finished.frames, 22 * CHUNK_FRAMES as u64);
    }

    #[test]
    fn a_chosen_microphone_that_stays_away_gives_way_to_the_default() {
        // The fake has one device; a chosen id it does not know is a headset
        // that is gone for good, and the default is the computer's own.
        let dir = tempfile::tempdir().unwrap();
        let mic = FakeMic::new(RATE, 1);
        // The chosen one opened at the start; afterwards only the default.
        struct Chosen {
            mic: FakeMic,
            first: AtomicBool,
            asked: Mutex<Vec<Option<String>>>,
        }
        impl Opener for Chosen {
            fn open(
                &self,
                device: Option<&str>,
                on_failure: OnFailure,
            ) -> Result<Recorder<WavSink>> {
                lock(&self.asked).push(device.map(str::to_string));
                let first = self.first.swap(false, Ordering::AcqRel);
                let device = if first { None } else { device };
                self.mic
                    .open(
                        config(device),
                        Some(Box::new(move |error| on_failure(error.to_string()))),
                        Duration::from_secs(2),
                    )
                    .map_err(|e| anyhow!("{e}"))
            }
        }
        let opener = Arc::new(Chosen {
            mic: mic.clone(),
            first: AtomicBool::new(true),
            asked: Mutex::new(Vec::new()),
        });
        struct Same(Arc<Chosen>);
        impl Opener for Same {
            fn open(
                &self,
                device: Option<&str>,
                on_failure: OnFailure,
            ) -> Result<Recorder<WavSink>> {
                self.0.open(device, on_failure)
            }
        }
        let (send, events) = mpsc::channel();
        let send = Mutex::new(send);
        let session = Session::start_with(
            &dir.path().join("녹음.wav"),
            Some("headset".to_string()),
            Box::new(Same(opener.clone())),
            QUICK,
            Clock::default(),
            Box::new(move |event| {
                let _ = lock(&send).send(event);
            }),
        )
        .unwrap();
        mic.push(&vec![0.5; CHUNK_FRAMES]);
        assert!(mic.report_error(FakeError::DeviceNotAvailable));
        let until = Instant::now() + Duration::from_secs(5);
        while !(session.inner.current.load(Ordering::Acquire) > 1
            && matches!(*lock(&session.inner.live), Live::Running(_))
            && mic.is_streaming())
        {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(5));
        }
        mic.push(&vec![0.5; CHUNK_FRAMES]);
        let mut back = false;
        while let Ok(event) = events.recv_timeout(Duration::from_secs(5)) {
            if matches!(event, Event::Back { .. }) {
                back = true;
                break;
            }
        }
        assert!(back);
        let asked = lock(&opener.asked).clone();
        assert_eq!(
            asked[1..3],
            [Some("headset".to_string()), None],
            "the chosen one first, then the default"
        );
        assert_eq!(stop(&mic, session).gaps.len(), 1);
    }

    #[test]
    fn stopping_while_the_microphone_is_away_keeps_what_was_recorded() {
        let rig = rig(None, QUICK);
        rig.talk(10, 0.5);
        rig.wait_for_frames(10 * CHUNK_FRAMES as u64);
        rig.lid_closed();
        assert!(matches!(rig.next(), Event::Lost { .. }));

        let path = rig.session.path.clone();
        let finished = stop(&rig.mic, rig.session);
        assert_eq!(finished.frames, 10 * CHUNK_FRAMES as u64);
        assert!(finished.trouble.is_some());
        // The stretch it ended in is the last gap.
        assert_eq!(finished.gaps.len(), 1);
        assert_eq!(finished.gaps[0].at_ms, 1_000);
        assert_eq!(samples(&path).len(), 10 * CHUNK_FRAMES);
    }

    #[test]
    fn a_microphone_that_never_comes_back_is_given_up_on() {
        let rig = rig(
            None,
            Reconnect {
                retry_every: Duration::from_millis(10),
                give_up_after: Duration::from_millis(200),
            },
        );
        rig.talk(10, 0.5);
        rig.wait_for_frames(10 * CHUNK_FRAMES as u64);
        rig.lid_closed();
        assert!(matches!(rig.next(), Event::Lost { .. }));
        assert!(matches!(rig.next(), Event::GaveUp { .. }));

        let path = rig.session.path.clone();
        let finished = stop(&rig.mic, rig.session);
        assert_eq!(finished.frames, 10 * CHUNK_FRAMES as u64);
        assert!(finished.trouble.is_some());
        assert_eq!(samples(&path).len(), 10 * CHUNK_FRAMES);
    }

    #[test]
    fn stopping_a_healthy_recording_is_as_before() {
        let rig = rig(None, QUICK);
        rig.talk(3, 0.5);
        rig.wait_for_frames(3 * CHUNK_FRAMES as u64);
        let finished = stop(&rig.mic, rig.session);
        assert_eq!(finished.frames, 3 * CHUNK_FRAMES as u64);
        assert_eq!(finished.trouble, None);
        assert!(finished.gaps.is_empty());
        assert!(rig.events.try_recv().is_err(), "nothing went away");
    }

    #[test]
    fn the_time_the_computer_slept_counts_in_the_gap() {
        // The lid shut for three minutes of sleep: hardly any time passes
        // for the test, the wall clock moves on three minutes.
        let hand = HandClock::new();
        let rig = rig_with_clock(
            None,
            Reconnect {
                retry_every: Duration::from_millis(20),
                give_up_after: Duration::from_secs(600),
            },
            hand.clock(),
        );
        rig.talk(10, 0.5);
        rig.wait_for_frames(10 * CHUNK_FRAMES as u64);
        let before = rig.which();
        rig.lid_closed();
        assert!(matches!(rig.next(), Event::Lost { .. }));
        hand.advance(Duration::from_secs(180));
        rig.lid_open();
        rig.wait_until_reopened(before);
        rig.talk(1, 0.5);
        let Event::Back { gap, .. } = rig.next() else {
            panic!("the microphone should be back");
        };
        assert!(
            (180_000..181_000).contains(&gap.gap_ms),
            "the sleep is in the gap: {} ms",
            gap.gap_ms
        );
        stop(&rig.mic, rig.session);
    }

    #[test]
    fn ten_minutes_away_are_counted_by_the_wall_clock_too() {
        let hand = HandClock::new();
        let rig = rig_with_clock(
            None,
            Reconnect {
                retry_every: Duration::from_millis(10),
                give_up_after: Duration::from_secs(600),
            },
            hand.clock(),
        );
        rig.talk(10, 0.5);
        rig.wait_for_frames(10 * CHUNK_FRAMES as u64);
        rig.lid_closed();
        assert!(matches!(rig.next(), Event::Lost { .. }));
        hand.advance(Duration::from_secs(9 * 60));
        assert!(
            rig.events.recv_timeout(Duration::from_millis(200)).is_err(),
            "nine minutes are not ten"
        );
        hand.advance(Duration::from_secs(2 * 60));
        assert!(matches!(rig.next(), Event::GaveUp { .. }));
        let finished = stop(&rig.mic, rig.session);
        assert_eq!(finished.gaps.len(), 1);
        assert!(finished.gaps[0].gap_ms >= 660_000);
    }

    #[test]
    fn a_late_sound_from_the_broken_microphone_is_not_its_return() {
        let dir = tempfile::tempdir().unwrap();
        let shared = Arc::new(Shared::default());
        let mut sink = WavSink::create(&dir.path().join("a.wav"), shared.clone()).unwrap();
        feed(&mut sink, 10, 0.5);
        // The microphone failed; the session marks it away, and a piece
        // the broken one had already heard arrives before it is let go of.
        shared.reconnecting.store(true, Ordering::Release);
        *lock(&shared.lost_since) = Some(shared.clock.now());
        feed(&mut sink, 1, 0.5);
        assert!(lock(&shared.gaps).is_empty(), "no return yet");
        assert!(lock(&shared.signal).back.is_none());
        assert!(shared.reconnecting.load(Ordering::Acquire));
        // Let go of; the next piece is from the microphone opened again.
        shared.awaiting_return.store(true, Ordering::Release);
        feed(&mut sink, 1, 0.5);
        let gaps = lock(&shared.gaps).clone();
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].at_ms, 1_100, "after the late piece, which is kept");
        assert!(!shared.reconnecting.load(Ordering::Acquire));
        // Later sound is just sound.
        feed(&mut sink, 1, 0.5);
        assert_eq!(lock(&shared.gaps).len(), 1);
    }

    #[test]
    fn a_gap_of_unknown_length_says_so() {
        let open = Gap {
            at_ms: 1_000,
            gap_ms: 0,
        };
        assert_eq!(open.note(), "(끊김)");
        let known = Gap {
            at_ms: 1_000,
            gap_ms: 12_000,
        };
        assert_eq!(known.note(), "(12초 끊김)");
    }

    #[test]
    fn a_gap_is_said_in_seconds_minutes_and_hours() {
        let gap = |ms| Gap {
            at_ms: 0,
            gap_ms: ms,
        };
        assert_eq!(gap(12_000).words(), "12초");
        assert_eq!(gap(400).words(), "1초");
        assert_eq!(gap(185_000).words(), "3분 5초");
        assert_eq!(gap(120_000).words(), "2분");
        assert_eq!(gap(3_720_000).words(), "1시간 2분");
    }
}
