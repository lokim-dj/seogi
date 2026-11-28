//! The library of meetings, and recordings arriving in it: brought in,
//! deleted and put back, tried again.

use super::*;

impl Secretary {
    // ----- the library ------------------------------------------------------

    pub(super) fn read_library(&self) {
        let found = self.library().scan();
        let mut slots = lock(&self.slots);
        let mut ledger = lock(&self.ledger);
        slots.clear();
        for (meeting, folder) in found {
            if !meeting.sha256.is_empty() {
                ledger
                    .hashes
                    .entry(meeting.sha256.clone())
                    .or_insert_with(|| meeting.id.clone());
            }
            slots.insert(meeting.id.clone(), Self::slot(&meeting, &folder));
        }
    }

    /// Takes in meeting folders that appeared in the library since it was
    /// read: put there by another copy of the app, or copied in by hand. A
    /// meeting already known (by its id or its folder) is left as it is.
    /// One that is not finished may still be in another program's hands,
    /// so it waits for the user's "try again" instead of being started.
    pub(super) fn pick_up_library(&self) {
        let found = self.library().scan();
        let mut added: Vec<String> = Vec::new();
        {
            let mut slots = lock(&self.slots);
            let mut ledger = lock(&self.ledger);
            for (meeting, folder) in found {
                if slots.contains_key(&meeting.id)
                    || slots.values().any(|slot| slot.folder == folder)
                {
                    continue;
                }
                // A record written moments ago is one being made right now,
                // by this app or another: it is looked at again later.
                let settled = std::fs::metadata(folder.join(store::RECORD_NAME))
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|at| at.elapsed().ok())
                    .is_some_and(|age| age >= Duration::from_secs(60));
                if !settled {
                    continue;
                }
                if !meeting.sha256.is_empty() {
                    ledger
                        .hashes
                        .entry(meeting.sha256.clone())
                        .or_insert_with(|| meeting.id.clone());
                }
                let mut slot = Self::slot(&meeting, &folder);
                slot.parked = !matches!(meeting.status, Status::Done | Status::Failed);
                info!("Found {} in the library", meeting.id);
                added.push(meeting.id.clone());
                slots.insert(meeting.id.clone(), slot);
            }
        }
        for id in added {
            self.changed(&id);
        }
    }

    fn slot(meeting: &Meeting, folder: &Path) -> Slot {
        Slot {
            folder: folder.to_path_buf(),
            summary: meeting.summary(folder),
            queued_at_ms: meeting.queued_at_ms,
            retry_at_ms: meeting.retry_at_ms,
            parked: false,
        }
    }

    /// Every meeting, newest first.
    pub fn meetings(&self) -> Vec<MeetingSummary> {
        let mut meetings: Vec<MeetingSummary> = lock(&self.slots)
            .values()
            .map(|slot| slot.summary.clone())
            .collect();
        meetings.sort_by_key(|meeting| std::cmp::Reverse(meeting.started_at_ms));
        meetings
    }

    /// The meetings whose title, minutes or transcript have `query` in them.
    pub fn search(&self, query: &str) -> Vec<Found> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        let folders: Vec<PathBuf> = lock(&self.slots)
            .values()
            .map(|slot| slot.folder.clone())
            .collect();
        folders
            .into_iter()
            .filter_map(|folder| Library::read(&folder).ok()?.found(query))
            .collect()
    }

    pub fn folder(&self, id: &str) -> Result<PathBuf> {
        lock(&self.slots)
            .get(id)
            .map(|slot| slot.folder.clone())
            .ok_or_else(|| anyhow!("there is no meeting {id}"))
    }

    pub fn meeting(&self, id: &str) -> Result<Meeting> {
        Library::read(&self.folder(id)?)
    }

    /// Writes the meeting to its folder and tells the screen.
    pub(super) fn save(&self, meeting: &Meeting, folder: &Path) -> Result<()> {
        Library::save(meeting, folder)?;
        let mut slot = Self::slot(meeting, folder);
        {
            let mut slots = lock(&self.slots);
            if let Some(old) = slots.get(&meeting.id) {
                slot.parked = old.parked;
            }
            slots.insert(meeting.id.clone(), slot);
        }
        self.changed(&meeting.id);
        Ok(())
    }

    /// [`save`] for a change the user made: what it changed of the things
    /// the phone can change too (a task's tick, a line, a speaker's name)
    /// is noted with the time, and a meeting in Drive goes up again so the
    /// phone sees it (issue #67).
    ///
    /// [`save`]: Secretary::save
    pub(super) fn save_edit(&self, meeting: &Meeting, folder: &Path) -> Result<()> {
        let before = Library::read(folder)
            .map(|old| edits::values(&old))
            .unwrap_or_default();
        self.save(meeting, folder)?;
        let changes = edits::changed(&before, &edits::values(meeting));
        self.uploads.edited(&meeting.id, changes, now_ms());
        Ok(())
    }

    /// The things changed on both this computer and the phone that say
    /// different things: the change that stands is in the meeting, the
    /// other is kept as a faint line (issue #67).
    pub fn edit_conflicts(&self, id: &str) -> Vec<edits::Conflict> {
        self.uploads.conflicts(id)
    }

    /// Takes the change of `key` that did not stand instead of the one that
    /// did; it is noted as made now, so it stands on both sides.
    pub fn take_other_edit(&self, id: &str, key: &str) -> Result<()> {
        let other = self
            .uploads
            .losing_edit(id, key)
            .ok_or_else(|| anyhow!("there is nothing else to take"))?;
        let _editing = lock(&self.edit_lock);
        let (mut meeting, folder) = self.editable(id)?;
        let old = key
            .strip_prefix("speaker:")
            .and_then(|n| n.parse::<u32>().ok())
            .and_then(|n| meeting.transcript.as_ref().map(|t| t.speaker_name(n)));
        if edits::apply(&mut meeting, key, &other.value) {
            if let (Some(old), Some(name)) = (old, other.value.as_str()) {
                owners_follow(&mut meeting, &old, name.trim(), 0, u64::MAX);
            }
            self.save_edit(&meeting, &folder)
        } else {
            // Already so here (the phone's change stands): note it as made now.
            self.uploads
                .edited(id, vec![(key.to_string(), other.value)], now_ms());
            self.changed(id);
            Ok(())
        }
    }

    pub(super) fn changed(&self, id: &str) {
        let _ = MeetingChanged { id: id.to_string() }.emit(&self.app);
    }

    pub(super) fn progress(&self, id: &str, stage: &str, percent: u8) {
        let moved = match lock(&self.slots).get_mut(id) {
            Some(slot) if slot.summary.stage != stage || slot.summary.percent != percent => {
                slot.summary.stage = stage.to_string();
                slot.summary.percent = percent;
                true
            }
            _ => false,
        };
        if moved {
            self.changed(id);
        }
    }

    /// Gives a speaker a name. The transcript and the minutes document are
    /// written again; the recording is not touched.
    pub fn rename_speaker(&self, id: &str, speaker: u32, name: &str) -> Result<()> {
        self.name_speaker(id, speaker, name, NameScope::All, 0, 0)
    }

    /// Names a speaker in the transcript: throughout, from a moment on, or
    /// in one stretch (see [`NameScope`]). Tasks given to the speaker's old
    /// name in that part follow the person, except one resting on a line
    /// someone still called the old name says; the minutes are not written
    /// again.
    pub fn name_speaker(
        &self,
        id: &str,
        speaker: u32,
        name: &str,
        scope: NameScope,
        from_ms: u64,
        to_ms: u64,
    ) -> Result<()> {
        let _editing = lock(&self.edit_lock);
        let (mut meeting, folder) = self.editable(id)?;
        let transcript = meeting
            .transcript
            .as_mut()
            .ok_or_else(|| anyhow!("it has not been transcribed yet"))?;
        let (old, from, to) = transcript
            .name_speaker(speaker, name, scope, from_ms, to_ms)
            .ok_or_else(|| anyhow!("there is no speaker {speaker}, or no name"))?;
        owners_follow(&mut meeting, &old, name.trim(), from, to);
        self.save_edit(&meeting, &folder)
    }

    /// Puts a naming back: the speakers, who said each line and who was
    /// given each task, as the screen kept them just before it named
    /// someone (`kept`). Refused, changing nothing, unless the meeting is
    /// still as the screen saw it just after (`now`): any later change of a
    /// name, or minutes written again, keeps it as it is.
    pub fn restore_naming(&self, id: &str, kept: &Naming, now: &Naming) -> Result<()> {
        let _editing = lock(&self.edit_lock);
        let (mut meeting, folder) = self.editable(id)?;
        put_back_naming(&mut meeting, kept, now)?;
        self.save_edit(&meeting, &folder)
    }

    // ----- arriving ---------------------------------------------------------

    /// Brings a recording file into the library and queues it. The file is
    /// copied; the one given is left exactly as it was. A recording already
    /// here (the same bytes, whatever its name) is not added again.
    pub fn import(&self, source: &Path, origin: Origin, template_id: &str) -> Result<Imported> {
        self.bring_in(source, origin, template_id, None)
    }

    /// [`import`] for a recording from the phone: its title, start, marks
    /// and phone come from the JSON the phone wrote, not from the file.
    ///
    /// [`import`]: Secretary::import
    pub(super) fn import_from_phone(&self, file: &Path, arrival: &Arrival) -> Result<Imported> {
        self.bring_in(file, Origin::Phone, "", Some(arrival))
    }

    fn bring_in(
        &self,
        source: &Path,
        origin: Origin,
        template_id: &str,
        arrival: Option<&Arrival>,
    ) -> Result<Imported> {
        if !source.is_file() {
            bail!("{} is not a file", source.display());
        }
        let sha256 = evidence::sha256_file(source)?;
        {
            let mut ledger = lock(&self.ledger);
            if ledger.deleted.contains(&sha256) {
                if matches!(origin, Origin::Watched | Origin::Phone) {
                    // Its meeting went to the trash: the copy in the watched
                    // folder (or the phone's) is left alone and not brought
                    // in again.
                    return Ok(Imported {
                        id: String::new(),
                        duplicate: true,
                        sha256,
                    });
                }
                // Brought in by hand: the user wants it back.
                ledger.deleted.remove(&sha256);
                if let Err(e) = ledger.save(&self.data_dir) {
                    warn!("Could not save the ledger: {e:#}");
                }
            }
        }
        let known = lock(&self.ledger).hashes.get(&sha256).cloned();
        if let Some(id) = known {
            if lock(&self.slots).contains_key(&id) {
                return Ok(Imported {
                    id,
                    duplicate: true,
                    sha256,
                });
            }
        }

        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("{} has no usable name", source.display()))?;
        let stem = source
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        // When it was recorded: the time in its name if a recorder put one
        // there, else the file's own time, which is when the recording ended
        // and is corrected by its length once that is known.
        let modified_ms = Look::at(source).map_or_else(now_ms, |look| look.modified_ms);
        // A time the recorder wrote inside the file is the next best: it is
        // still true when the file was uploaded or renamed hours later.
        let (started_at_ms, time_source) = match (arrival, store::time_in_name(&name)) {
            // The phone's own clock, written when it started recording.
            (Some(arrival), _) => (arrival.started_at_ms, "phone"),
            (None, Some(at)) => (at, "file_name"),
            (None, None) => match evidence::created_in_file(source) {
                Some(at) if at > 946_684_800_000 && at <= modified_ms + 86_400_000 => {
                    (at, "file_metadata")
                }
                _ => (modified_ms, "file_modified"),
            },
        };
        let title = match arrival {
            Some(arrival) => store::safe_name(&arrival.title),
            None => store::title_in_name(stem),
        };

        let folder = self.library().create_folder(started_at_ms, &title)?;
        let copy = folder.join(&name);
        let copied = std::fs::copy(source, &copy)
            .with_context(|| format!("could not copy {}", source.display()))
            .and_then(|_| evidence::sha256_file(&copy))
            .and_then(|copied| {
                if copied == sha256 {
                    Ok(())
                } else {
                    Err(anyhow!(
                        "{} changed while it was being copied",
                        source.display()
                    ))
                }
            });
        if let Err(e) = copied {
            // Only the half-made copy is removed; the file given is untouched.
            let _ = std::fs::remove_file(&copy);
            let _ = std::fs::remove_dir(&folder);
            return Err(e);
        }

        let mut meeting = Meeting::new(started_at_ms, origin, &name);
        meeting.title_is_auto = title.is_empty() || arrival.is_some_and(|a| a.title_is_auto);
        meeting.title = title;
        meeting.time_source = time_source.to_string();
        meeting.source_path = match arrival {
            Some(arrival) => arrival.source.clone(),
            None => std::path::absolute(source)
                .unwrap_or_else(|_| source.to_path_buf())
                .display()
                .to_string(),
        };
        if let Some(arrival) = arrival {
            meeting.duration_ms = arrival.duration_ms;
            meeting.marks_ms = arrival.marks_ms.clone();
            meeting.mark_notes = vec![String::new(); arrival.marks_ms.len()];
            meeting.device = arrival.device.clone();
            if arrival.recovered {
                meeting.capture_note = "app_stopped".to_string();
            }
        }
        meeting.sha256 = sha256.clone();
        meeting.template_id = template_id.to_string();
        meeting.queued_at_ms = now_ms();
        self.save(&meeting, &folder)?;
        self.write_evidence(&meeting, &folder);
        self.remember(&sha256, &meeting.id);
        info!("Queued {} as {}", source.display(), meeting.id);
        self.wake();
        Ok(Imported {
            id: meeting.id,
            duplicate: false,
            sha256,
        })
    }

    pub(super) fn remember(&self, sha256: &str, id: &str) {
        let mut ledger = lock(&self.ledger);
        ledger.hashes.insert(sha256.to_string(), id.to_string());
        if let Err(e) = ledger.save(&self.data_dir) {
            warn!("Could not save the ledger: {e:#}");
        }
    }

    /// Moves a meeting's whole folder (the original recording, the
    /// transcript, the minutes) to the system's trash, where the user can
    /// still bring it back. Nothing is deleted for good. A meeting being
    /// recorded or worked on is refused. The recording is noted, so the same
    /// file in a watched folder is not brought in again.
    pub fn delete(&self, id: &str) -> Result<()> {
        let folder = self.folder(id)?;
        let meeting = Library::read(&folder)?;
        let busy = matches!(meeting.status, Status::Recording | Status::Working)
            || lock(&self.slots).get(id).is_some_and(|slot| {
                matches!(slot.summary.status, Status::Recording | Status::Working)
            });
        if busy {
            bail!("busy");
        }
        if !meeting.sha256.is_empty() {
            let mut ledger = lock(&self.ledger);
            ledger.deleted.insert(meeting.sha256.clone());
            if let Err(e) = ledger.save(&self.data_dir) {
                warn!("Could not save the ledger: {e:#}");
            }
        }
        // Out of the queue first, so nothing starts on it meanwhile.
        let slot = lock(&self.slots).remove(id);
        let in_trash = match to_trash(&folder) {
            Ok(in_trash) => in_trash,
            Err(e) => {
                // Not in the trash: the meeting stays as it was.
                if let Some(slot) = slot {
                    lock(&self.slots).insert(id.to_string(), slot);
                }
                bail!("the meeting could not be moved to the trash ({e})");
            }
        };
        // Its folder in Drive goes to Drive's trash too, so the phone's list
        // follows; the meeting here is in the system's trash, not gone.
        let from_drive = self.uploads.in_drive(id);
        if from_drive {
            self.uploads.exclude(id);
        }
        *lock(&self.trashed) = Some(Trashed {
            id: id.to_string(),
            from_drive,
            folder: folder.clone(),
            sha256: meeting.sha256.clone(),
            in_trash,
            at: Instant::now(),
        });
        info!("A meeting was moved to the trash");
        self.changed(id);
        Ok(())
    }

    /// Brings the meeting moved to the trash in the last minute back to
    /// where it was.
    pub fn undo_delete(&self) -> Result<()> {
        let Some(trashed) = lock(&self.trashed).take() else {
            bail!("nothing to bring back");
        };
        if trashed.at.elapsed() > Duration::from_secs(90) {
            bail!("too late; it is still in the trash");
        }
        if trashed.folder.exists() {
            bail!("something else is in its place");
        }
        from_trash(&trashed.folder, trashed.in_trash.as_deref())?;
        {
            let mut ledger = lock(&self.ledger);
            ledger.deleted.remove(&trashed.sha256);
            if let Err(e) = ledger.save(&self.data_dir) {
                warn!("Could not save the ledger: {e:#}");
            }
        }
        self.pick_up_library();
        if trashed.from_drive {
            self.uploads.include(&trashed.id);
        }
        info!("A meeting was brought back from the trash");
        Ok(())
    }

    /// Queues a meeting again: one that failed, or one whose minutes should
    /// be written afresh (`rewrite`).
    pub fn retry(&self, id: &str, rewrite: bool) -> Result<()> {
        let folder = self.folder(id)?;
        let mut meeting = Library::read(&folder)?;
        if matches!(meeting.status, Status::Recording | Status::Working) {
            bail!("it is being worked on right now");
        }
        if rewrite {
            meeting.minutes.clear();
            // New minutes, new tasks: changes made to the old ones are set aside.
            self.uploads.reset_edits(id, now_ms());
        }
        self.requeue(meeting, &folder, id)
    }

    /// Transcribes a meeting again from its original recording, with what is
    /// now known about it (its people, place and words), and writes its
    /// minutes afresh. The recording itself is not touched.
    pub fn retranscribe(&self, id: &str) -> Result<()> {
        let folder = self.folder(id)?;
        let mut meeting = Library::read(&folder)?;
        if matches!(meeting.status, Status::Recording | Status::Working) {
            bail!("it is being worked on right now");
        }
        meeting.transcript = None;
        meeting.stats = None;
        meeting.minutes.clear();
        // A new transcript: changes made to the old one are set aside, here and
        // on the phone.
        self.uploads.reset_edits(id, now_ms());
        self.requeue(meeting, &folder, id)
    }

    fn requeue(&self, mut meeting: Meeting, folder: &Path, id: &str) -> Result<()> {
        meeting.status = if meeting.transcript.is_some() {
            Status::Transcribed
        } else {
            Status::Queued
        };
        meeting.attempts = 0;
        meeting.minutes_attempts = 0;
        meeting.retry_at_ms = 0;
        meeting.error.clear();
        meeting.error_detail.clear();
        meeting.queued_at_ms = now_ms();
        self.save(&meeting, folder)?;
        if let Some(slot) = lock(&self.slots).get_mut(id) {
            slot.parked = false;
        }
        self.wake();
        Ok(())
    }

    pub(super) fn write_evidence(&self, meeting: &Meeting, folder: &Path) {
        let evidence = Evidence {
            file: meeting.audio_file.clone(),
            sha256: meeting.sha256.clone(),
            size_bytes: std::fs::metadata(folder.join(&meeting.audio_file)).map_or(0, |m| m.len()),
            recorded_start: store::iso_local(meeting.started_at_ms),
            recorded_end: if meeting.duration_ms > 0 {
                store::iso_local(meeting.started_at_ms + meeting.duration_ms as i64)
            } else {
                String::new()
            },
            time_source: meeting.time_source.clone(),
            duration_ms: meeting.duration_ms,
            origin: meeting.origin.as_str().to_string(),
            original_path: meeting.source_path.clone(),
            input_device: meeting.device.clone(),
            capture_note: meeting.capture_note.clone(),
            event_title: meeting
                .event
                .as_ref()
                .map_or(String::new(), |event| event.title.clone()),
            event_start: meeting
                .event
                .as_ref()
                .map_or(String::new(), |event| store::iso_local(event.start_ms)),
            app: format!(
                "{} {}",
                crate::brand::APP_NAME_ASCII,
                env!("CARGO_PKG_VERSION")
            ),
            written_at: store::iso_local(now_ms()),
        };
        if let Err(e) = evidence::write_sidecar(folder, &evidence) {
            warn!("Could not write {}: {e:#}", evidence::SIDECAR_NAME);
        }
    }
}

/// Moves a file or folder to the system's trash. On macOS through the file
/// manager itself, not by asking Finder, which would make the system ask the
/// user for permission to control Finder.
pub fn to_trash(path: &Path) -> Result<Option<PathBuf>> {
    #[cfg(target_os = "macos")]
    {
        use objc2::rc::Retained;
        use objc2_foundation::{NSFileManager, NSString, NSURL};
        let text = path
            .to_str()
            .ok_or_else(|| anyhow!("the folder's name cannot be read"))?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(text));
        let mut landed: Option<Retained<NSURL>> = None;
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, Some(&mut landed))
            .map_err(|e| anyhow!("{}", e.localizedDescription()))?;
        Ok(landed
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string())))
    }
    #[cfg(not(target_os = "macos"))]
    {
        trash::delete(path).map_err(|e| anyhow!("{e}"))?;
        Ok(None)
    }
}

/// Puts back what [`to_trash`] moved: from where it landed (macOS), or by
/// asking the system's trash for the item that was at `original`.
fn from_trash(original: &Path, in_trash: Option<&Path>) -> Result<()> {
    if let Some(in_trash) = in_trash {
        return std::fs::rename(in_trash, original)
            .map_err(|e| anyhow!("it could not be put back ({e})"));
    }
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        let items: Vec<_> = trash::os_limited::list()
            .map_err(|e| anyhow!("{e}"))?
            .into_iter()
            .filter(|item| item.original_path() == original)
            .collect();
        if items.is_empty() {
            bail!("it is no longer in the trash");
        }
        trash::os_limited::restore_all(items).map_err(|e| anyhow!("{e}"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        bail!("it is no longer in the trash")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "moves a folder to the real trash"]
    fn a_folder_goes_to_the_trash_whole_and_comes_back() {
        let home = std::env::var("HOME").unwrap();
        let dir = Path::new(&home).join(format!("seogi-trash-selftest-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("녹음.wav"), b"x").unwrap();
        let landed = to_trash(&dir).unwrap();
        assert!(!dir.exists());
        from_trash(&dir, landed.as_deref()).unwrap();
        assert!(dir.join("녹음.wav").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
