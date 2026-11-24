//! What Seogi's recording pipeline needs from the engine: a model held for
//! the length of a job, and one run per chunk with the job's own options.

use anyhow::{anyhow, Result};
use log::info;
use transcribe_cpp::{RunOptions, TimestampKind};

use super::{echoes_vocabulary, TranscriptionManager};
use crate::engine_supervisor::{EngineError, RunOutput};
use crate::settings::get_settings;

/// How long a recording job waits for a dictation in progress before it
/// loads its own model anyway. A dictation is seconds long; this only stops a
/// stuck one from holding the queue.
const DICTATION_WAIT: std::time::Duration = std::time::Duration::from_secs(120);

/// The model a recording is transcribed with, and what it can do.
#[derive(Debug, Clone)]
pub struct JobModel {
    pub id: String,
    /// The model family: `whisper`, `qwen3_asr`, ...
    pub arch: String,
    /// Custom terms reach the model as a prompt.
    pub supports_vocabulary: bool,
    /// The finest timestamps the model returns for a run; `None` when it
    /// returns text only.
    pub timestamps: TimestampKind,
    /// Language codes the model accepts as a hint; empty when it takes none.
    pub languages: Vec<String>,
}

impl TranscriptionManager {
    /// Loads `model_id`, or the selected model when `None`, and holds it
    /// until [`end_recording_job`](Self::end_recording_job): nothing may
    /// switch or unload the model in between.
    pub fn begin_recording_job(&self, model_id: Option<&str>) -> Result<JobModel> {
        let selected = get_settings(&self.app_handle).selected_model;
        let wanted = model_id
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or(selected);
        if wanted.is_empty() {
            return Err(anyhow!("No transcription model is selected"));
        }

        self.set_job_active(true);
        let loaded = self.load_for_job(&wanted);
        if loaded.is_err() {
            self.set_job_active(false);
        }
        loaded
    }

    fn load_for_job(&self, wanted: &str) -> Result<JobModel> {
        // A dictation being recorded or transcribed finishes first on the
        // model it started with: swapping the model under it could lose its
        // words. Dictations that start from here on use the job's model.
        if self.get_current_model().as_deref() != Some(wanted) {
            let started = std::time::Instant::now();
            while self.dictation_in_flight() && started.elapsed() < DICTATION_WAIT {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            if started.elapsed() > std::time::Duration::from_millis(200) {
                info!(
                    "The recording job waited {:.1}s for a dictation to finish",
                    started.elapsed().as_secs_f64()
                );
            }
        }
        // A load already under way finishes first, so two loads never race.
        {
            let mut is_loading = self.is_loading.lock().unwrap();
            while *is_loading {
                is_loading = self.loading_condvar.wait(is_loading).unwrap();
            }
        }
        if !self.is_model_loaded() || self.get_current_model().as_deref() != Some(wanted) {
            info!("Loading '{}' for a recording job", wanted);
            self.load_model(wanted)?;
        }
        let info = self
            .engine
            .loaded()
            .ok_or_else(|| anyhow!("The transcription model did not stay loaded"))?;
        self.touch_activity();
        Ok(JobModel {
            id: wanted.to_string(),
            arch: info.arch,
            supports_vocabulary: info.supports_vocabulary,
            timestamps: info.capabilities.max_timestamp_kind,
            languages: info.capabilities.languages,
        })
    }

    /// One chunk of a recording job. A dictation's cancel stops only the
    /// chunk it caught; the caller decides whether to try it again.
    pub fn run_recording_chunk(
        &self,
        pcm: Vec<f32>,
        options: &RunOptions,
        model: &JobModel,
    ) -> Result<RunOutput, EngineError> {
        let mark = self.engine.cancel_mark();
        self.touch_activity();
        self.engine
            .transcribe_traced_since(mark, pcm, options.clone())
            .inspect_err(|e| {
                if !matches!(e, EngineError::Cancelled) {
                    self.forget_model_if_engine_dropped(&model.id, &e.to_string());
                }
            })
            .map(|mut output| {
                // A model prompted with terms can answer near-silence by
                // reading the terms back.
                if echoes_vocabulary(&output.transcript.text, &options.vocabulary) {
                    info!("The model returned the hint terms, not speech; dropping them");
                    output.transcript.text.clear();
                    output.transcript.segments.clear();
                }
                output
            })
    }

    /// Releases the model held by [`begin_recording_job`](Self::begin_recording_job).
    /// If the job ran on a model other than the selected one, the next
    /// dictation loads the selected model again.
    pub fn end_recording_job(&self, model: &JobModel) {
        self.set_job_active(false);
        if get_settings(&self.app_handle).selected_model != model.id {
            self.reload_model_on_next_use();
        }
        self.maybe_unload_immediately("recording job");
    }
}
