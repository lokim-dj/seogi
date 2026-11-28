//! Recording here: the microphone, marks, and the shortcuts.

use super::*;

impl Secretary {
    // ----- recording --------------------------------------------------------

    /// Starts recording a meeting from the microphone. Returns its id.
    pub fn start_recording(
        self: &Arc<Self>,
        title: Option<String>,
        event: Option<(String, i64)>,
    ) -> Result<String> {
        let mut recording = lock(&self.recording);
        if recording.is_some() {
            bail!("a recording is already in progress");
        }
        let config = self.config();
        let started_at_ms = now_ms();
        let title = title.map(|t| t.trim().to_string()).unwrap_or_default();
        let folder = self.library().create_folder(started_at_ms, &title)?;
        let path = folder.join(store::RECORDING_NAME);

        let me = Arc::downgrade(self);
        let device = (!config.input_device.is_empty()).then(|| config.input_device.clone());
        // What the microphone does is told on the recording's own thread
        // and handled on one other thread, in the order it happened, so a
        // quick loss and return never leave the screen saying "away". The
        // thread ends when the recording does (its sender goes with it).
        let (tell, told) = std::sync::mpsc::channel::<recorder::Event>();
        let session_folder = folder.clone();
        std::thread::spawn(move || {
            for event in told {
                let Some(me) = me.upgrade() else {
                    return;
                };
                me.microphone_event(&session_folder, event);
            }
        });
        let tell = Mutex::new(tell);
        let session = recorder::Session::start(&path, device, move |event| {
            let _ = lock(&tell).send(event);
        });
        let session = match session {
            Ok(session) => session,
            Err(e) => {
                // Nothing was recorded, so there is no meeting.
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::remove_dir(&folder);
                return Err(e);
            }
        };

        let mut meeting = Meeting::new(started_at_ms, Origin::Recorded, store::RECORDING_NAME);
        meeting.title_is_auto = title.is_empty();
        meeting.title = title;
        meeting.time_source = "recorder".to_string();
        meeting.status = Status::Recording;
        meeting.device = session.device();
        self.apply_calendar(
            &mut meeting,
            event.as_ref().map(|(uid, start)| (uid.as_str(), *start)),
        );
        self.save(&meeting, &folder)?;
        *recording = Some(Active {
            id: meeting.id.clone(),
            folder,
            session,
            started_at_ms,
            marks_ms: Vec::new(),
            _awake: KeepAwake::new(),
        });
        drop(recording);
        info!("Recording {} from {}", meeting.id, meeting.device);
        self.recording_changed(true);
        self.watch_for_no_sound(&meeting.id);
        Ok(meeting.id)
    }

    /// A microphone macOS does not let Seogi use gives silence, not an
    /// error. A recording started by its shortcut or by the calendar may have
    /// no window in sight, so a recording that hears nothing at all a few
    /// seconds in is said once with a notification.
    fn watch_for_no_sound(self: &Arc<Self>, id: &str) {
        let me = Arc::downgrade(self);
        let id = id.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(6));
            let Some(me) = me.upgrade() else {
                return;
            };
            let dead = lock(&me.recording)
                .as_ref()
                .is_some_and(|active| active.id == id && active.session.is_dead());
            if dead {
                warn!("Recording {id} hears nothing: muted, or the microphone is not allowed");
                me.notify(
                    say(&me.app, "소리가 들어오지 않습니다", "No sound is coming in"),
                    say(
                        &me.app,
                        "마이크 음소거나, 시스템 설정 › 개인정보 보호 및 보안 › 마이크에서 서기가 켜져 있는지 확인해 주세요.",
                        "Check that the microphone is not muted and that Seogi is on in System Settings › Privacy & Security › Microphone.",
                    ),
                );
            }
        });
    }

    /// Ends the recording and queues it. Returns its id.
    pub fn stop_recording(&self) -> Result<String> {
        let active = lock(&self.recording)
            .take()
            .ok_or_else(|| anyhow!("no recording is in progress"))?;
        self.finish_recording(active, None)
    }

    /// What the recording in `folder` says about its microphone. A laptop's
    /// lid closed or a headset dropped does not end the meeting: the
    /// recording goes on when the microphone is back, and only one that
    /// stays away for long ends it.
    fn microphone_event(&self, folder: &Path, event: recorder::Event) {
        // Said by a recording that has ended since: nothing to do.
        let current = lock(&self.recording)
            .as_ref()
            .is_some_and(|active| active.folder == folder);
        if !current {
            return;
        }
        match event {
            recorder::Event::Lost { reason } => {
                warn!("The microphone went away; reconnecting: {reason}");
                // Where it went is kept with the meeting at once, so a
                // crash while it is away still knows.
                self.keep_microphone_state(folder);
                self.reconnecting_changed(folder);
                // Said now, and said again when it is back: a notice shown
                // while the lid is shut is seen by nobody.
                self.notify(
                    say(
                        &self.app,
                        "마이크 끊김, 다시 연결 중",
                        "Microphone lost, reconnecting",
                    ),
                    say(
                        &self.app,
                        "회의는 끝내지 않았습니다. 마이크가 돌아오면 같은 녹음에 이어서 씁니다.",
                        "The meeting goes on. When the microphone is back, the same recording continues.",
                    ),
                );
            }
            recorder::Event::Back { gap, device } => {
                info!("The microphone is back after {} ms: {device}", gap.gap_ms);
                // The gap and the microphone now in use (the default one
                // when the chosen one stayed away) are kept with the
                // meeting at once.
                self.keep_microphone_state(folder);
                self.reconnecting_changed(folder);
                let words = gap.words();
                self.notify(
                    say(
                        &self.app,
                        "마이크가 다시 연결됐습니다",
                        "The microphone is back",
                    ),
                    &if is_korean(&self.app) {
                        format!("{words} 끊겼던 녹음을 같은 회의에 이어서 씁니다.")
                    } else {
                        format!(
                            "The recording goes on in the same meeting after {} s.",
                            gap.gap_ms.div_ceil(1_000)
                        )
                    },
                );
            }
            recorder::Event::GaveUp { reason } => self.recording_broke(
                reason,
                say(
                    &self.app,
                    "마이크가 10분 동안 돌아오지 않았습니다. 그때까지의 녹음으로 회의록을 만듭니다.",
                    "The microphone did not come back for 10 minutes. Minutes will be made from what was recorded.",
                ),
            ),
            recorder::Event::Broken { reason } => self.recording_broke(
                reason,
                say(
                    &self.app,
                    "마이크가 끊기면서 녹음을 더 이어 쓸 수 없게 됐습니다. 저장된 데까지의 녹음으로 회의록을 만듭니다.",
                    "When the microphone went away the recording could not be continued. Minutes will be made from what was saved.",
                ),
            ),
        }
    }

    /// Keeps the recording's gaps so far (one still open has no length
    /// yet) and its microphone with the meeting.
    fn keep_microphone_state(&self, folder: &Path) {
        let recording = lock(&self.recording);
        let Some(active) = recording.as_ref().filter(|a| a.folder == folder) else {
            return;
        };
        let kept = Library::read(&active.folder).and_then(|mut meeting| {
            meeting.gaps = active.session.gaps_so_far();
            meeting.device = active.session.device();
            self.save(&meeting, &active.folder)
        });
        if let Err(e) = kept {
            warn!("Could not keep the microphone's gap: {e:#}");
        }
    }

    /// The menu bar and the window follow whether the recording's
    /// microphone is away now, as the recording says at this moment.
    fn reconnecting_changed(&self, folder: &Path) {
        let Some(reconnecting) = lock(&self.recording)
            .as_ref()
            .filter(|active| active.folder == folder)
            .map(|active| active.session.is_reconnecting())
        else {
            return;
        };
        let _ = RecordingChanged { recording: true }.emit(&self.app);
        if self.quiet {
            return;
        }
        use crate::notices::{self, Notice, NoticeCode, NoticeSource};
        super::status::set_meeting_reconnecting(&self.app, reconnecting);
        if reconnecting {
            notices::raise_without_overlay(
                &self.app,
                Notice::new(NoticeCode::MicrophoneReconnecting, NoticeSource::Recording),
            );
        } else {
            notices::clear_matching(
                &self.app,
                NoticeSource::Recording,
                Some(NoticeCode::MicrophoneReconnecting),
            );
        }
    }

    /// The recording cannot go on without its microphone: the meeting ends
    /// with what was recorded, and `why` says so.
    fn recording_broke(&self, reason: String, why: &str) {
        let Some(active) = lock(&self.recording).take() else {
            return;
        };
        warn!("The recording ends without a microphone: {reason}");
        match self.finish_recording(active, Some(("microphone_lost", reason))) {
            Ok(_) => self.notify(
                say(&self.app, "녹음이 멈췄습니다", "The recording stopped"),
                why,
            ),
            Err(e) => error!("Could not finish the interrupted recording: {e:#}"),
        }
    }

    fn finish_recording(
        &self,
        active: Active,
        trouble: Option<(&'static str, String)>,
    ) -> Result<String> {
        let Active {
            id,
            folder,
            session,
            ..
        } = active;
        let path = folder.join(store::RECORDING_NAME);
        let gaps_so_far = session.gaps();
        let (frames, trouble, gaps) = match session.stop() {
            Ok(finished) => (
                finished.frames,
                trouble.or(finished.trouble.map(|why| ("microphone_lost", why))),
                finished.gaps,
            ),
            // The file was not completed; make whole what reached the disk.
            Err(e) => (
                recorder::repair(&path).unwrap_or(0),
                Some(("file_incomplete", format!("{e:#}"))),
                gaps_so_far,
            ),
        };
        let mut meeting = Library::read(&folder)?;
        meeting.duration_ms = frames * 1_000 / u64::from(recorder::RATE);
        meeting.gaps = gaps;
        let queued = self.queue_recorded(&mut meeting, &folder, trouble);
        self.recording_changed(false);
        queued.map(|()| id)
    }

    /// A finished recording gets its hash and evidence record and joins the
    /// queue.
    pub(super) fn queue_recorded(
        &self,
        meeting: &mut Meeting,
        folder: &Path,
        trouble: Option<(&'static str, String)>,
    ) -> Result<()> {
        meeting.sha256 = evidence::sha256_file(&folder.join(&meeting.audio_file))?;
        if let Some((note, detail)) = trouble {
            meeting.capture_note = note.to_string();
            meeting.error_detail = detail;
        }
        if meeting.duration_ms < 1_000 {
            // Kept, like every recording, but there is nothing to write up.
            meeting.status = Status::Failed;
            meeting.error = "too_short".to_string();
        } else {
            meeting.status = Status::Queued;
            meeting.queued_at_ms = now_ms();
            self.remember(&meeting.sha256, &meeting.id);
        }
        self.save(meeting, folder)?;
        self.write_evidence(meeting, folder);
        self.wake();
        Ok(())
    }

    /// Marks this moment of the recording as important. Returns the moment,
    /// in milliseconds into the recording.
    pub fn mark(&self) -> Result<u64> {
        let mut recording = lock(&self.recording);
        let active = recording
            .as_mut()
            .ok_or_else(|| anyhow!("no recording is in progress"))?;
        let at = active.session.elapsed_ms();
        let mut meeting = Library::read(&active.folder)?;
        meeting.marks_ms.push(at);
        self.save(&meeting, &active.folder)?;
        active.marks_ms.push(at);
        Ok(at)
    }

    pub fn recording(&self) -> Option<RecordingState> {
        lock(&self.recording).as_ref().map(|active| RecordingState {
            id: active.id.clone(),
            started_at_ms: active.started_at_ms,
            elapsed_ms: active.session.elapsed_ms(),
            level: active.session.level(),
            marks_ms: active.marks_ms.clone(),
            device: active.session.device(),
            no_sound: active.session.is_dead(),
            reconnecting: active.session.is_reconnecting(),
        })
    }

    /// Whether a recording is in progress, without waiting for one that is
    /// just starting (which counts).
    pub fn is_recording(&self) -> bool {
        self.recording
            .try_lock()
            .map_or(true, |recording| recording.is_some())
    }

    fn recording_changed(&self, recording: bool) {
        let _ = RecordingChanged { recording }.emit(&self.app);
        if !self.quiet {
            if !recording {
                // Ended while the microphone was away: that is over too.
                crate::notices::clear_matching(
                    &self.app,
                    crate::notices::NoticeSource::Recording,
                    Some(crate::notices::NoticeCode::MicrophoneReconnecting),
                );
            }
            super::status::set_meeting_recording(&self.app, recording);
            crate::tray::set_tray_state(
                &self.app,
                if recording {
                    crate::tray::TrayIconState::Recording
                } else {
                    crate::tray::TrayIconState::Idle
                },
            );
            crate::tray::rebuild_tray_menu(&self.app);
        }
    }

    /// Starts a recording, or ends the one in progress.
    pub fn toggle_recording(self: &Arc<Self>) {
        let result = if self.is_recording() {
            self.stop_recording()
        } else {
            self.start_recording(None, None)
        };
        if let Err(e) = result {
            warn!("Could not start or stop recording: {e:#}");
            self.notify(
                say(&self.app, "녹음을 시작하지 못했습니다", "Could not record"),
                &format!("{e:#}"),
            );
        }
    }

    pub(super) fn register_shortcuts(self: &Arc<Self>) {
        use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
        let config = self.config();
        let bindings: [(String, Shortcut); 2] = [
            (config.record_shortcut, |me| me.toggle_recording()),
            (config.mark_shortcut, |me| {
                let _ = me.mark();
            }),
        ];
        for (keys, action) in bindings {
            if keys.trim().is_empty() {
                continue;
            }
            let me = self.clone();
            let registered =
                self.app
                    .global_shortcut()
                    .on_shortcut(keys.as_str(), move |_, _, event| {
                        if event.state == ShortcutState::Pressed {
                            let me = me.clone();
                            std::thread::spawn(move || action(&me));
                        }
                    });
            if let Err(e) = registered {
                warn!("Could not set the shortcut {keys}: {e}");
            }
        }
    }
}

/// What a global shortcut does.
type Shortcut = fn(&Arc<Secretary>);
