//! What the user tells the app about a meeting: its details, tasks,
//! lines, words and speakers' names, and questions about it.

use super::*;

impl Secretary {
    /// Answers a question about a meeting from its transcript, with the
    /// words the answer rests on.
    pub fn ask(&self, id: &str, question: &str) -> Result<super::ask::Answer> {
        let meeting = self.meeting(id)?;
        // The lines as the user fixed them.
        let transcript = meeting
            .transcript
            .as_ref()
            .map(super::transcript::Transcript::as_read)
            .ok_or_else(|| anyhow!("it has not been transcribed yet"))?;
        let keychain = |name: &str| super::secrets::get(name);
        super::ask::answer(&transcript, question, &self.config().summary, &keychain)
    }

    // ----- what the user tells the app --------------------------------------

    pub(super) fn editable(&self, id: &str) -> Result<(Meeting, PathBuf)> {
        let folder = self.folder(id)?;
        let meeting = Library::read(&folder)?;
        if meeting.status == Status::Working {
            bail!("it is being worked on right now");
        }
        Ok((meeting, folder))
    }

    /// Sets what the user knows about a meeting: its title, kind, customer,
    /// who was there. Used for the minutes when they are written (again).
    pub fn set_meeting_info(&self, id: &str, info: MeetingInfo) -> Result<()> {
        let _editing = lock(&self.edit_lock);
        let (mut meeting, folder) = self.editable(id)?;
        if let Some(title) = info.title {
            let title = store::safe_name(&title);
            meeting.title_is_auto = title.is_empty();
            meeting.title = title;
        }
        if let Some(template_id) = info.template_id {
            meeting.template_id = template_id.trim().to_string();
        }
        if let Some(customer) = info.customer {
            meeting.customer = customer.trim().to_string();
        }
        let tidy = |names: Vec<String>| -> Vec<String> { tidy_names(names) };
        if let Some(people) = info.people {
            meeting.people = tidy(people);
        }
        if let Some(place) = info.place {
            meeting.place = place.trim().to_string();
        }
        if let Some(note) = info.note {
            meeting.note = note.trim().to_string();
        }
        if let Some(terms) = info.terms {
            meeting.terms = tidy(terms);
        }
        self.save_edit(&meeting, &folder)
    }

    /// Candidate words for a meeting from its documents, without those it
    /// has already or the vocabulary knows.
    pub fn terms_from_documents(
        &self,
        id: &str,
        paths: &[PathBuf],
    ) -> Result<super::documents::TermsFound> {
        let meeting = self.meeting(id)?;
        let config = self.config();
        let vocabulary = Vocabulary::load(&config.vocabulary(&self.data_dir)).unwrap_or_default();
        let mut known: Vec<String> = meeting.terms.clone();
        known.extend(meeting.people.iter().cloned());
        let keychain = |name: &str| super::secrets::get(name);
        let mut found = super::documents::terms_from(paths, &config.summary, &keychain, &known)?;
        found.terms.retain(|term| !vocabulary.knows(term));
        Ok(found)
    }

    /// Adds words to the user's vocabulary, which is for every meeting.
    /// Returns how many were new.
    pub fn add_to_vocabulary(&self, terms: &[String]) -> Result<usize> {
        let path = self.config().vocabulary(&self.data_dir);
        let heading = format!(
            "서기에서 자료로 추가 ({})",
            chrono::Local::now().format("%Y-%m-%d")
        );
        super::vocab::append_terms(&path, terms, &heading)
    }

    /// Writes a note beside a mark of the recording in progress.
    pub fn set_mark_note(&self, index: usize, note: &str) -> Result<()> {
        let folder = lock(&self.recording)
            .as_ref()
            .map(|active| active.folder.clone())
            .ok_or_else(|| anyhow!("no recording is in progress"))?;
        let mut meeting = Library::read(&folder)?;
        if index >= meeting.marks_ms.len() {
            bail!("there is no such mark");
        }
        meeting
            .mark_notes
            .resize(meeting.marks_ms.len(), String::new());
        meeting.mark_notes[index] = note.trim().to_string();
        self.save(&meeting, &folder)
    }

    /// Ticks a task off, or back on.
    pub fn set_action_done(&self, id: &str, section: &str, index: usize, done: bool) -> Result<()> {
        let _editing = lock(&self.edit_lock);
        let (mut meeting, folder) = self.editable(id)?;
        let entry = meeting
            .minutes
            .first_mut()
            .and_then(|minutes| minutes.sections.iter_mut().find(|s| s.key == section))
            .and_then(|section| section.entries.get_mut(index))
            .ok_or_else(|| anyhow!("there is no such task"))?;
        entry.done = done;
        self.save_edit(&meeting, &folder)
    }

    /// The task section `section` of a meeting's minutes, to change.
    fn task_section<'a>(
        meeting: &'a mut Meeting,
        section: &str,
    ) -> Result<&'a mut minutes::MinutesSection> {
        meeting
            .minutes
            .first_mut()
            .and_then(|minutes| minutes.sections.iter_mut().find(|s| s.key == section))
            .ok_or_else(|| anyhow!("there is no such list of tasks"))
    }

    /// Gives a task of the minutes the user's words; the minutes' own are
    /// kept beside them.
    pub fn edit_task(&self, id: &str, section: &str, index: usize, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            bail!("a task needs words");
        }
        let _editing = lock(&self.edit_lock);
        let (mut meeting, folder) = self.editable(id)?;
        Self::task_section(&mut meeting, section)?
            .entries
            .get_mut(index)
            .ok_or_else(|| anyhow!("there is no such task"))?
            .rewrite(text);
        self.save_edit(&meeting, &folder)
    }

    /// Takes a task out of the minutes; it is kept so it can be put back.
    pub fn remove_task(&self, id: &str, section: &str, index: usize) -> Result<()> {
        let _editing = lock(&self.edit_lock);
        let (mut meeting, folder) = self.editable(id)?;
        let tasks = Self::task_section(&mut meeting, section)?;
        if index >= tasks.entries.len() {
            bail!("there is no such task");
        }
        let entry = tasks.entries.remove(index);
        tasks.removed.push(minutes::Removed {
            index: index as u32,
            entry,
        });
        if let Some(minutes) = meeting.minutes.first_mut() {
            minutes.recount();
        }
        self.save_edit(&meeting, &folder)
    }

    /// Puts a task taken out back where it was; `removed` is its place
    /// among the ones taken out of that section.
    pub fn restore_task(&self, id: &str, section: &str, removed: usize) -> Result<()> {
        let _editing = lock(&self.edit_lock);
        let (mut meeting, folder) = self.editable(id)?;
        let tasks = Self::task_section(&mut meeting, section)?;
        if removed >= tasks.removed.len() {
            bail!("there is no such task taken out");
        }
        let back = tasks.removed.remove(removed);
        let at = (back.index as usize).min(tasks.entries.len());
        tasks.entries.insert(at, back.entry);
        if let Some(minutes) = meeting.minutes.first_mut() {
            minutes.recount();
        }
        self.save_edit(&meeting, &folder)
    }

    /// Adds a task the user writes to the end of a task section. With no
    /// such section yet (the minutes found no task), the template's first
    /// task section is made, in its place among the others.
    pub fn add_task(&self, id: &str, section: &str, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            bail!("a task needs words");
        }
        let _editing = lock(&self.edit_lock);
        let (mut meeting, folder) = self.editable(id)?;
        let all_templates = templates::load(&self.data_dir.join("templates"));
        let minutes = meeting
            .minutes
            .first_mut()
            .ok_or_else(|| anyhow!("the minutes are not written yet"))?;
        let template = all_templates
            .iter()
            .find(|template| template.id == minutes.template_id);
        let at = match minutes.sections.iter().position(|s| s.key == section) {
            Some(at) => at,
            None => {
                let made = template
                    .and_then(|template| {
                        template
                            .sections
                            .iter()
                            .find(|s| s.kind == templates::SectionKind::Actions)
                    })
                    .map(|s| (s.key.clone(), s.title.clone()))
                    .unwrap_or_else(|| ("actions".to_string(), "할 일".to_string()));
                if let Some(at) = minutes.sections.iter().position(|s| s.key == made.0) {
                    at
                } else {
                    // Before the first section that comes after it in the
                    // template; at the top when the template does not say.
                    let order = |key: &str| {
                        template.and_then(|t| t.sections.iter().position(|s| s.key == key))
                    };
                    let mine = order(&made.0).unwrap_or(0);
                    let at = minutes
                        .sections
                        .iter()
                        .position(|s| order(&s.key).is_none_or(|o| o > mine))
                        .unwrap_or(minutes.sections.len());
                    minutes.sections.insert(
                        at,
                        minutes::MinutesSection {
                            key: made.0,
                            title: made.1,
                            kind: templates::SectionKind::Actions,
                            entries: Vec::new(),
                            removed: Vec::new(),
                        },
                    );
                    at
                }
            }
        };
        minutes.sections[at]
            .entries
            .push(minutes::Entry::added(text));
        minutes.recount();
        self.save_edit(&meeting, &folder)
    }

    /// Gives a stretch of the transcript (one paragraph on the screen, from
    /// the line starting at `from_ms` to `to_ms`) the user's words. The
    /// words as transcribed are kept, and `전사.txt` keeps them.
    pub fn edit_lines(&self, id: &str, from_ms: u64, to_ms: u64, text: &str) -> Result<()> {
        let _editing = lock(&self.edit_lock);
        let (mut meeting, folder) = self.editable(id)?;
        meeting
            .transcript
            .as_mut()
            .ok_or_else(|| anyhow!("there is no transcript"))?
            .edit_lines(from_ms, to_ms, text)
            .map_err(|e| anyhow!(e))?;
        self.save_edit(&meeting, &folder)
    }

    /// How many places in a meeting fixing the misheard words `from`,
    /// picked at character `at` of the line starting at `at_ms`, as `to`
    /// would fix: the picked one and the others the vocabulary's safe rules
    /// would (`to` empty while the user has not written it yet).
    pub fn word_places(
        &self,
        id: &str,
        at_ms: u64,
        at: usize,
        from: &str,
        to: &str,
    ) -> Result<u32> {
        let meeting = self.meeting(id)?;
        let transcript = meeting
            .transcript
            .as_ref()
            .ok_or_else(|| anyhow!("there is no transcript"))?;
        let vocabulary =
            Vocabulary::load(&self.config().vocabulary(&self.data_dir)).unwrap_or_default();
        transcript
            .word_places(at_ms, at, from, to, &vocabulary)
            .map_err(|e| anyhow!(e))
    }

    /// Reads the misheard words `from` (picked at character `at` of the
    /// line starting at `at_ms`) as `to` everywhere in the meeting they
    /// safely can be. The words as transcribed and `전사.txt` stay. With
    /// `remember`, the vocabulary learns `from` as a way `to` is misheard,
    /// with this meeting and today, so meetings from now on are fixed as
    /// they are transcribed. The minutes are not written again.
    pub fn fix_word(
        &self,
        id: &str,
        at_ms: u64,
        at: usize,
        from: &str,
        to: &str,
        remember: bool,
    ) -> Result<WordFixOutcome> {
        let path = self.config().vocabulary(&self.data_dir);
        let vocabulary = Vocabulary::load(&path).unwrap_or_default();
        let _editing = lock(&self.edit_lock);
        let (mut meeting, folder) = self.editable(id)?;
        let elsewhere = Vocabulary::one_correction(from, to, &vocabulary);
        let fixed = meeting
            .transcript
            .as_mut()
            .ok_or_else(|| anyhow!("there is no transcript"))?
            .fix_word(at_ms, at, from, to, &elsewhere)
            .map_err(|e| anyhow!(e))?;
        self.save_edit(&meeting, &folder)?;
        let remembered = remember
            && super::vocab::add_misheard(
                &path,
                to,
                from,
                &chrono::Local::now().format("%Y-%m-%d").to_string(),
                &meeting.title,
            )
            .unwrap_or_else(|e| {
                warn!("Could not add a misheard word to the vocabulary: {e:#}");
                false
            });
        Ok(WordFixOutcome {
            places: fixed.places,
            before: fixed.before,
            remembered,
        })
    }

    /// Undoes a fix right after it was made: the lines as they were
    /// (`WordFixOutcome::before`), and, when the vocabulary learnt `from`
    /// with it, takes that out again.
    pub fn unfix_words(
        &self,
        id: &str,
        before: &[super::transcript::Segment],
        from: &str,
        to: &str,
        forget: bool,
    ) -> Result<()> {
        let _editing = lock(&self.edit_lock);
        let (mut meeting, folder) = self.editable(id)?;
        meeting
            .transcript
            .as_mut()
            .ok_or_else(|| anyhow!("there is no transcript"))?
            .put_back_lines(before)
            .map_err(|e| anyhow!(e))?;
        self.save_edit(&meeting, &folder)?;
        if forget {
            super::vocab::remove_misheard(&self.config().vocabulary(&self.data_dir), to, from)?;
        }
        Ok(())
    }

    /// Reads the words fixed from `from` to `to` in one paragraph on the
    /// screen (from the line starting at `from_ms` up to `to_ms`) as
    /// transcribed again. The vocabulary is left as it is.
    pub fn unfix_word(
        &self,
        id: &str,
        from_ms: u64,
        to_ms: u64,
        from: &str,
        to: &str,
    ) -> Result<()> {
        let _editing = lock(&self.edit_lock);
        let (mut meeting, folder) = self.editable(id)?;
        let undone = meeting
            .transcript
            .as_mut()
            .ok_or_else(|| anyhow!("there is no transcript"))?
            .unfix_word(from_ms, to_ms, from, to);
        if undone == 0 {
            bail!("there is no such fix there");
        }
        self.save_edit(&meeting, &folder)
    }

    /// The user's vocabulary as one list, for the settings.
    pub fn vocabulary_words(&self) -> Result<super::vocab::WordList> {
        super::vocab::list(&self.config().vocabulary(&self.data_dir))
    }

    /// Adds or changes a word of the user's vocabulary (`old` is its name
    /// before, when it is renamed), with the ways it is misheard.
    pub fn save_word(
        &self,
        old: Option<&str>,
        term: &str,
        misheard: &[super::vocab::Misheard],
    ) -> Result<()> {
        super::vocab::save_word(
            &self.config().vocabulary(&self.data_dir),
            old,
            term,
            misheard,
            &chrono::Local::now().format("%Y-%m-%d").to_string(),
        )
    }

    /// Takes a word out of the user's vocabulary.
    pub fn remove_word(&self, term: &str) -> Result<()> {
        super::vocab::remove_word(&self.config().vocabulary(&self.data_dir), term)
    }
}

/// Names as they are kept: composed the one way (a name typed and the same
/// name from a file can differ in how their Korean letters are encoded),
/// spaces tidied, and each once, whatever its case.
pub fn tidy_names(names: Vec<String>) -> Vec<String> {
    use unicode_normalization::UnicodeNormalization;
    let mut kept: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for name in names {
        let name: String = name.nfc().collect::<String>();
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        let key = name.to_lowercase();
        if !name.is_empty() && !seen.contains(&key) {
            seen.push(key);
            kept.push(name);
        }
    }
    kept
}

/// After a naming in the transcript that covered `from..to` (`old` was the
/// speaker's name before it), the tasks given to `old` in that part are
/// given to `name`. A task stays with `old` when the line it rests on is
/// said by someone still called `old`: that line was moved to that person
/// (#34), so the task is theirs.
pub(super) fn owners_follow(meeting: &mut Meeting, old: &str, name: &str, from: u64, to: u64) {
    let transcript = meeting.transcript.as_ref();
    let said_by_old = |at: u64| {
        transcript.is_some_and(|t| {
            t.segments
                .iter()
                .rev()
                .find(|segment| segment.start_ms <= at)
                .and_then(|line| t.speakers.iter().find(|s| s.id == line.speaker))
                .is_some_and(|speaker| speaker.name == old)
        })
    };
    for minutes in &mut meeting.minutes {
        for section in &mut minutes.sections {
            for item in &mut section.entries {
                let at = item.sources.first().map(|source| source.ms);
                let in_part = (from..to).contains(&at.unwrap_or(0));
                if item.owner == old && in_part && !at.is_some_and(said_by_old) {
                    item.owner = name.to_string();
                }
            }
        }
    }
}

/// One task of the minutes as a naming keeps it: which task (its text and
/// the moment it rests on) and who was given it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct KeptTask {
    pub text: String,
    /// The first moment the task rests on, when it has one.
    pub at_ms: Option<u64>,
    pub owner: String,
}

/// Who said what and who was given each task, kept by the screen around a
/// naming of speakers, so the naming can be put back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Naming {
    pub speakers: Vec<Speaker>,
    /// Every line of the transcript, in order.
    pub lines: Vec<LineSpeaker>,
    /// Every task, in order: each minutes, its sections, their entries.
    pub tasks: Vec<KeptTask>,
}

/// Who said what and who was given each task in `meeting` now.
fn naming_of(meeting: &Meeting) -> Naming {
    let transcript = meeting.transcript.as_ref();
    Naming {
        speakers: transcript.map(|t| t.speakers.clone()).unwrap_or_default(),
        lines: transcript
            .map(|t| {
                t.segments
                    .iter()
                    .map(|s| LineSpeaker {
                        start_ms: s.start_ms,
                        speaker: s.speaker,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        tasks: meeting
            .minutes
            .iter()
            .flat_map(|m| &m.sections)
            .flat_map(|section| &section.entries)
            .map(|entry| KeptTask {
                text: entry.text.clone(),
                at_ms: entry.sources.first().map(|source| source.ms),
                owner: entry.owner.clone(),
            })
            .collect(),
    }
}

/// Puts `kept` (taken just before a naming) back into `meeting`, only when
/// the meeting is still exactly `now` (taken just after it): then what is
/// undone is that naming and nothing else. Nothing changes otherwise.
pub(super) fn put_back_naming(meeting: &mut Meeting, kept: &Naming, now: &Naming) -> Result<()> {
    if naming_of(meeting) != *now {
        bail!("who said what has changed since");
    }
    let same_task = |a: &KeptTask, b: &KeptTask| a.text == b.text && a.at_ms == b.at_ms;
    if kept.tasks.len() != now.tasks.len()
        || kept
            .tasks
            .iter()
            .zip(&now.tasks)
            .any(|(a, b)| !same_task(a, b))
    {
        bail!("the minutes have changed since");
    }
    let mut transcript = meeting
        .transcript
        .clone()
        .ok_or_else(|| anyhow!("it has not been transcribed yet"))?;
    transcript
        .put_back_speakers(&kept.speakers, &kept.lines)
        .map_err(|e| anyhow!(e))?;
    meeting.transcript = Some(transcript);
    let entries = meeting
        .minutes
        .iter_mut()
        .flat_map(|m| m.sections.iter_mut())
        .flat_map(|section| section.entries.iter_mut());
    for (entry, task) in entries.zip(&kept.tasks) {
        entry.owner = task.owner.clone();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_naming_is_put_back_with_the_tasks_or_not_at_all() {
        use crate::seogi::minutes::{Entry, Minutes, MinutesSection, Source};
        use crate::seogi::templates::SectionKind;
        use crate::seogi::transcript::{Segment, Transcript};
        let task = |owner: &str, ms: u64| Entry {
            text: "도면 다시 그려 보내기".into(),
            owner: owner.into(),
            due: String::new(),
            sources: vec![Source {
                ms,
                quote: "도면".into(),
            }],
            reasons: vec![],
            done: false,
            faint: false,
            original: None,
            added: false,
            id: String::new(),
        };
        let mut meeting = Meeting::new(0, store::Origin::Recorded, "녹음.wav");
        meeting.transcript = Some(Transcript {
            segments: [(0, 1), (1_000, 2), (2_000, 1)]
                .iter()
                .map(|&(start_ms, speaker)| Segment {
                    start_ms,
                    end_ms: start_ms + 900,
                    speaker,
                    text: "말".into(),
                    ..Default::default()
                })
                .collect(),
            speakers: vec![
                Speaker {
                    id: 1,
                    name: "화자 1".into(),
                },
                Speaker {
                    id: 2,
                    name: "화자 2".into(),
                },
            ],
            ..Default::default()
        });
        meeting.minutes = vec![Minutes {
            template_id: "consult".into(),
            template_name: "상담".into(),
            title: String::new(),
            summary: String::new(),
            story: vec![],
            feedback: None,
            sections: vec![MinutesSection {
                key: "our_tasks".into(),
                title: "우리 숙제".into(),
                kind: SectionKind::Actions,
                entries: vec![task("화자 1", 0), task("화자 2", 1_000)],
                removed: Vec::new(),
            }],
            verified: 0,
            to_check: 0,
            dropped: 0,
            not_rechecked: false,
            written_by: String::new(),
        }];
        let before = meeting.clone();
        let kept = naming_of(&meeting);
        // Named as the screen does it, tasks following.
        let named = meeting.transcript.as_mut().unwrap();
        named.name_speaker(1, "김서윤 실장", NameScope::All, 0, 0);
        meeting.minutes[0].sections[0].entries[0].owner = "김서윤 실장".into();
        let after = meeting.clone();
        let now = naming_of(&meeting);
        put_back_naming(&mut meeting, &kept, &now).unwrap();
        assert_eq!(meeting, before);

        // Refused, changing nothing: the case and what goes wrong.
        let refused = |change: &dyn Fn(&mut Meeting)| {
            let mut changed = after.clone();
            change(&mut changed);
            let untouched = changed.clone();
            assert!(put_back_naming(&mut changed, &kept, &now).is_err());
            assert_eq!(changed, untouched);
        };
        // Another name changed since.
        refused(&|m| {
            m.transcript.as_mut().unwrap().speakers[1].name = "고객".into();
        });
        // Minutes written again with as many tasks, but other ones: the old
        // owners would land on the wrong tasks.
        refused(&|m| {
            m.minutes[0].sections[0].entries[0].text = "견적서 다시 보내기".into();
        });
        refused(&|m| {
            m.minutes[0].sections[0].entries[1].sources[0].ms = 5_000;
        });
        // Fewer tasks.
        refused(&|m| {
            m.minutes[0].sections[0].entries.pop();
        });
        // A task given to someone else by hand since.
        refused(&|m| {
            m.minutes[0].sections[0].entries[1].owner = "박도현 소장".into();
        });
    }

    /// #34: a line answered with the speaker's own name is moved to a
    /// speaker of its own, then the speaker is named someone else. The task
    /// resting on the moved line stays with the person it was moved to;
    /// the speaker's other tasks follow the new name.
    #[test]
    fn a_task_on_a_moved_line_stays_when_the_speaker_is_named_again() {
        use crate::seogi::minutes::{Entry, Minutes, MinutesSection, Source};
        use crate::seogi::templates::SectionKind;
        use crate::seogi::transcript::{Segment, Transcript};
        let task = |owner: &str, ms: u64| Entry {
            text: format!("일 {ms}"),
            owner: owner.into(),
            due: String::new(),
            sources: vec![Source {
                ms,
                quote: "말".into(),
            }],
            reasons: vec![],
            done: false,
            faint: false,
            original: None,
            added: false,
            id: String::new(),
        };
        let mut meeting = Meeting::new(0, store::Origin::Recorded, "녹음.wav");
        meeting.transcript = Some(Transcript {
            segments: [(0, 2), (1_000, 2), (2_000, 1)]
                .iter()
                .map(|&(start_ms, speaker)| Segment {
                    start_ms,
                    end_ms: start_ms + 900,
                    speaker,
                    text: "말".into(),
                    ..Default::default()
                })
                .collect(),
            speakers: vec![
                Speaker {
                    id: 1,
                    name: "화자 1".into(),
                },
                Speaker {
                    id: 2,
                    name: "지우".into(),
                },
            ],
            ..Default::default()
        });
        meeting.minutes = vec![Minutes {
            template_id: "consult".into(),
            template_name: "상담".into(),
            title: String::new(),
            summary: String::new(),
            story: vec![],
            feedback: None,
            sections: vec![MinutesSection {
                key: "our_tasks".into(),
                title: "우리 숙제".into(),
                kind: SectionKind::Actions,
                entries: vec![task("지우", 0), task("지우", 1_000), task("지우", 2_000)],
                removed: Vec::new(),
            }],
            verified: 0,
            to_check: 0,
            dropped: 0,
            not_rechecked: false,
            written_by: String::new(),
        }];
        // As the card does it: the line answered "지우" is moved first, then
        // the speaker is named "하린" throughout.
        let name = |meeting: &mut Meeting, name: &str, scope, from_ms, to_ms| {
            let (old, from, to) = meeting
                .transcript
                .as_mut()
                .unwrap()
                .name_speaker(2, name, scope, from_ms, to_ms)
                .unwrap();
            owners_follow(meeting, &old, name, from, to);
        };
        name(&mut meeting, "지우", NameScope::Here, 0, 1_000);
        name(&mut meeting, "하린", NameScope::All, 0, 0);
        let owners: Vec<&str> = meeting.minutes[0].sections[0]
            .entries
            .iter()
            .map(|entry| entry.owner.as_str())
            .collect();
        // The moved line's task stays; the speaker's own follows; a task
        // given to "지우" on someone else's line follows as before.
        assert_eq!(owners, ["지우", "하린", "하린"]);
        let transcript = meeting.transcript.as_ref().unwrap();
        let said_by = |ms: u64| {
            let line = transcript
                .segments
                .iter()
                .find(|s| s.start_ms == ms)
                .unwrap();
            let who = transcript
                .speakers
                .iter()
                .find(|s| s.id == line.speaker)
                .unwrap();
            who.name.clone()
        };
        assert_eq!(said_by(0), "지우");
        assert_eq!(said_by(1_000), "하린");
    }

    #[test]
    fn the_same_name_is_kept_once_however_it_was_written() {
        let decomposed: String = "보리"
            .chars()
            .flat_map(|c| {
                use unicode_normalization::UnicodeNormalization;
                c.to_string().nfd().collect::<Vec<_>>()
            })
            .collect();
        assert_ne!(decomposed, "보리");
        assert_eq!(
            tidy_names(vec![
                "보리".into(),
                " 민트 ".into(),
                "고객".into(),
                "고객".into(),
                decomposed,
                "Jamie  Lee".into(),
                "jamie lee".into(),
                " ".into(),
            ]),
            ["보리", "민트", "고객", "Jamie Lee"]
        );
    }
}
