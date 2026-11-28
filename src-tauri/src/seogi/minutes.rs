//! Minutes from a transcript.
//!
//! Nobody listens to a two-hour recording again to check its minutes, so the
//! minutes have to be trusted as they come out. The language model is not
//! trusted to that degree, and everything here is arranged around that:
//!
//! 1. **Overview.** The model names the kind of recording, a title, and the
//!    speakers whose names are said aloud.
//! 2. **Extract.** The transcript is read a window at a time, and the model
//!    answers for every line of it in turn, so that no part is skipped.
//!    Every item it takes out must come with the words it rests on, quoted.
//! 3. **Check by machine.** The quote is looked up in the transcript. An item
//!    whose quote is not there is dropped; the time of an item is the time of
//!    the line its quote was found in, never the model's word. Numbers in an
//!    item must be in the lines it cites, and lines the speech model was
//!    unsure of mark the item for a look.
//! 4. **Compose.** Items that say the same thing are merged by comparing
//!    their text, not by asking the model, which merges too freely. The
//!    model writes only the one-line summary, from the checked items.
//! 5. **Verify.** The model is shown each task and decision beside the lines
//!    it cites and asked whether those lines say it.
//!
//! 5b. **Reconcile.** Entries that may say different things about the same
//!    matter (a size said twice, a choice changed, something settled and
//!    then called open) are put to the model together with what was said
//!    around them: the later word stands, a question asked about something
//!    settled does not unsettle it, and what cannot be told goes to "확인
//!    필요" as said both ways. What comes after the meeting ended (chat
//!    once the customer left) and a fact that rests on someone else's story
//!    (another site, the speaker's own home) are marked for a look by
//!    machine before that, in [`super::facts`].
//!
//! 6. **Tasks.** The tasks are read together once more: what is no promise
//!    (something done already, or said in passing) goes, the same promise
//!    told twice becomes one with every time it was said, and each gets who
//!    does it from the people in the meeting or the side that promised.
//!
//! What cannot be verified goes to "확인 필요" with its time, so a person
//! checks a few short stretches of audio instead of the whole recording:
//! at most [`MOST_CHECKS`] of them, money, dates and promises first. The
//! rest are shown faintly, or left out when the checker found no support.

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{anyhow, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;

use super::llm::{self, Provider, Secrets};
use super::templates::{SectionKind, Template, DEFAULT_TEMPLATE, OPEN_SECTION, SOLO_TEMPLATE};
use super::transcript::{clock, Segment, Transcript};
// The parts below reach these as `super::amounts` and so on, as they did
// when they were in this file.
use super::{amounts, claims, facts, feedback};

mod checks;
mod extract;
mod quotes;
mod story;
mod tasks;
#[cfg(test)]
mod test_help;
mod verify;

use checks::*;
use extract::*;
pub use quotes::locate;
use quotes::*;
use story::*;
use tasks::*;
use verify::*;

/// Characters of transcript given to the model at a time. Small enough for a
/// local model to read with care, large enough to hold a topic.
const WINDOW_CHARS: usize = 3_500;
/// Lines repeated at the start of the next window, so an exchange cut by the
/// boundary is still read whole once.
const WINDOW_OVERLAP_LINES: usize = 2;
/// A quote is looked for near the time the model gave before anywhere else.
const NEAR_MS: u64 = 120_000;
/// How much of a quote must be found in a line for a loose match.
const LOOSE_MATCH: f32 = 0.6;
/// At or above this a loose match is as good as an exact one.
const CLOSE_MATCH: f32 = 0.85;
/// The most entries put to the person to listen to. More is a list nobody
/// goes through.
pub const MOST_CHECKS: usize = 10;
/// The most tasks a meeting's minutes keep, and how many are kept before
/// light ones (a tip, a suggestion) are left to the story.
pub const MOST_TASKS: usize = 10;
const ENOUGH_TASKS: usize = 8;
/// Tries for a request whose answer could not be read.
const ATTEMPTS: usize = 2;
/// How many tasks and decisions the checker is asked about in one request,
/// at most, and how much text. A long meeting has hundreds; asked about all
/// of them at once, the question outgrows what a model reads and the answer
/// what it may write, and then nothing is checked at all.
const VERIFY_ENTRIES: usize = 20;
const VERIFY_CHARS: usize = 5_000;
/// The places an entry was said that the checker is shown. One told many
/// times over is checked against the first few.
const VERIFY_SOURCES: usize = 4;
/// The most of the items that the one-line summary is written from.
const SUMMARY_CHARS: usize = 4_000;
/// How long to wait before making a failed request again, and so how many
/// times. A long meeting takes dozens of requests; one that fails in passing
/// should not throw the answers to the others away.
const PATIENCE: [std::time::Duration; 2] = [
    std::time::Duration::from_secs(5),
    std::time::Duration::from_secs(20),
];
/// Failures that waiting does not help: too much asked at once, a model too
/// slow to answer in time, an answer already asked for twice, a provider
/// that is not set up.
const NOT_WORTH_WAITING: [&str; 6] = [
    llm::CUT_OFF,
    "did not answer within",
    "timed out",
    "JSON",
    "is not installed",
    "no API key",
];

/// Where in the recording an entry comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Source {
    /// Milliseconds into the original recording.
    pub ms: u64,
    /// The words it rests on, as transcribed.
    pub quote: String,
}

/// Why an entry should be listened to before it is relied on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CheckReason {
    /// The speech model was unsure of the words it rests on.
    UnsureSpeech,
    /// Two people were talking at once there.
    OverlappingSpeech,
    /// A number in it is not in the lines it cites.
    NumberNotInSource,
    /// Its quote was found only roughly.
    LooseQuote,
    /// The lines it cites do not clearly say it.
    WeakSupport,
    /// The lines do not say who does it.
    OwnerUnclear,
    /// The lines do not give the date it carries.
    DueUnclear,
    /// It may only have been talked about, not settled.
    NotSettled,
    /// What was said differs a little from how it is written.
    SaidDifferently,
    /// Different amounts or numbers were said for it.
    NumbersDiffer,
    /// The checker could not be asked about it.
    Unchecked,
    /// Something else said about the same matter disagrees with it, and
    /// which stands could not be told.
    Conflicting,
    /// The words it rests on may be about another site or person (a past
    /// case, the speaker's own home), not this meeting's.
    OtherSubject,
    /// An amount in it, or in the words it rests on, is written in a way no
    /// one says it: a big unit twice ("1억 5천억"), five digits before 만.
    UnitOdd,
    /// A change it states does not add up with the amounts said for it.
    SumsDisagree,
    /// It was said after the meeting seems to have ended (after the
    /// farewells, once someone had left): it may be chat, or another call.
    AfterTheEnd,
}

impl CheckReason {
    /// The reason as the minutes show it.
    pub fn korean(self) -> &'static str {
        match self {
            CheckReason::UnsureSpeech => "받아쓰기가 불확실한 구간",
            CheckReason::OverlappingSpeech => "말이 겹친 구간",
            CheckReason::NumberNotInSource => "숫자가 근거 문장에 없음",
            CheckReason::LooseQuote => "근거 문장이 정확히 일치하지 않음",
            CheckReason::WeakSupport => "근거 문장만으로는 분명하지 않음",
            CheckReason::OwnerUnclear => "누가 할지 말이 없음",
            CheckReason::DueUnclear => "기한이 말에 없음",
            CheckReason::NotSettled => "정했는지 분명하지 않음",
            CheckReason::SaidDifferently => "말한 내용과 조금 다름",
            CheckReason::NumbersDiffer => "금액이나 숫자가 다르게 나옴",
            CheckReason::Unchecked => "기계가 맞춰 보지 못함",
            CheckReason::Conflicting => "앞뒤에 다른 말이 있음",
            CheckReason::OtherSubject => "이 회의 이야기인지 분명하지 않음",
            CheckReason::UnitOdd => "금액 단위가 이상함",
            CheckReason::SumsDisagree => "금액 계산이 맞지 않음",
            CheckReason::AfterTheEnd => "회의가 끝난 뒤의 말일 수 있음",
        }
    }
}

/// One line of the minutes: a task, a decision, a fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Entry {
    pub text: String,
    /// Who does it, when the recording says; empty otherwise.
    pub owner: String,
    /// By when, in the words of the recording; empty when not said.
    pub due: String,
    pub sources: Vec<Source>,
    /// Empty when the checks found nothing to doubt.
    pub reasons: Vec<CheckReason>,
    /// A task the user has ticked off. Theirs to set; the minutes never do.
    #[serde(default)]
    pub done: bool,
    /// The checks had a small doubt about it, not worth a listen beside the
    /// few that are: it is shown faintly instead of being put to the person.
    #[serde(default)]
    pub faint: bool,
    /// The minutes' own words, when the user changed `text` on the screen;
    /// `None` while it stands as written. Absent in meetings saved before
    /// tasks could be changed.
    #[serde(default)]
    pub original: Option<String>,
    /// A task the user added: no line of the recording is behind it.
    #[serde(default)]
    pub added: bool,
    /// Names this entry for as long as it lives, whatever the user changes
    /// in it, so the phone and this computer can speak of the same task.
    /// Given when the minutes are written (and to a task the user adds);
    /// empty in meetings saved before entries had ids, which get theirs
    /// when they are first put in Drive.
    #[serde(default)]
    pub id: String,
}

/// A new entry id: 12 hex characters from the system's randomness (the
/// time, should there be none), unique enough within one meeting.
pub fn new_entry_id() -> String {
    let mut bytes = [0u8; 6];
    if getrandom::fill(&mut bytes).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        bytes.copy_from_slice(&nanos.to_le_bytes()[..6]);
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Entry {
    pub fn needs_check(&self) -> bool {
        !self.reasons.is_empty()
    }

    /// A task the user writes in themselves.
    pub fn added(text: &str) -> Self {
        Self {
            text: text.trim().to_string(),
            owner: String::new(),
            due: String::new(),
            sources: Vec::new(),
            reasons: Vec::new(),
            done: false,
            faint: false,
            original: None,
            added: true,
            id: new_entry_id(),
        }
    }

    /// Gives the entry the user's words, keeping the minutes' own beside
    /// them; words put back as written are no longer a change.
    pub fn rewrite(&mut self, text: &str) {
        let text = text.trim().to_string();
        if text == self.text {
            return;
        }
        let written = self.original.take().unwrap_or_else(|| self.text.clone());
        self.original = (written != text && !self.added).then_some(written);
        self.text = text;
    }
}

/// An entry the user took out of the minutes, kept so it can be put back
/// where it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Removed {
    /// Its place among the section's entries when it was taken out.
    pub index: u32,
    pub entry: Entry,
}

/// A part of the meeting told as a story: a short heading and a paragraph,
/// with the times in the recording its sentences rest on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StoryPart {
    pub title: String,
    pub text: String,
    pub sources: Vec<Source>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MinutesSection {
    pub key: String,
    pub title: String,
    pub kind: SectionKind,
    pub entries: Vec<Entry>,
    /// Entries the user took out, newest last. Kept apart from `entries`
    /// so that everything that reads the minutes (and an older version of
    /// the app) leaves them out. Absent in meetings saved before tasks
    /// could be taken out.
    #[serde(default)]
    pub removed: Vec<Removed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Minutes {
    pub template_id: String,
    pub template_name: String,
    /// A title from what was said, for a recording with no calendar event.
    pub title: String,
    /// One line: what this recording was about and what came of it.
    pub summary: String,
    /// How the meeting went, in a few parts: what was talked about, where
    /// views differed, how it was settled. Every sentence rests on checked
    /// items; one that does not is left out.
    #[serde(default)]
    pub story: Vec<StoryPart>,
    /// Feedback on the conversation, when the template asks for it.
    #[serde(default)]
    pub feedback: Option<super::feedback::Feedback>,
    /// The template's sections that have entries, in its order.
    pub sections: Vec<MinutesSection>,
    /// Entries the checks found nothing to doubt in.
    pub verified: u32,
    /// Entries marked for a look.
    pub to_check: u32,
    /// Items left out because the recording does not back them.
    pub dropped: u32,
    /// Entries that may say different things could not be asked about
    /// again (the request failed): they stand as written, unsettled.
    #[serde(default)]
    pub not_rechecked: bool,
    /// The model that wrote it: `ollama gemma3:12b`.
    pub written_by: String,
}

impl Minutes {
    /// Gives every entry that has none an id, those taken out included.
    /// Returns whether any was given.
    pub fn give_ids(&mut self) -> bool {
        let mut taken: std::collections::HashSet<String> = self
            .sections
            .iter()
            .flat_map(|s| s.entries.iter().chain(s.removed.iter().map(|r| &r.entry)))
            .map(|e| e.id.clone())
            .filter(|id| !id.is_empty())
            .collect();
        let mut given = false;
        for section in &mut self.sections {
            let entries = section
                .entries
                .iter_mut()
                .chain(section.removed.iter_mut().map(|r| &mut r.entry));
            for entry in entries.filter(|e| e.id.is_empty()) {
                let mut id = new_entry_id();
                while taken.contains(&id) {
                    id = new_entry_id();
                }
                taken.insert(id.clone());
                entry.id = id;
                given = true;
            }
        }
        given
    }

    /// Counts again what was checked and what is to be checked, after the
    /// user took an entry out, put it back or added one. An entry the user
    /// added was not checked against the recording, so it is neither.
    pub fn recount(&mut self) {
        let entries = || self.sections.iter().flat_map(|s| &s.entries);
        let to_check = entries().filter(|e| e.needs_check()).count() as u32;
        let checked = entries().filter(|e| !e.added).count() as u32;
        self.to_check = to_check;
        self.verified = checked.saturating_sub(to_check);
    }
}

/// What the minutes are made from.
pub struct Input<'a> {
    pub transcript: &'a Transcript,
    pub templates: &'a [Template],
    /// The template to use (from a calendar rule or the user); `None` lets
    /// the content decide.
    pub template_id: Option<&'a str>,
    /// Times the person recording pressed "중요 표시".
    pub marks_ms: &'a [u64],
    /// What the person wrote beside each mark, in the order of `marks_ms`;
    /// shorter or empty when they wrote nothing.
    pub mark_notes: &'a [String],
    /// Attendees or other names known beforehand.
    pub people: &'a [String],
    /// The customer the meeting is about, when known; empty otherwise.
    pub customer: &'a str,
    /// When the recording was made, for the model to read dates against:
    /// `2026-11-24 화요일`. Empty when unknown.
    pub recorded_on: &'a str,
    /// What the user told about this meeting (place, note, its own words),
    /// as lines to read the transcript with. Empty when nothing.
    pub context: &'a str,
    /// What the company counts as a good consultation, for the feedback.
    pub feedback_rules: &'a str,
    pub provider: &'a Provider,
    pub secrets: Secrets<'a>,
}

pub struct Output {
    pub minutes: Minutes,
    /// Names for speakers, found in what was said.
    pub speaker_names: Vec<(u32, String)>,
}

/// Writes minutes. `progress` is called with the fraction done; `Ok(None)`
/// if `cancelled` was set along the way.
pub fn write(
    input: &Input<'_>,
    progress: &dyn Fn(f32),
    cancelled: &AtomicBool,
) -> Result<Option<Output>> {
    let transcript = input.transcript;
    if transcript.segments.is_empty() {
        return Err(anyhow!("the recording has no speech to write minutes from"));
    }
    let stop = || cancelled.load(Ordering::Acquire);
    progress(0.0);

    // 1. Overview.
    let overview = overview(input)?;
    if stop() {
        return Ok(None);
    }
    let template = input
        .templates
        .iter()
        .find(|t| t.id == overview.template_id)
        .or_else(|| input.templates.iter().find(|t| t.id == DEFAULT_TEMPLATE))
        .or_else(|| input.templates.first())
        .ok_or_else(|| anyhow!("there are no minutes templates"))?;
    let mut named = transcript.clone();
    for (id, name) in &overview.speaker_names {
        if let Some(speaker) = named.speakers.iter_mut().find(|s| s.id == *id) {
            speaker.name = name.clone();
        }
    }
    // Lines the speech model made up (a broken character, a line it is
    // known to invent, foreign scraps in a Korean meeting) are not something
    // anyone said: nothing rests on them. The transcript itself keeps them.
    let korean = super::facts::korean_meeting(transcript);
    {
        let heard = named.segments.len();
        named
            .segments
            .retain(|segment| !super::facts::is_gibberish(&segment.text, korean));
        if named.segments.is_empty() {
            named.segments = transcript.segments.clone();
        } else if named.segments.len() < heard {
            info!(
                "Left {} noise lines out of the minutes",
                heard - named.segments.len()
            );
        }
    }
    let index = Index::new(&named);
    // What is said after the meeting seems to have ended is marked.
    let end_ms = super::facts::meeting_end(&named);
    progress(0.1);

    // 2. Extract, 3. check by machine.
    let windows = windows(&named, input.marks_ms, input.mark_notes, end_ms);
    let total = windows.len();
    let (answers, finished) = in_rounds(
        &windows,
        input.provider.at_once(),
        &|at, window: &Vec<Line>| extract(input, template, window, at + 1, total),
        &|answers: &[Result<Vec<RawItem>>]| {
            progress(0.1 + 0.6 * answers.len() as f32 / total.max(1) as f32);
            !stop() && answers.iter().all(Result::is_ok)
        },
    );
    let mut items: Vec<Item> = Vec::new();
    let mut dropped = 0u32;
    for answer in answers {
        for raw in answer? {
            match check_item(raw, template, &index, &named) {
                Some(mut item) => {
                    // Said after the farewells: it may be chat once the
                    // customer left, or it may be one more word of the
                    // meeting. Not stated as fact, not thrown away either.
                    if after_the_end(&item, &index, end_ms) {
                        push_reason(&mut item.entry, CheckReason::AfterTheEnd);
                    }
                    mark_elsewhere(&mut item, template, &index, &named, input.customer);
                    items.push(item);
                }
                None => dropped += 1,
            }
        }
    }
    if !finished {
        return Ok(None);
    }
    info!(
        "Extracted {} items from {} windows ({} without support)",
        items.len(),
        windows.len(),
        dropped
    );

    // 4. Compose.
    if stop() {
        return Ok(None);
    }
    let mut composed = compose(items);
    mark_sums(&mut composed.entries);
    progress(0.85);

    // 5. Verify.
    if stop() {
        return Ok(None);
    }
    let mut entries = composed.entries;
    let Some(unbacked) = verify(
        input,
        template,
        &mut entries,
        &index,
        &named,
        progress,
        cancelled,
    ) else {
        return Ok(None);
    };
    dropped += unbacked;

    // 5b. Reconcile what disagrees: the later word stands.
    if stop() {
        return Ok(None);
    }
    let (gone, not_rechecked) = reconcile(input, template, &mut entries, &index, &named);
    dropped += gone;

    // 5c. Numbers said in the meeting that disagree with each other: both
    // places are put to the person; no number is changed.
    let disagreeing = mark_disagreements(template, &mut entries, &named, end_ms);
    if disagreeing > 0 {
        info!("Numbers that disagree with each other: {disagreeing}");
    }

    // 6. Tasks: one per promise, with who does it.
    if stop() {
        return Ok(None);
    }
    // The story is told from everything checked, light tasks included:
    // what is too small for the task list still belongs to how it went.
    // What may not have been said so, or may be someone else's, is not
    // told as fact: it waits in "확인 필요".
    let told: Vec<(String, Entry)> = entries
        .iter()
        .filter(|(_, entry)| !entry.reasons.iter().any(|r| DOUBTS_NOT_TOLD.contains(r)))
        .cloned()
        .collect();
    let summary = if told.is_empty() {
        String::new()
    } else {
        summarize(input, template, &told, &overview.title).unwrap_or_else(|e| {
            warn!("Could not write the summary line ({e:#}); leaving it out");
            String::new()
        })
    };
    let mut people: Vec<String> = input.people.to_vec();
    for speaker in &named.speakers {
        if !speaker.name.starts_with("화자") && !people.contains(&speaker.name) {
            people.push(speaker.name.clone());
        }
    }
    if let Err(e) = tidy_tasks(input, template, &mut entries, &named, &people) {
        warn!("Could not tidy the tasks ({e:#}); keeping them as they were");
    }
    owners_by_name(template, &mut entries, &named, input.customer);

    // 7. The story of the meeting, from what is left.
    if stop() {
        return Ok(None);
    }
    let mut story = tell(input, template, &told, &overview.title).unwrap_or_else(|e| {
        warn!("Could not write the story ({e:#}); leaving it out");
        Story::default()
    });

    // 8. Feedback on the conversation, for a consultation.
    let feedback = if template.feedback && !stop() {
        let talk = super::talk::measure(&named);
        let line: Vec<String> = talk
            .speakers
            .iter()
            .map(|s| {
                format!(
                    "{} {:.0}% (질문 {}번, 상대 말이 끝나기 전에 이어받음 {}번)",
                    named.speaker_name(s.speaker),
                    s.share,
                    s.questions,
                    s.cut_ins
                )
            })
            .collect();
        let cited: Vec<(&str, &Entry)> = told
            .iter()
            .map(|(key, entry)| {
                let section = template
                    .section(key)
                    .map_or(key.as_str(), |s| s.title.as_str());
                (section, entry)
            })
            .collect();
        let ask = |system: &str, user: &str, schema: &Value| ask_again(input, system, user, schema);
        match super::feedback::write(&cited, input.feedback_rules, &line.join(", "), &ask) {
            Ok(feedback) => Some(feedback),
            Err(e) => {
                warn!("Could not write the feedback ({e:#}); leaving it out");
                Some(super::feedback::Feedback::default())
            }
        }
    } else {
        None
    };

    // 9. The story and the feedback, checked against the recording: what
    // the lines do not say is not told.
    let mut feedback = feedback;
    if !stop() {
        let started = std::time::Instant::now();
        let checked = check_told(
            input,
            &named,
            &mut story,
            feedback.as_mut(),
            &mut entries,
            &told,
            cancelled,
        );
        info!(
            "Checked the story and feedback: {} read, {} softened, {} left out, in {:.0}s",
            checked.read,
            checked.softened,
            checked.gone,
            started.elapsed().as_secs_f32()
        );
    }
    if let Some(feedback) = feedback.as_mut() {
        super::feedback::trim_times(feedback);
    }
    let story = story.finish();
    dropped += keep_checks(template, &mut entries, MOST_CHECKS);
    progress(1.0);

    let mut sections: Vec<MinutesSection> = Vec::new();
    for section in &template.sections {
        let in_section: Vec<Entry> = entries
            .iter()
            .filter(|(key, _)| key == &section.key)
            .map(|(_, entry)| entry.clone())
            .collect();
        if !in_section.is_empty() {
            sections.push(MinutesSection {
                key: section.key.clone(),
                title: section.title.clone(),
                kind: section.kind,
                entries: in_section,
                removed: Vec::new(),
            });
        }
    }
    let to_check = entries.iter().filter(|(_, e)| e.needs_check()).count() as u32;
    let minutes = Minutes {
        template_id: template.id.clone(),
        template_name: template.name.clone(),
        title: overview.title,
        summary,
        story,
        feedback,
        sections,
        verified: entries.len() as u32 - to_check,
        to_check,
        dropped,
        not_rechecked,
        // The model that really wrote them: a refused choice was replaced
        // by the tool's own.
        written_by: super::llm::in_use(input.provider).label(),
    };
    Ok(Some(Output {
        minutes,
        speaker_names: overview.speaker_names,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_changed_keeps_the_minutes_own_words_until_they_are_put_back() {
        let mut entry = Entry::added("처음 적은 일");
        entry.rewrite("고쳐 적은 일");
        assert_eq!(
            entry.original, None,
            "a task the user added has no words of the minutes"
        );
        let mut entry = Entry {
            added: false,
            id: String::new(),
            ..Entry::added("도면을 보내기로 했다.")
        };
        entry.rewrite("도면을 금요일에 보내기로 했다.");
        entry.rewrite("도면을 금요일 오전에 보내기로 했다.");
        assert_eq!(entry.original.as_deref(), Some("도면을 보내기로 했다."));
        entry.rewrite(" 도면을 보내기로 했다. ");
        assert_eq!(entry.original, None);
        assert_eq!(entry.text, "도면을 보내기로 했다.");
    }

    #[test]
    fn every_entry_gets_an_id_once_and_keeps_it() {
        let entry = |text: &str, id: &str| Entry {
            id: id.to_string(),
            ..Entry::added(text)
        };
        let mut minutes: Minutes = serde_json::from_value(serde_json::json!({
            "template_id": "meeting", "template_name": "회의", "title": "", "summary": "",
            "sections": [], "verified": 0, "to_check": 0, "dropped": 0, "written_by": ""
        }))
        .unwrap();
        minutes.sections.push(MinutesSection {
            key: "actions".into(),
            title: "할 일".into(),
            kind: SectionKind::Actions,
            entries: vec![entry("가", ""), entry("나", "kept"), entry("다", "")],
            removed: vec![Removed {
                index: 0,
                entry: entry("라", ""),
            }],
        });
        assert!(minutes.give_ids());
        let ids: Vec<String> = minutes.sections[0]
            .entries
            .iter()
            .chain(minutes.sections[0].removed.iter().map(|r| &r.entry))
            .map(|e| e.id.clone())
            .collect();
        assert_eq!(ids[1], "kept");
        assert!(ids.iter().all(|id| !id.is_empty()));
        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), 4);
        // Given once: nothing changes the second time, nor when a task is
        // changed.
        assert!(!minutes.give_ids());
        minutes.sections[0].entries[0].rewrite("가 고침");
        assert_eq!(minutes.sections[0].entries[0].id, ids[0]);
        // A task the user adds has its own at once; an old record has none.
        assert_eq!(Entry::added("새 일").id.len(), 12);
        let old: Entry = serde_json::from_value(serde_json::json!({
            "text": "옛 일", "owner": "", "due": "", "sources": [], "reasons": []
        }))
        .unwrap();
        assert!(old.id.is_empty());
    }
}
