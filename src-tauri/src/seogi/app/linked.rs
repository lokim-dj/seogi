//! Places linked to the app: watched folders, the phone's recordings and
//! meetings in Drive.

use super::*;

impl Secretary {
    // ----- watched folders --------------------------------------------------

    pub(super) fn watch_loop(self: Arc<Self>) {
        let mut settling = Settling::default();
        // Files that could not be brought in, and when: they are tried again
        // after a while, not at every look.
        let mut refused: HashMap<String, Instant> = HashMap::new();
        const TRY_AGAIN_AFTER: Duration = Duration::from_secs(5 * 60);
        let every = std::env::var("SEOGI_WATCH_SECS")
            .ok()
            .and_then(|secs| secs.parse().ok())
            .map_or(WATCH_EVERY, Duration::from_secs);
        loop {
            self.pick_up_library();
            self.note_watched_folders();
            for watched in self.config().watched.iter().filter(|w| w.enabled) {
                let folder = Path::new(&watched.path);
                // Until what was already there has been noted (a drive not
                // connected yet), nothing in the folder is taken.
                let noted = format!("{}|{}", folder.display(), watched.since_ms);
                if !lock(&self.ledger).noted.contains(&noted) {
                    continue;
                }
                for (file, look) in settling.settled(folder, now_ms()) {
                    let key = Ledger::key(&file, look);
                    if lock(&self.ledger).seen.contains_key(&key) {
                        continue;
                    }
                    if refused
                        .get(&key)
                        .is_some_and(|at| at.elapsed() < TRY_AGAIN_AFTER)
                    {
                        continue;
                    }
                    // A file the cloud has not brought down yet is read (and
                    // so fetched) here; if that fails it is tried again in a
                    // few minutes.
                    match self.import(&file, Origin::Watched, &watched.template_id) {
                        Ok(imported) => {
                            let mut ledger = lock(&self.ledger);
                            ledger.seen.insert(key, imported.sha256);
                            if let Err(e) = ledger.save(&self.data_dir) {
                                warn!("Could not save the ledger: {e:#}");
                            }
                        }
                        Err(e) => {
                            warn!("Could not bring in {}: {e:#}", file.display());
                            refused.insert(key, Instant::now());
                        }
                    }
                }
            }
            std::thread::sleep(every);
        }
    }

    // ----- the phone's recordings --------------------------------------------

    /// Looks at the phone's Drive folder when the app starts, every few
    /// minutes after, and when asked.
    pub(super) fn phone_loop(self: Arc<Self>) {
        loop {
            self.phone.pull(self.as_ref());
            self.phone.wait(phone::LOOK_EVERY);
        }
    }

    pub fn phone_status(&self) -> PhoneStatus {
        self.phone.status()
    }

    /// Connects Google in the browser (`open` opens the address); blocks
    /// until the user is done there.
    pub fn phone_connect(&self, open: impl FnOnce(&str) -> Result<()>) -> Result<String> {
        let account = self.phone.connect(open)?;
        self.uploads.wake_up();
        Ok(account)
    }

    pub fn phone_cancel_connect(&self) {
        self.phone.cancel_connect();
    }

    pub fn phone_disconnect(&self) -> Result<()> {
        self.phone.disconnect()
    }

    pub fn phone_look_now(&self) {
        self.phone.look_now();
    }

    // ----- meetings in Drive --------------------------------------------------

    /// Puts meetings in the shared Drive folder when asked, and every few
    /// minutes for what is still waiting.
    pub(super) fn drive_loop(self: Arc<Self>) {
        loop {
            self.uploads.run(self.as_ref());
            self.uploads.wait();
        }
    }

    pub fn drive_status(&self) -> DriveStatus {
        self.uploads.status()
    }

    fn drive_settings(&self) -> upload::Settings {
        let config = self.config();
        upload::Settings {
            own: config.drive_upload_own,
            audio: config.drive_upload_audio,
        }
    }

    /// The meetings "지난 회의 올리기" would put in Drive, and about how
    /// much would go up.
    pub fn drive_backlog(&self) -> (Vec<String>, Backlog) {
        let meetings = self.library().scan();
        self.uploads
            .backlog_of(meetings.iter(), self.drive_settings())
    }

    /// "지난 회의 올리기": puts the meetings done before in Drive. Returns
    /// how many.
    pub fn drive_upload_backlog(&self) -> Result<u32> {
        if !self.phone_status().connected {
            bail!("not_connected");
        }
        let (ids, _) = self.drive_backlog();
        for id in &ids {
            self.uploads.want(id);
        }
        Ok(ids.len() as u32)
    }

    /// "드라이브에서 빼기", or putting it back.
    pub fn drive_exclude(&self, id: &str, excluded: bool) -> Result<()> {
        self.folder(id)?;
        if excluded {
            self.uploads.exclude(id);
        } else {
            self.uploads.include(id);
        }
        Ok(())
    }
}

impl phone::Host for Secretary {
    fn done(&self, key: &str) -> bool {
        lock(&self.ledger).phone.contains_key(key)
    }

    fn known(&self, sha256: &str) -> bool {
        let ledger = lock(&self.ledger);
        let sha256 = sha256.to_lowercase();
        ledger.deleted.contains(&sha256)
            || ledger
                .hashes
                .get(&sha256)
                .is_some_and(|id| lock(&self.slots).contains_key(id))
    }

    fn bring_in(&self, file: &Path, arrival: &Arrival) -> Result<String> {
        let imported = self.import_from_phone(file, arrival)?;
        Ok(if imported.duplicate {
            String::new()
        } else {
            imported.id
        })
    }

    fn note(&self, key: &str, meeting_id: &str) {
        let mut ledger = lock(&self.ledger);
        ledger.phone.insert(key.to_string(), meeting_id.to_string());
        if let Err(e) = ledger.save(&self.data_dir) {
            warn!("Could not save the ledger: {e:#}");
        }
    }

    fn brought_from(&self, meeting_id: &str, drive_files: &[String]) {
        self.uploads.note_phone_files(meeting_id, drive_files);
    }

    fn lost_sign_in(&self) {
        self.notify(
            say(
                &self.app,
                "폰 녹음을 가져오지 못했습니다",
                "Phone recordings are not coming in",
            ),
            say(
                &self.app,
                "설정 > 보관과 일정에서 구글을 다시 연결해 주세요.",
                "Connect Google again in Settings > Storage and calendar.",
            ),
        );
    }
}

impl upload::Library for Secretary {
    fn meeting(&self, id: &str) -> Result<Option<(Meeting, PathBuf)>> {
        let Ok(folder) = self.folder(id) else {
            return Ok(None);
        };
        let mut meeting = Library::read(&folder)?;
        // Tasks get their ids when first put in Drive; the ids are kept
        // here too, so they stay the same.
        if meeting.status != Status::Working
            && meeting
                .minutes
                .first_mut()
                .is_some_and(minutes::Minutes::give_ids)
        {
            self.save(&meeting, &folder)?;
        }
        Ok(Some((meeting, folder)))
    }

    fn phone_audio(&self, id: &str) -> Vec<String> {
        lock(&self.ledger)
            .phone
            .iter()
            .filter(|(_, meeting)| meeting.as_str() == id)
            .filter_map(|(key, _)| key.split('|').next().map(str::to_string))
            .collect()
    }

    fn settings(&self) -> upload::Settings {
        self.drive_settings()
    }

    fn low_space(&self) {
        self.notify(
            say(
                &self.app,
                "구글 드라이브 공간이 부족합니다",
                "Google Drive is almost full",
            ),
            say(
                &self.app,
                "녹음을 올리면 남은 공간이 1GB보다 적어져, 녹음 파일은 기다리고 회의록과 받아쓴 글만 올립니다.",
                "A recording would leave less than 1 GB free, so it waits; the minutes and transcripts still go up.",
            ),
        );
    }

    fn lost_sign_in(&self) {
        phone::Host::lost_sign_in(self);
    }

    fn phone_edits_arrived(
        &self,
        id: &str,
        winning: &[(String, serde_json::Value)],
    ) -> Result<bool> {
        let Ok(folder) = self.folder(id) else {
            return Ok(false);
        };
        let mut meeting = Library::read(&folder)?;
        if meeting.status == Status::Working {
            bail!("it is being worked on right now");
        }
        let mut changed = false;
        for (key, value) in winning {
            if key.starts_with("speaker:") {
                // A name follows the person into the tasks, as naming here does.
                let old = key
                    .strip_prefix("speaker:")
                    .and_then(|n| n.parse::<u32>().ok())
                    .and_then(|n| meeting.transcript.as_ref().map(|t| t.speaker_name(n)));
                if edits::apply(&mut meeting, key, value) {
                    if let (Some(old), Some(name)) = (old, value.as_str()) {
                        owners_follow(&mut meeting, &old, name.trim(), 0, u64::MAX);
                    }
                    changed = true;
                }
            } else {
                changed |= edits::apply(&mut meeting, key, value);
            }
        }
        if changed {
            self.save(&meeting, &folder)?;
        } else {
            // A change that did not stand shows as a faint line: the screen looks again.
            self.changed(id);
        }
        Ok(changed)
    }
}
