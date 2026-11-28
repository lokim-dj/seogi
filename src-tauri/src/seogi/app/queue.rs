//! Restarting, and the queue: transcribing, writing the minutes, trying
//! again later when a step fails.

use super::*;

impl Secretary {
    // ----- restarting -------------------------------------------------------

    /// What a crash or a restart left half done goes back in the queue.
    pub(super) fn recover(&self) {
        let unfinished: Vec<(PathBuf, Status)> = lock(&self.slots)
            .values()
            .filter(|slot| matches!(slot.summary.status, Status::Recording | Status::Working))
            .map(|slot| (slot.folder.clone(), slot.summary.status))
            .collect();
        for (folder, status) in unfinished {
            let Ok(mut meeting) = Library::read(&folder) else {
                continue;
            };
            let result = if status == Status::Recording {
                let path = folder.join(&meeting.audio_file);
                // Still being written: another copy of the app is recording.
                let fresh =
                    Look::at(&path).is_some_and(|look| now_ms() - look.modified_ms < 15_000);
                if fresh {
                    continue;
                }
                let frames = recorder::repair(&path).unwrap_or(0);
                meeting.duration_ms = frames * 1_000 / u64::from(recorder::RATE);
                info!(
                    "Recovering the recording {} ({} s)",
                    meeting.id,
                    meeting.duration_ms / 1_000
                );
                self.queue_recorded(
                    &mut meeting,
                    &folder,
                    Some((
                        "app_stopped",
                        "the app stopped while this was being recorded".to_string(),
                    )),
                )
            } else {
                meeting.status = if meeting.transcript.is_some() {
                    Status::Transcribed
                } else {
                    Status::Queued
                };
                meeting.stage.clear();
                meeting.percent = 0;
                self.save(&meeting, &folder)
            };
            if let Err(e) = result {
                warn!("Could not recover {}: {e:#}", folder.display());
            }
        }
    }

    // ----- the queue --------------------------------------------------------

    pub(super) fn wake(&self) {
        *lock(&self.wake.0) = true;
        self.wake.1.notify_all();
    }

    pub(super) fn work_loop(self: Arc<Self>) {
        loop {
            match self.next_job() {
                Some(id) => {
                    // The faint dot while minutes are being written.
                    if !self.quiet {
                        super::status::set_meeting_busy(&self.app, true);
                    }
                    if let Err(e) = self.process(&id) {
                        // The meeting's own record could not be read or
                        // written. Leave it be for a while, not in a spin.
                        error!("Could not work on {id}: {e:#}");
                        if let Some(slot) = lock(&self.slots).get_mut(&id) {
                            slot.retry_at_ms = now_ms() + 60_000;
                        }
                    }
                }
                None => {
                    // Nothing left to do, however the last job ended: the
                    // faint dot goes. Checked here rather than after each
                    // job, so a job that failed (its retry written only
                    // afterwards) cannot pass for the next one and leave the
                    // dot on. Setting it when already off does nothing.
                    if !self.quiet {
                        super::status::set_meeting_busy(&self.app, false);
                    }
                    let mut woken = lock(&self.wake.0);
                    if !*woken {
                        // Wakes on its own too: a retry may have come due.
                        woken = self
                            .wake
                            .1
                            .wait_timeout(woken, Duration::from_secs(15))
                            .map_or_else(|e| e.into_inner().0, |(guard, _)| guard);
                    }
                    *woken = false;
                }
            }
        }
    }

    /// The meeting to work on next: recordings waiting to be transcribed
    /// first, in the order they arrived, then minutes waiting to be retried.
    fn next_job(&self) -> Option<String> {
        let now = now_ms();
        lock(&self.slots)
            .iter()
            .filter(|(_, slot)| {
                matches!(slot.summary.status, Status::Queued | Status::Transcribed)
                    && !slot.parked
                    && slot.retry_at_ms <= now
            })
            .min_by_key(|(_, slot)| (slot.summary.status != Status::Queued, slot.queued_at_ms))
            .map(|(id, _)| id.clone())
    }

    /// Whether nothing is waiting or being worked on. Minutes waiting for a
    /// model that cannot be reached do not count: that can last for hours.
    pub fn is_settled(&self) -> bool {
        !lock(&self.slots).values().any(|slot| {
            matches!(slot.summary.status, Status::Queued | Status::Working)
                && slot.summary.error != "speech_model_missing"
        })
    }

    /// The speech model to transcribe with now: the chosen one, or one that
    /// stands in while it is not on this computer; `None` when no usable
    /// model is here yet.
    fn speech_model_now(&self) -> Option<SpeechModelNow> {
        self.config().speech_model_now(self.model_here())
    }

    /// Whether a speech model is downloaded. Without the model manager (a
    /// run that has none) every model counts as here, as it always did.
    fn model_here(&self) -> impl Fn(&str) -> bool + '_ {
        use crate::managers::model::ModelManager;
        let models = self.app.try_state::<Arc<ModelManager>>();
        move |id: &str| {
            models.as_ref().is_none_or(|models| {
                models
                    .get_model_info(id)
                    .is_some_and(|model| model.is_downloaded)
            })
        }
    }

    fn process(&self, id: &str) -> Result<()> {
        let mut folder = self.folder(id)?;
        let mut meeting = Library::read(&folder)?;
        let _awake = KeepAwake::new();
        let started = Instant::now();
        let cancel = AtomicBool::new(false);

        if meeting.transcript.is_none() && self.speech_model_now().is_none() {
            // Nothing is wrong with the recording. It waits, without using
            // up its tries, until the model has been downloaded.
            if meeting.error != "speech_model_missing" {
                meeting.error = "speech_model_missing".to_string();
                meeting.status = Status::Queued;
                self.save(&meeting, &folder)?;
            }
            if let Some(slot) = lock(&self.slots).get_mut(id) {
                slot.retry_at_ms = now_ms() + 30_000;
            }
            return Ok(());
        }

        if meeting.transcript.is_none() {
            meeting.status = Status::Working;
            meeting.stage = "decoding".to_string();
            meeting.percent = 0;
            meeting.error.clear();
            meeting.error_detail.clear();
            meeting.attempts += 1;
            self.save(&meeting, &folder)?;

            match self.transcribe(&mut meeting, &folder, &cancel) {
                Ok(true) => {}
                Ok(false) => {
                    meeting.status = Status::Queued;
                    meeting.stage.clear();
                    return self.save(&meeting, &folder);
                }
                Err(trouble) => return self.set_back(&mut meeting, &folder, trouble),
            }
            // The start time may be known better now; the folder is named by it.
            folder = self
                .library()
                .retitle_folder(&folder, meeting.started_at_ms, &meeting.title);
            meeting.status = Status::Transcribed;
            meeting.stage.clear();
            meeting.retry_at_ms = 0;
            meeting.total_ms += started.elapsed().as_millis() as u64;
            self.save(&meeting, &folder)?;
            self.write_evidence(&meeting, &folder);
        }

        let has_speech = meeting
            .transcript
            .as_ref()
            .is_some_and(|transcript| !transcript.segments.is_empty());
        if !has_speech {
            meeting.status = Status::Done;
            meeting.error = "no_speech".to_string();
            meeting.percent = 100;
            self.save(&meeting, &folder)?;
            self.notify(
                say(
                    &self.app,
                    "녹음에서 말소리를 찾지 못했습니다",
                    "No speech was found in the recording",
                ),
                &shown_title(&meeting),
            );
            return Ok(());
        }

        if meeting.minutes.is_empty() {
            let to_try = self.ways_to_write();
            if to_try.is_empty() {
                return self.wait_for_a_way(&mut meeting, &folder);
            }
            let minutes_started = Instant::now();
            meeting.status = Status::Working;
            meeting.stage = "writing_minutes".to_string();
            meeting.percent = 80;
            meeting.minutes_attempts += 1;
            self.save(&meeting, &folder)?;

            match self.write_minutes(&mut meeting, &to_try, &cancel) {
                Ok(true) => {}
                Ok(false) => {
                    meeting.status = Status::Transcribed;
                    meeting.stage.clear();
                    return self.save(&meeting, &folder);
                }
                Err(trouble) => {
                    // The transcript is kept and readable; the minutes are
                    // tried again later.
                    warn!(
                        "Minutes for {} failed (try {}): {}",
                        meeting.id, meeting.minutes_attempts, trouble.detail
                    );
                    meeting.status = Status::Transcribed;
                    meeting.stage.clear();
                    meeting.error = trouble.code.to_string();
                    meeting.error_detail = trouble.detail;
                    let give_up =
                        !trouble.retry || meeting.minutes_attempts >= MAX_MINUTES_ATTEMPTS;
                    meeting.retry_at_ms = if give_up {
                        0
                    } else {
                        now_ms() + backoff_ms(meeting.minutes_attempts)
                    };
                    self.save(&meeting, &folder)?;
                    if give_up {
                        if let Some(slot) = lock(&self.slots).get_mut(&meeting.id) {
                            slot.parked = true;
                        }
                        self.notify(
                            say(
                                &self.app,
                                "회의록을 아직 쓰지 못했습니다",
                                "The minutes are not written yet",
                            ),
                            say(
                                &self.app,
                                "받아쓴 글은 저장했습니다. 회의록을 쓰는 모델에 연결되면 다시 씁니다.",
                                "The transcript is saved. Minutes will be written once the model can be reached.",
                            ),
                        );
                    }
                    return Ok(());
                }
            }
            meeting.minutes_ms = minutes_started.elapsed().as_millis() as u64;
            meeting.total_ms += meeting.minutes_ms;
        }

        folder = self
            .library()
            .retitle_folder(&folder, meeting.started_at_ms, &meeting.title);
        meeting.status = Status::Done;
        meeting.stage.clear();
        meeting.percent = 100;
        meeting.error.clear();
        meeting.error_detail.clear();
        meeting.retry_at_ms = 0;
        self.save(&meeting, &folder)?;
        self.write_evidence(&meeting, &folder);
        self.leave_minutes_beside_source(&meeting, &folder);

        let summary = meeting.summary(&folder);
        info!(
            "Minutes ready for {}: {} verified, {} to check, in {} s",
            meeting.id,
            summary.verified,
            summary.to_check,
            meeting.total_ms / 1_000
        );
        self.show_banner(Banner::Ready {
            id: meeting.id.clone(),
            title: shown_title(&meeting),
            actions: summary.actions,
            to_check: summary.to_check,
        });
        self.put_in_drive(&meeting);
        Ok(())
    }

    /// Has a meeting whose minutes are ready put in the shared Drive
    /// folder, when Google is connected: a phone's meeting always, one of
    /// this computer's when the setting says so. Not connected, nothing
    /// changes.
    fn put_in_drive(&self, meeting: &Meeting) {
        if !self.phone_status().connected {
            return;
        }
        if meeting.origin == Origin::Phone || self.config().drive_upload_own {
            self.uploads.want(&meeting.id);
        }
    }

    /// Copies the minutes of a recording that came from a watched folder to
    /// that folder, beside the recording: `<its name> 회의록.md`, and the
    /// same as `.txt` for phones that will not open the first. The copy is a
    /// convenience; the meeting's own folder stays the record.
    fn leave_minutes_beside_source(&self, meeting: &Meeting, folder: &Path) {
        if meeting.origin != Origin::Watched || !self.config().minutes_beside_source {
            return;
        }
        let source = Path::new(&meeting.source_path);
        let (Some(beside), Some(name)) = (
            source.parent().filter(|dir| dir.is_dir()),
            source.file_stem().and_then(|stem| stem.to_str()),
        ) else {
            return;
        };
        let minutes = folder.join(store::MINUTES_NAME);
        for ending in ["md", "txt"] {
            let copy = beside.join(format!("{name} 회의록.{ending}"));
            if let Err(e) = std::fs::copy(&minutes, &copy) {
                warn!("Could not leave the minutes at {}: {e}", copy.display());
            }
        }
    }

    /// Records why a meeting could not be transcribed and when to try again.
    fn set_back(&self, meeting: &mut Meeting, folder: &Path, trouble: Trouble) -> Result<()> {
        warn!(
            "Transcribing {} failed (try {}): {}",
            meeting.id, meeting.attempts, trouble.detail
        );
        meeting.stage.clear();
        meeting.percent = 0;
        meeting.error = trouble.code.to_string();
        meeting.error_detail = trouble.detail;
        if trouble.retry && meeting.attempts < MAX_TRANSCRIBE_ATTEMPTS {
            meeting.status = Status::Queued;
            meeting.retry_at_ms = now_ms() + backoff_ms(meeting.attempts);
            return self.save(meeting, folder);
        }
        meeting.status = Status::Failed;
        meeting.retry_at_ms = 0;
        self.save(meeting, folder)?;
        self.notify(
            say(
                &self.app,
                "녹음을 정리하지 못했습니다",
                "A recording could not be processed",
            ),
            &shown_title(meeting),
        );
        Ok(())
    }

    /// `Ok(false)` if it was stopped part way.
    fn transcribe(
        &self,
        meeting: &mut Meeting,
        folder: &Path,
        cancel: &AtomicBool,
    ) -> Result<bool, Trouble> {
        let audio = folder.join(&meeting.audio_file);
        // A file that cannot be read will not read better next time.
        let pcm = audio_in::decode(&audio).map_err(|e| Trouble::new("cannot_decode", e, false))?;
        meeting.duration_ms = pcm.len() as u64 * 1_000 / RATE as u64;
        if meeting.time_source == "file_modified" {
            meeting.started_at_ms -= meeting.duration_ms as i64;
            meeting.time_source = "file_modified_minus_length".to_string();
        }
        // Now that its start and length are known, the calendar can say
        // which meeting this was.
        self.apply_calendar(meeting, None);

        let config = self.config();
        let vocabulary = Vocabulary::load(&config.vocabulary(&self.data_dir)).unwrap_or_else(|e| {
            warn!("The vocabulary could not be read, going without: {e:#}");
            Vocabulary::default()
        });
        // What is known about this meeting beforehand steers the hints.
        let mut mentions: Vec<String> = Vec::new();
        if !meeting.title_is_auto && !meeting.title.is_empty() {
            mentions.push(meeting.title.clone());
        }
        mentions.extend(meeting.people.iter().cloned());
        if !meeting.customer.is_empty() {
            mentions.push(meeting.customer.clone());
        }
        if let Some(event) = &meeting.event {
            mentions.push(event.title.clone());
        }
        if !meeting.place.is_empty() {
            mentions.push(meeting.place.clone());
        }
        mentions.extend(meeting.terms.iter().cloned());

        let monitor = JobMonitor {
            secretary: self,
            id: &meeting.id,
            cancel,
        };
        let outcome = transcribe_now(&config, self.model_here(), |model| {
            let request = Request {
                pcm: &pcm,
                language: &config.language,
                vocabulary: &vocabulary,
                mentions: &mentions,
                diarize: config.diarize,
                model_id: Some(model),
            };
            pipeline::transcribe(&self.app, &request, &monitor)
        });
        match outcome {
            Ok(Some(outcome)) => {
                if let Some(chosen) = &outcome.stats.stood_in_for {
                    info!(
                        "{chosen} is not here; {} was transcribed with {}",
                        meeting.id, outcome.transcript.model
                    );
                }
                meeting.transcript = Some(outcome.transcript);
                meeting.stats = Some(outcome.stats);
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(e) => Err(Trouble::new("speech_model", e, true)),
        }
    }

    /// The ways minutes can be written with now, in the order they are
    /// tried ([`Ways::to_try`]): the one chosen first when it can be used.
    /// A model on this computer that is not ready yet is still tried, as
    /// before, and its own error says why.
    fn ways_to_write(&self) -> Vec<Provider> {
        let chosen = self.config().summary;
        let mut to_try = Ways::look(&chosen, &|name| super::secrets::has(name)).to_try(&chosen);
        if to_try.is_empty() && chosen.is_local() {
            to_try.push(chosen);
        }
        to_try
    }

    /// Minutes that nothing on this computer can write: no key, no
    /// signed-in plan, no model here. They wait without using up their tries
    /// and are written as soon as a way appears; the user hears of it once.
    fn wait_for_a_way(&self, meeting: &mut Meeting, folder: &Path) -> Result<()> {
        let first_time = meeting.error != "summary_unavailable";
        if first_time {
            warn!(
                "Minutes for {}: nothing can write them ({} cannot be used, no other plan is signed in, no other key is saved, no model on this computer)",
                meeting.id,
                self.config().summary.label()
            );
        }
        meeting.status = Status::Transcribed;
        meeting.stage.clear();
        meeting.error = "summary_unavailable".to_string();
        meeting.error_detail = format!(
            "{} cannot be used, and no other way to write minutes is ready",
            self.config().summary.label()
        );
        meeting.retry_at_ms = now_ms() + LOOK_FOR_A_WAY_MS;
        self.save(meeting, folder)?;
        if first_time {
            self.notify(
                say(
                    &self.app,
                    "회의록을 아직 쓰지 못했습니다",
                    "The minutes are not written yet",
                ),
                say(
                    &self.app,
                    "받아쓴 글은 저장했습니다. 로그인하거나 키를 넣으면 바로 씁니다.",
                    "The transcript is saved. Sign in or add a key and the minutes are written at once.",
                ),
            );
        }
        Ok(())
    }

    /// Minutes waiting for another try are tried now: a way to write them
    /// may have appeared (a sign-in, a key, other settings). Only within
    /// their tries: minutes that used them all up still wait for the user.
    pub fn minutes_may_go(&self) {
        let mut any = false;
        for slot in lock(&self.slots).values_mut() {
            if slot.summary.status == Status::Transcribed
                && slot.summary.error.starts_with("summary_")
                && !slot.parked
                && slot.retry_at_ms > 0
            {
                slot.retry_at_ms = 0;
                any = true;
            }
        }
        if any {
            self.wake();
        }
    }

    /// `Ok(false)` if it was stopped part way. `to_try` are the ways to
    /// write them with, in order: when one cannot be used (a key refused, a
    /// plan run out) the next writes them, and the minutes say which did.
    fn write_minutes(
        &self,
        meeting: &mut Meeting,
        to_try: &[Provider],
        cancel: &AtomicBool,
    ) -> Result<bool, Trouble> {
        let mut last = None;
        for (index, provider) in to_try.iter().enumerate() {
            match self.write_minutes_with(meeting, provider, cancel) {
                Err(e) if index + 1 < to_try.len() && llm::cannot_use(&format!("{e:#}")) => {
                    warn!(
                        "Minutes for {} could not be written with {} ({e:#}); trying {}",
                        meeting.id,
                        provider.label(),
                        to_try[index + 1].label()
                    );
                    last = Some(e);
                }
                Err(e) => {
                    let code = llm::trouble_code(&format!("{e:#}"));
                    return Err(Trouble::new(code, e, true));
                }
                Ok(written) => return Ok(written),
            }
        }
        let e = last.unwrap_or_else(|| anyhow!("there is no way to write minutes"));
        let code = llm::trouble_code(&format!("{e:#}"));
        Err(Trouble::new(code, e, true))
    }

    fn write_minutes_with(
        &self,
        meeting: &mut Meeting,
        provider: &Provider,
        cancel: &AtomicBool,
    ) -> Result<bool> {
        let all_templates = templates::load(&self.data_dir.join("templates"));
        // The lines as the user fixed them: the minutes rest on those.
        let Some(transcript) = meeting
            .transcript
            .as_ref()
            .map(super::transcript::Transcript::as_read)
        else {
            return Err(anyhow!("there is no transcript"));
        };
        let recorded_on = meeting.recorded_on();
        let context = meeting.context();
        let feedback_rules = super::feedback::rules(&self.data_dir);
        let mut people = meeting.people.clone();
        if !meeting.customer.is_empty() && !people.contains(&meeting.customer) {
            people.push(meeting.customer.clone());
        }
        // Keys for the user's own paid models come from the system's
        // keychain, and only when such a model is the one writing.
        let keychain = |name: &str| super::secrets::get(name);
        let id = meeting.id.clone();
        let progress = |fraction: f32| {
            self.progress(
                &id,
                "writing_minutes",
                80 + (fraction.clamp(0.0, 1.0) * 19.0) as u8,
            )
        };
        let input = minutes::Input {
            transcript: &transcript,
            templates: &all_templates,
            template_id: (!meeting.template_id.is_empty()).then_some(meeting.template_id.as_str()),
            marks_ms: &meeting.marks_ms,
            mark_notes: &meeting.mark_notes,
            people: &people,
            customer: &meeting.customer,
            recorded_on: &recorded_on,
            context: &context,
            feedback_rules: &feedback_rules,
            provider,
            secrets: &keychain,
        };
        match minutes::write(&input, &progress, cancel) {
            Ok(Some(output)) => {
                if let Some(transcript) = meeting.transcript.as_mut() {
                    for (speaker, name) in &output.speaker_names {
                        if let Some(entry) =
                            transcript.speakers.iter_mut().find(|s| s.id == *speaker)
                        {
                            entry.name = name.clone();
                        }
                    }
                }
                let title = output.minutes.title.trim();
                if meeting.title_is_auto && !title.is_empty() {
                    meeting.title = store::safe_name(title);
                }
                let mut minutes = output.minutes;
                minutes.give_ids();
                meeting.minutes = vec![minutes];
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

/// Reports the pipeline's progress as the meeting's.
struct JobMonitor<'a> {
    secretary: &'a Secretary,
    id: &'a str,
    cancel: &'a AtomicBool,
}

impl Monitor for JobMonitor<'_> {
    fn progress(&self, stage: Stage, fraction: f32) {
        let fraction = fraction.clamp(0.0, 1.0);
        // Transcribing is most of the work; the minutes take the last fifth.
        let (name, from, span) = match stage {
            Stage::DetectingSpeech => ("detecting_speech", 2.0, 3.0),
            Stage::Diarizing => ("diarizing", 5.0, 20.0),
            Stage::Transcribing => ("transcribing", 25.0, 55.0),
        };
        self.secretary
            .progress(self.id, name, (from + fraction * span) as u8);
    }

    fn cancel_flag(&self) -> &AtomicBool {
        self.cancel
    }
}
