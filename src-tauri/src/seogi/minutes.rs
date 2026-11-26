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
//!    (another site, the speaker's own home) are marked or left out by
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
}

impl Entry {
    pub fn needs_check(&self) -> bool {
        !self.reasons.is_empty()
    }
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
    /// The model that wrote it: `ollama gemma3:12b`.
    pub written_by: String,
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
    // Lines the speech model made up in a Korean meeting (foreign letters,
    // broken characters) are not something anyone said: nothing rests on
    // them. The transcript itself keeps them.
    if korean(transcript) {
        let heard = named.segments.len();
        named
            .segments
            .retain(|segment| !super::facts::is_gibberish(&segment.text));
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
    // What is said after the meeting ended is not the meeting's.
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
                Some(item) if after_the_end(&item, &index, end_ms) => dropped += 1,
                Some(mut item) => {
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
    dropped += reconcile(input, template, &mut entries, &index, &named);

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
    dropped += keep_checks(template, &mut entries, MOST_CHECKS);

    // 7. The story of the meeting, from what is left.
    if stop() {
        return Ok(None);
    }
    let story = tell(input, template, &told, &overview.title).unwrap_or_else(|e| {
        warn!("Could not write the story ({e:#}); leaving it out");
        Vec::new()
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
        // The model that really wrote them: a refused choice was replaced
        // by the tool's own.
        written_by: super::llm::in_use(input.provider).label(),
    };
    Ok(Some(Output {
        minutes,
        speaker_names: overview.speaker_names,
    }))
}

// ---------------------------------------------------------------------------
// The transcript, indexed for looking quotes up
// ---------------------------------------------------------------------------

/// Letters and digits only, lowercased: how text is compared.
fn bare(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Where in the transcript `quote` was said: the start of the line it is
/// in, and how closely it matches (1.0 for word for word). `None` when
/// nothing like it was said.
pub fn locate(transcript: &Transcript, quote: &str) -> Option<(u64, f32)> {
    let index = Index::new(transcript);
    let (lines, score) = index.find(quote, None)?;
    Some((transcript.segments.get(*lines.start())?.start_ms, score))
}

struct Index<'a> {
    segments: &'a [Segment],
    /// Every segment's bare text, joined.
    joined: Vec<char>,
    /// For each char of `joined`, the segment it belongs to.
    owner: Vec<usize>,
    /// `joined` as a string, for looking names up.
    text: String,
    /// The confidence below which a line is among the least sure tenth.
    unsure_below: f32,
}

impl<'a> Index<'a> {
    fn new(transcript: &'a Transcript) -> Self {
        let mut joined = Vec::new();
        let mut owner = Vec::new();
        for (index, segment) in transcript.segments.iter().enumerate() {
            for c in bare(&segment.text).chars() {
                joined.push(c);
                owner.push(index);
            }
        }
        let mut confidences: Vec<f32> = transcript
            .segments
            .iter()
            .filter_map(|segment| segment.confidence)
            .collect();
        confidences.sort_by(f32::total_cmp);
        // Only a recording long enough to have a "least sure tenth" gets one,
        // and only lines that are also not sure in absolute terms count.
        let unsure_below = if confidences.len() >= 20 {
            confidences[confidences.len() / 10].min(0.75)
        } else {
            0.0
        };
        Self {
            segments: &transcript.segments,
            text: joined.iter().collect(),
            joined,
            owner,
            unsure_below,
        }
    }

    /// The segments `quote` is in, and how well it matched (1 for exact).
    /// Looked for near `near_ms` first, then anywhere, then loosely.
    fn find(
        &self,
        quote: &str,
        near_ms: Option<u64>,
    ) -> Option<(std::ops::RangeInclusive<usize>, f32)> {
        let needle: Vec<char> = bare(quote).chars().collect();
        if needle.len() < 2 {
            return None;
        }
        let mut hits: Vec<usize> = Vec::new();
        if needle.len() <= self.joined.len() {
            for start in 0..=self.joined.len() - needle.len() {
                if self.joined[start..start + needle.len()] == needle[..] {
                    hits.push(start);
                }
            }
        }
        if !hits.is_empty() {
            // The hit nearest the time the model gave; else the first.
            let best = match near_ms {
                Some(near) => hits
                    .iter()
                    .copied()
                    .min_by_key(|&start| self.segments[self.owner[start]].start_ms.abs_diff(near))
                    .unwrap_or(hits[0]),
                None => hits[0],
            };
            let first = self.owner[best];
            let last = self.owner[best + needle.len() - 1];
            return Some((first..=last, 1.0));
        }

        // Loosely: the line (near the given time if any) sharing the most of
        // the quote's four-character pieces.
        if needle.len() < 6 {
            return None;
        }
        let pieces: HashSet<&[char]> = needle.windows(4).collect();
        let mut best: Option<(usize, f32)> = None;
        for (index, segment) in self.segments.iter().enumerate() {
            let text: Vec<char> = bare(&segment.text).chars().collect();
            if text.len() < 4 {
                continue;
            }
            let have: HashSet<&[char]> = text.windows(4).collect();
            let shared = pieces.iter().filter(|piece| have.contains(*piece)).count();
            let mut score = shared as f32 / pieces.len() as f32;
            // Between equals, the line near the stated time.
            if near_ms.is_some_and(|near| segment.start_ms.abs_diff(near) <= NEAR_MS) {
                score += 0.05;
            }
            if best.is_none_or(|(_, top)| score > top) {
                best = Some((index, score));
            }
        }
        best.filter(|(_, score)| *score >= LOOSE_MATCH)
            .map(|(index, score)| (index..=index, score.min(0.99)))
    }

    /// The text of segments `range`, widened by `around` lines each side.
    fn text_around(&self, range: &std::ops::RangeInclusive<usize>, around: usize) -> String {
        let first = range.start().saturating_sub(around);
        let last = (range.end() + around).min(self.segments.len() - 1);
        self.segments[first..=last]
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

// ---------------------------------------------------------------------------
// Windows of transcript, as the model reads them
// ---------------------------------------------------------------------------

/// One line of a window as the model reads it.
struct Line {
    /// When the line starts; `None` for a "중요 표시" marker, which is not a
    /// line of speech and gets no number.
    ms: Option<u64>,
    text: String,
}

/// The line put where the meeting ended, when the recording goes on after.
const AFTER_THE_END: &str = "[회의 끝: 여기부터는 회의가 끝난 뒤의 말입니다]";

/// The transcript as lines `[hh:mm:ss] name: text`, with the marks in place
/// and, at `end_ms`, the line saying the meeting is over; cut into windows.
fn windows(
    transcript: &Transcript,
    marks_ms: &[u64],
    mark_notes: &[String],
    end_ms: Option<u64>,
) -> Vec<Vec<Line>> {
    // A mark as the model reads it, with the note written beside it if any.
    let mut marks: Vec<(u64, String)> = marks_ms
        .iter()
        .enumerate()
        .map(|(index, &ms)| {
            let note = mark_notes.get(index).map_or("", |note| note.trim());
            let text = if note.is_empty() {
                format!("[중요 표시 {}]", clock(ms))
            } else {
                format!("[중요 표시 {} 메모: {note}]", clock(ms))
            };
            (ms, text)
        })
        .collect();
    marks.sort_by_key(|(ms, _)| *ms);
    let mut marks = marks.into_iter().peekable();

    let mut lines: Vec<Line> = Vec::new();
    let mut ended = false;
    for paragraph in transcript.paragraphs() {
        if !ended && end_ms.is_some_and(|end| paragraph.start_ms >= end) {
            ended = true;
            lines.push(Line {
                ms: None,
                text: AFTER_THE_END.to_string(),
            });
        }
        while marks
            .peek()
            .is_some_and(|(mark, _)| *mark <= paragraph.start_ms)
        {
            if let Some((_, text)) = marks.next() {
                lines.push(Line { ms: None, text });
            }
        }
        lines.push(Line {
            ms: Some(paragraph.start_ms),
            text: format!(
                "[{}] {}: {}",
                clock(paragraph.start_ms),
                transcript.speaker_name(paragraph.speaker),
                paragraph.text
            ),
        });
    }
    for (_, text) in marks {
        lines.push(Line { ms: None, text });
    }

    let mut windows: Vec<Vec<Line>> = Vec::new();
    let mut start = 0;
    while start < lines.len() {
        let mut end = start;
        let mut chars = 0;
        while end < lines.len()
            && (end == start || chars + lines[end].text.chars().count() <= WINDOW_CHARS)
        {
            chars += lines[end].text.chars().count() + 1;
            end += 1;
        }
        windows.push(
            lines[start..end]
                .iter()
                .map(|line| Line {
                    ms: line.ms,
                    text: line.text.clone(),
                })
                .collect(),
        );
        if end >= lines.len() {
            break;
        }
        start = end.saturating_sub(WINDOW_OVERLAP_LINES).max(start + 1);
    }
    windows
}

/// A window as text, each line of speech numbered from 1.
fn numbered(window: &[Line]) -> (String, Vec<u64>) {
    let mut text = String::new();
    let mut starts: Vec<u64> = Vec::new();
    for line in window {
        match line.ms {
            Some(ms) => {
                starts.push(ms);
                text.push_str(&format!("{}. {}\n", starts.len(), line.text));
            }
            None => text.push_str(&format!("   {}\n", line.text)),
        }
    }
    (text, starts)
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// Runs `job` on each of `things`, `at_once` at a time, and gives the results
/// in the order of `things`. After each round `go_on` sees the results so far
/// and says whether to continue. Returns what was done and whether that was
/// all of it.
fn in_rounds<T: Sync, R: Send>(
    things: &[T],
    at_once: usize,
    job: &(dyn Fn(usize, &T) -> R + Sync),
    go_on: &dyn Fn(&[R]) -> bool,
) -> (Vec<R>, bool) {
    let at_once = at_once.max(1);
    let mut results: Vec<R> = Vec::with_capacity(things.len());
    for (round, group) in things.chunks(at_once).enumerate() {
        let first = round * at_once;
        if let [only] = group {
            results.push(job(first, only));
        } else {
            let together: Vec<R> = std::thread::scope(|scope| {
                let running: Vec<_> = group
                    .iter()
                    .enumerate()
                    .map(|(offset, thing)| scope.spawn(move || job(first + offset, thing)))
                    .collect();
                running
                    .into_iter()
                    .map(|handle| {
                        handle
                            .join()
                            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                    })
                    .collect()
            });
            results.extend(together);
        }
        if !go_on(&results) {
            return (results, false);
        }
    }
    (results, true)
}

/// Makes `attempt` again after each of `pauses` while it fails with something
/// that may pass. The last failure is returned.
fn patiently<T>(
    pauses: &[std::time::Duration],
    mut attempt: impl FnMut() -> Result<T>,
) -> Result<T> {
    let mut pauses = pauses.iter();
    loop {
        let error = match attempt() {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        let text = format!("{error:#}");
        let passing = !NOT_WORTH_WAITING.iter().any(|sign| text.contains(sign));
        match pauses.next() {
            Some(pause) if passing => {
                warn!(
                    "A request failed ({text}); making it again in {}s",
                    pause.as_secs()
                );
                std::thread::sleep(*pause);
            }
            _ => return Err(error),
        }
    }
}

/// A request made after the provider has answered once in this job, when a
/// failure is more likely a passing one than a provider that is not there.
fn ask_again(input: &Input<'_>, system: &str, user: &str, schema: &Value) -> Result<Value> {
    patiently(&PATIENCE, || ask(input, system, user, schema))
}

/// One request, tried again when its answer cannot be read. A provider that
/// cannot be reached fails at once: the caller retries the whole job later.
fn ask(input: &Input<'_>, system: &str, user: &str, schema: &Value) -> Result<Value> {
    let request = llm::Request {
        system,
        user,
        schema,
    };
    let mut last = None;
    for attempt in 1..=ATTEMPTS {
        match llm::complete(input.provider, &request, input.secrets) {
            Ok(value) => return Ok(value),
            Err(e) => {
                let unreadable = format!("{e:#}").contains("JSON");
                warn!("Minutes request failed (attempt {attempt}): {e:#}");
                last = Some(e);
                if !unreadable {
                    break;
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("the request failed")))
}

const BASE_RULES: &str = "\
꼭 지킬 것:
1. 받아쓴 글에 있는 내용만 씁니다. 짐작으로 빈 곳을 채우지 않습니다.
2. 항목마다 근거가 된 말을 그 줄에서 그대로 옮긴 짧은 인용(quote, 8~40자)을 적습니다. 인용은 한 글자도 바꾸지 않습니다.
3. owner(담당)는 누가 맡았는지 이름이나 직함으로 분명할 때만 적습니다. \"제가 하겠습니다\"라고 말한 사람이 이름으로 적혀 있으면 그 이름을 씁니다. \"화자 2\" 같은 표기는 쓰지 않습니다. 분명하지 않으면 빈 문자열로 둡니다.
4. due(기한)는 기한이 말로 나왔을 때만, 말한 그대로 적습니다(예: \"금요일까지\", \"다음 주 화요일\"). 없으면 빈 문자열로 둡니다.
5. \"검토해 볼게요\", \"생각해 볼게요\", \"일단\", \"아마\", \"견적을 보고 결정할게요\"처럼 확정하지 않은 말은 결정도 할 일도 아닙니다. section을 open으로 합니다.
6. 숫자, 날짜, 금액, 치수, 이름은 받아쓴 글에 나온 그대로 적습니다.
7. 인사, 맞장구, 잡담은 뽑지 않습니다. [회의 끝 …] 줄 뒤의 말(고객이 나간 뒤의 잡담, 다른 통화)은 아무것도 뽑지 않습니다.
8. [중요 표시 …] 줄은 녹음한 사람이 그 순간을 중요하다고 표시한 것입니다. 그 앞뒤의 말은 빠뜨리지 않습니다. \"메모:\"가 붙어 있으면 왜 표시했는지 적은 것입니다.
9. text는 회의록에 그대로 실을 수 있는 한 문장으로, 군더더기 없이 씁니다.
10. 할 일 갈래(갈래 목록에 '할 일 갈래'라고 붙은 것)의 text는 '무엇을 하기' 꼴의 짧은 할 일로 씁니다. 예: \"신발장 도면을 다시 그려 보내기\", \"냉장고 크기 재서 알려 주기\". 담당과 기한은 text에 넣지 않고 owner와 due에 적습니다. '~함', '~하기로 했다', '~했습니다'로 끝내지 않습니다.
11. 같은 것을 두고 앞뒤 말이 다르면(예산을 바꿈, 색을 바꿈) 마지막에 정한 말을 따릅니다. 앞의 말은 결정·사실로 뽑지 않고, 꼭 남길 때는 '처음에는 ~였다가 ~로 바꿈'처럼 바뀐 것을 text에 밝힙니다.
12. 이미 정한 것을 나중에 다시 묻거나 다른 안을 물어본 것만으로는 '아직 안 정한 것'이 아닙니다. '다시 생각해 보겠다', '보류하자'처럼 분명히 되돌린 경우만 open으로 합니다.
13. 누구 이야기인지 봅니다. 우리 회사가 했던 다른 현장, 다른 고객, 말하는 사람 자신의 집이나 사무실 이야기(예: \"예전에 한 현장은 40평대였어요\", \"제 집은 꼭대기 층이라\")는 이 회의의 고객·현장 사실(현장, 예산, 요청, 결정)로 옮기지 않습니다. 꼭 적어야 하면 text에 '다른 현장 사례'처럼 누구 이야기인지 밝힙니다.";

struct Overview {
    template_id: String,
    title: String,
    speaker_names: Vec<(u32, String)>,
}

/// The kind of recording, a title, and the speakers whose names are said.
/// A failed request falls back to defaults: minutes with "화자 1" are better
/// than none.
fn overview(input: &Input<'_>) -> Result<Overview> {
    let transcript = input.transcript;
    let solo = transcript.speakers.len() <= 1;
    let fallback_template = match input.template_id {
        Some(id) => id.to_string(),
        None if solo => SOLO_TEMPLATE.to_string(),
        None => DEFAULT_TEMPLATE.to_string(),
    };

    // The opening and the close say the most about what this is and who is
    // speaking.
    let lines: Vec<String> = transcript
        .paragraphs()
        .iter()
        .map(|p| {
            format!(
                "[{}] {}: {}",
                clock(p.start_ms),
                transcript.speaker_name(p.speaker),
                p.text
            )
        })
        .collect();
    let mut sample = String::new();
    let mut taken = 0;
    for line in &lines {
        if sample.chars().count() > 2_500 {
            break;
        }
        sample.push_str(line);
        sample.push('\n');
        taken += 1;
    }
    if taken < lines.len() {
        sample.push_str("…\n");
        let tail: Vec<&String> = lines[taken..].iter().rev().take(6).collect();
        for line in tail.into_iter().rev() {
            sample.push_str(line);
            sample.push('\n');
        }
    }

    let kinds: String = input
        .templates
        .iter()
        .map(|t| format!("- {}: {} — {}\n", t.id, t.name, t.description))
        .collect();
    let system = format!(
        "당신은 녹음을 받아 적은 글의 앞부분과 끝부분을 보고 세 가지를 정하는 서기입니다.\n\n\
         1. template: 이 녹음의 종류. 아래에서 하나를 고릅니다.\n{kinds}\n\
         2. title: 이 녹음을 한눈에 알아볼 제목. 받아쓴 글에 나온 말로 20자 안쪽으로 짓습니다(예: \"한빛아파트 상담\", \"주간 회의\"). 날짜는 넣지 않습니다.\n\
         3. speakers: 화자가 누구인지 받아쓴 글에서 분명히 알 수 있는 경우만 적습니다. 스스로 이름을 말했거나(\"저는 김서윤 실장입니다\") 다른 사람이 그 화자를 부른 경우입니다. id는 \"화자 2\"의 숫자, name은 이름과 직함(예: \"김서윤 실장\") 또는 이름이 없을 때의 역할(예: \"고객\"), quote는 그렇게 볼 근거가 된 말을 그대로 옮긴 것입니다. 알 수 없는 화자는 넣지 않습니다. 짐작하지 않습니다."
    );
    let people = if input.people.is_empty() {
        String::new()
    } else {
        format!("미리 알려진 참석자: {}\n\n", input.people.join(", "))
    };
    let user = format!("{people}받아쓴 글:\n{sample}");
    let ids: Vec<&str> = input.templates.iter().map(|t| t.id.as_str()).collect();
    let schema = json!({
        "type": "object",
        "properties": {
            "template": { "type": "string", "enum": ids },
            "title": { "type": "string" },
            "speakers": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "integer" },
                        "name": { "type": "string" },
                        "quote": { "type": "string" }
                    },
                    "required": ["id", "name", "quote"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["template", "title", "speakers"],
        "additionalProperties": false
    });

    let answer = match ask(input, &system, &user, &schema) {
        Ok(answer) => answer,
        Err(e) => {
            // Unreachable now is unreachable for the windows too: stop here.
            if !format!("{e:#}").contains("JSON") {
                return Err(e);
            }
            warn!("Overview could not be read; using defaults");
            json!({})
        }
    };

    let template_id = match input.template_id {
        Some(id) => id.to_string(),
        None if solo => SOLO_TEMPLATE.to_string(),
        None => answer
            .get("template")
            .and_then(Value::as_str)
            .filter(|id| input.templates.iter().any(|t| t.id == *id))
            .map_or(fallback_template, str::to_string),
    };
    let title: String = answer
        .get("title")
        .and_then(Value::as_str)
        .map(|title| title.trim().chars().take(40).collect())
        .unwrap_or_default();

    // A name is kept only when the words given for it are in the recording,
    // spoken by or to that speaker's part of the transcript.
    let index = Index::new(transcript);
    let mut speaker_names: Vec<(u32, String)> = Vec::new();
    for speaker in answer
        .get("speakers")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
    {
        let (Some(id), Some(name), Some(quote)) = (
            speaker.get("id").and_then(Value::as_u64),
            speaker.get("name").and_then(Value::as_str),
            speaker.get("quote").and_then(Value::as_str),
        ) else {
            continue;
        };
        let name = name.trim();
        let id = id as u32;
        let known = transcript.speakers.iter().any(|s| s.id == id);
        let fits = !name.is_empty() && name.chars().count() <= 16 && !name.starts_with("화자");
        let said = index
            .find(quote, None)
            .is_some_and(|(_, score)| score >= CLOSE_MATCH);
        let taken = speaker_names.iter().any(|(other, _)| *other == id);
        if known && fits && said && !taken {
            speaker_names.push((id, name.to_string()));
        }
    }
    Ok(Overview {
        template_id,
        title,
        speaker_names,
    })
}

/// An item as the model gave it.
struct RawItem {
    section: String,
    text: String,
    owner: String,
    due: String,
    /// When the line the model took it from starts.
    near_ms: Option<u64>,
    quote: String,
}

fn extract(
    input: &Input<'_>,
    template: &Template,
    window: &[Line],
    number: usize,
    of: usize,
) -> Result<Vec<RawItem>> {
    match extract_once(input, template, window, number, of) {
        Err(e) if format!("{e:#}").contains(llm::CUT_OFF) && window.len() >= 8 => {
            // The model ran out of room for its answer, which a long
            // meeting with a small model does. Ask about half as much.
            warn!(
                "Window {number} of {of} is too much for the model at once; reading it in two halves"
            );
            let middle = window.len() / 2;
            let mut items = extract(input, template, &window[..middle], number, of)?;
            items.extend(extract(input, template, &window[middle..], number, of)?);
            Ok(items)
        }
        other => other,
    }
}

fn extract_once(
    input: &Input<'_>,
    template: &Template,
    window: &[Line],
    number: usize,
    of: usize,
) -> Result<Vec<RawItem>> {
    let (text, starts) = numbered(window);
    if starts.is_empty() {
        return Ok(Vec::new());
    }
    let sections: String = template
        .sections
        .iter()
        .map(|s| {
            format!(
                "- {} ({}){}: {}\n",
                s.key,
                s.title,
                if s.kind == SectionKind::Actions {
                    ", 할 일 갈래"
                } else {
                    ""
                },
                s.instruction
            )
        })
        .collect();
    let system = format!(
        "당신은 녹음을 받아 적은 글에서 회의록에 들어갈 내용을 뽑는 서기입니다. 받아쓴 글은 줄마다 번호가 붙어 있습니다.\n\n\
         {BASE_RULES}\n\n\
         이 녹음의 종류: {} — {}\n{}\n\n뽑을 갈래(section):\n{sections}\n\
         답하는 방법: 1번 줄부터 마지막 줄까지 차례로, 모든 줄에 대해 하나씩 답합니다. line에 줄 번호를 적고, 그 줄에서 뽑은 항목을 items에 넣습니다. 한 줄에서 여러 항목이 나올 수 있습니다. 뽑을 것이 없는 줄은 items를 비워 둡니다. 줄을 건너뛰지 않습니다.",
        template.name, template.description, template.guidance
    );
    let mut user = String::new();
    if !input.recorded_on.is_empty() {
        user.push_str(&format!("녹음한 날: {}\n", input.recorded_on));
    }
    if !input.people.is_empty() {
        user.push_str(&format!("참석자: {}\n", input.people.join(", ")));
    }
    if !input.context.is_empty() {
        // Spellings to trust; never a source of items on its own.
        user.push_str(&format!(
            "녹음한 사람이 미리 알려 준 것(이름·용어의 바른 표기로만 쓰고, 이것만으로 항목을 만들지 않음):\n{}\n",
            input.context
        ));
    }
    user.push_str(&format!(
        "\n받아쓴 글 ({number}/{of}, 1번부터 {}번 줄까지):\n{text}",
        starts.len()
    ));

    let keys: Vec<&str> = template.sections.iter().map(|s| s.key.as_str()).collect();
    let schema = json!({
        "type": "object",
        "properties": {
            "lines": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "line": { "type": "integer" },
                        "items": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "section": { "type": "string", "enum": keys },
                                    "text": { "type": "string" },
                                    "owner": { "type": "string" },
                                    "due": { "type": "string" },
                                    "quote": { "type": "string" }
                                },
                                "required": ["section", "text", "owner", "due", "quote"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["line", "items"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["lines"],
        "additionalProperties": false
    });

    // A model that stops early has read only part of the window. One more
    // try, saying so; the fuller of the two answers is kept.
    let mut best: (usize, Vec<RawItem>) = (0, Vec::new());
    for attempt in 0..2 {
        let nudge = if attempt == 0 {
            String::new()
        } else {
            format!(
                "\n\n앞선 답은 {}번 줄까지만 다뤘습니다. 1번부터 {}번까지 모든 줄에 대해 답해야 합니다.",
                best.0,
                starts.len()
            )
        };
        let answer = ask_again(input, &system, &format!("{user}{nudge}"), &schema)?;
        let (covered, items) = read_extraction(&answer, &starts);
        if covered >= best.0 {
            best = (covered, items);
        }
        if best.0 * 10 >= starts.len() * 8 {
            break;
        }
        warn!(
            "The model answered for {} of {} lines of window {number}",
            best.0,
            starts.len()
        );
    }
    Ok(best.1)
}

/// The items of one extraction answer, and how many of the window's lines it
/// answered for.
fn read_extraction(answer: &Value, starts: &[u64]) -> (usize, Vec<RawItem>) {
    let text_of = |item: &Value, key: &str| {
        item.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let mut covered: HashSet<usize> = HashSet::new();
    let mut items: Vec<RawItem> = Vec::new();
    for line in answer
        .get("lines")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
    {
        let Some(number) = line
            .get("line")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .filter(|n| (1..=starts.len()).contains(n))
        else {
            continue;
        };
        covered.insert(number);
        for item in line
            .get("items")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
        {
            let raw = RawItem {
                section: text_of(item, "section"),
                text: text_of(item, "text"),
                owner: text_of(item, "owner"),
                due: text_of(item, "due"),
                near_ms: Some(starts[number - 1]),
                quote: text_of(item, "quote"),
            };
            if !raw.text.is_empty() {
                items.push(raw);
            }
        }
    }
    (covered.len(), items)
}

/// An item that passed the machine's checks.
#[derive(Debug, Clone)]
struct Item {
    section: String,
    entry: Entry,
    /// The transcript lines it cites.
    lines: std::ops::RangeInclusive<usize>,
}

/// Checks one item against the transcript. `None` when the recording does
/// not back it.
fn check_item(
    raw: RawItem,
    template: &Template,
    index: &Index<'_>,
    transcript: &Transcript,
) -> Option<Item> {
    let section = template.section(&raw.section)?.key.clone();
    let (lines, score) = index.find(&raw.quote, raw.near_ms)?;
    let mut reasons: Vec<CheckReason> = Vec::new();
    if score < CLOSE_MATCH {
        reasons.push(CheckReason::LooseQuote);
    }

    let cited = &index.segments[lines.clone()];
    if cited
        .iter()
        .any(|line| line.unsure || line.confidence.is_some_and(|c| c < index.unsure_below))
    {
        reasons.push(CheckReason::UnsureSpeech);
    }
    if cited.iter().any(|line| line.overlap) {
        reasons.push(CheckReason::OverlappingSpeech);
    }

    // Every number the item states must be in the lines around its quote,
    // as the same digits or the same value however said ("아홉 평" is 9평).
    let stated = format!("{} {}", raw.text, raw.due);
    if !super::amounts::backed(&stated, &index.text_around(&lines, 2)) {
        reasons.push(CheckReason::NumberNotInSource);
    }
    // An amount no one says that way, in the item or in the words it copies
    // ("1억 5천억", "80,900만원"): the speech model may have misheard it.
    let quoted = index.text_around(&lines, 0);
    if super::amounts::odd_units(&raw.text)
        || (!numbers_in(&raw.text).is_empty() && super::amounts::odd_units(&quoted))
    {
        reasons.push(CheckReason::UnitOdd);
    }
    // The same rate said with another value close by ("자당 20", "자당 25만").
    if super::amounts::rates_differ(&raw.text, &index.text_around(&lines, 3)) {
        reasons.push(CheckReason::NumbersDiffer);
    }

    // An owner is a person: a name the recording has, never a speaker label.
    let owner = raw.owner.trim();
    let owner_known = !owner.is_empty()
        && !owner.starts_with("화자")
        && (transcript.speakers.iter().any(|s| s.name == owner)
            || owner.split_whitespace().next().is_some_and(|first| {
                first.chars().count() >= 2 && index.text.contains(&bare(first))
            }));
    let first = &index.segments[*lines.start()];
    // The quote as transcribed, not as the model retyped it.
    let quote = if score >= 1.0 {
        raw.quote.clone()
    } else {
        first.text.clone()
    };
    Some(Item {
        section,
        entry: Entry {
            text: raw.text,
            owner: if owner_known {
                owner.to_string()
            } else {
                String::new()
            },
            due: raw.due,
            sources: vec![Source {
                ms: first.start_ms,
                quote,
            }],
            reasons,
            done: false,
            faint: false,
        },
        lines,
    })
}

/// Marks the entries whose changes of an amount do not add up: one says it
/// went from A to B, another (or the same) by C, near B − A but not it.
fn mark_sums(entries: &mut [(String, Entry)]) {
    let texts: Vec<&str> = entries.iter().map(|(_, e)| e.text.as_str()).collect();
    for at in super::amounts::sums_disagree(&texts) {
        push_reason(&mut entries[at].1, CheckReason::SumsDisagree);
    }
}

/// Whether the recording was transcribed as Korean (or the language is not
/// said, as with older transcripts).
fn korean(transcript: &Transcript) -> bool {
    transcript.language.is_empty() || transcript.language.starts_with("ko")
}

/// Whether everything an item rests on was said after the meeting ended.
fn after_the_end(item: &Item, index: &Index<'_>, end_ms: Option<u64>) -> bool {
    end_ms.is_some_and(|end| index.segments[*item.lines.start()].start_ms >= end)
}

/// Marks a fact or a choice whose words talk about another site or person
/// (a past case, the speaker's own home): it may be someone else's story
/// moved onto this meeting's customer. Tasks are not marked: who does them
/// is checked on its own.
fn mark_elsewhere(
    item: &mut Item,
    template: &Template,
    index: &Index<'_>,
    transcript: &Transcript,
    customer: &str,
) {
    if template
        .section(&item.section)
        .is_some_and(|section| section.kind == SectionKind::Actions)
    {
        return;
    }
    let cited = &index.segments[item.lines.clone()];
    let words: Vec<&str> = cited.iter().map(|line| line.text.as_str()).collect();
    let by_customer = cited
        .iter()
        .all(|line| super::facts::is_customer(&transcript.speaker_name(line.speaker), customer));
    if super::facts::about_someone_else(&item.entry.text, &words.join(" "), by_customer) {
        push_reason(&mut item.entry, CheckReason::OtherSubject);
    }
}

/// Doubts that keep an entry out of the story and the summary: it may not
/// have been said so, may be someone else's, or carries a number that may
/// be wrong; stating it there would undo the "확인 필요" beside it.
const DOUBTS_NOT_TOLD: [CheckReason; 7] = [
    CheckReason::WeakSupport,
    CheckReason::Conflicting,
    CheckReason::OtherSubject,
    CheckReason::NumberNotInSource,
    CheckReason::NumbersDiffer,
    CheckReason::UnitOdd,
    CheckReason::SumsDisagree,
];

/// The most pairs of entries put to the model as possibly disagreeing.
const MOST_CLASHES: usize = 12;

/// Finds entries that may say different things about the same matter and
/// asks the model, for each pair, which stands: the later word when it
/// changed what was said before; the earlier when the later only asked
/// about it again; both when they are about different things. A pair it
/// cannot settle, or that the model could not be asked about, is marked
/// for a look on both sides. Returns how many entries were left out.
fn reconcile(
    input: &Input<'_>,
    template: &Template,
    entries: &mut Vec<(String, Entry)>,
    index: &Index<'_>,
    transcript: &Transcript,
) -> u32 {
    // Facts and choices; tasks are read together later.
    let candidates: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, (key, _))| {
            template
                .section(key)
                .is_some_and(|section| section.kind != SectionKind::Actions)
        })
        .map(|(at, _)| at)
        .collect();
    let said: Vec<super::facts::Said<'_>> = candidates
        .iter()
        .map(|&at| super::facts::Said {
            section: entries[at].0.as_str(),
            text: entries[at].1.text.as_str(),
            ms: entries[at].1.sources.first().map_or(0, |s| s.ms),
        })
        .collect();
    let pairs: Vec<(usize, usize, super::facts::Clash)> =
        super::facts::clashes(&said, OPEN_SECTION, MOST_CLASHES)
            .into_iter()
            .map(|(x, y, clash)| (candidates[x], candidates[y], clash))
            .collect();
    if pairs.is_empty() {
        return 0;
    }

    // What was said around an entry, and a little after: a later word may
    // be the one that settles it.
    let around = |entry: &Entry, after: usize| -> String {
        let ms = entry.sources.last().map_or(0, |s| s.ms);
        let at = index
            .segments
            .iter()
            .position(|segment| segment.start_ms >= ms)
            .unwrap_or(0);
        let first = at.saturating_sub(1);
        let last = (at + after).min(index.segments.len() - 1);
        index.segments[first..=last]
            .iter()
            .map(|segment| {
                format!(
                    "[{}] {}: {}\n",
                    clock(segment.start_ms),
                    transcript.speaker_name(segment.speaker),
                    segment.text
                )
            })
            .collect()
    };
    let section_title = |key: &str| {
        template
            .section(key)
            .map_or(key.to_string(), |s| s.title.clone())
    };
    let listing: String = pairs
        .iter()
        .enumerate()
        .map(|(number, &(a, b, _))| {
            let (ka, ea) = &entries[a];
            let (kb, eb) = &entries[b];
            format!(
                "### {}\n먼저 나온 것 [{}] {}\n{}나중에 나온 것 [{}] {}\n{}\n",
                number + 1,
                section_title(ka),
                ea.text,
                around(ea, 2),
                section_title(kb),
                eb.text,
                around(eb, 4),
            )
        })
        .collect();
    let system = "당신은 회의록을 검수하는 사람입니다. 회의록에 실릴 항목 가운데 같은 것을 두고 서로 다르게 말하는 것 같은 두 항목이 번호별로, 녹음의 그 대목과 함께 주어집니다. 대목에 적힌 말만 보고 판정합니다.\n\n\
         verdict:\n\
         - later: 나중 말이 앞의 말을 바꿨다(예산을 올림, 색을 바꿈, 다시 생각해 보겠다고 분명히 되돌림). 나중 것만 남깁니다.\n\
         - earlier: 앞에서 정한 것이 그대로다. 나중 것은 다시 물어보거나 다른 안을 물어본 것일 뿐 바꾸지 않았거나, 나중 것이 다른 사람·다른 현장 이야기다. 앞의 것만 남깁니다.\n\
         - both: 서로 다른 것에 관한 말이라 둘 다 맞다.\n\
         - unclear: 어느 쪽이 맞는지 대목만으로는 알 수 없다.\n\n\
         모든 번호에 대해 하나씩 답합니다.";
    let schema = json!({
        "type": "object",
        "properties": {
            "results": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "integer" },
                        "verdict": { "type": "string", "enum": ["later", "earlier", "both", "unclear"] }
                    },
                    "required": ["id", "verdict"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["results"],
        "additionalProperties": false
    });
    let verdicts: BTreeMap<usize, String> = match ask_again(input, system, &listing, &schema) {
        Ok(answer) => answer
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|result| {
                Some((
                    result.get("id")?.as_u64()? as usize,
                    result.get("verdict")?.as_str()?.to_string(),
                ))
            })
            .collect(),
        Err(e) => {
            warn!(
                "Could not ask which of {} pairs stands ({e:#}); marking them for a look",
                pairs.len()
            );
            BTreeMap::new()
        }
    };
    apply_reconciled(entries, &pairs, &verdicts)
}

/// Makes the verdicts so: the entry that does not stand goes, and a pair
/// that was not settled is marked on both sides. Returns how many went.
fn apply_reconciled(
    entries: &mut Vec<(String, Entry)>,
    pairs: &[(usize, usize, super::facts::Clash)],
    verdicts: &BTreeMap<usize, String>,
) -> u32 {
    let mut gone: HashSet<usize> = HashSet::new();
    for (number, &(earlier, later, _)) in pairs.iter().enumerate() {
        match verdicts.get(&(number + 1)).map(String::as_str) {
            Some("later") => {
                gone.insert(earlier);
            }
            Some("earlier") => {
                gone.insert(later);
            }
            Some("both") => {}
            // Unsettled, or not answered: said both ways, so a person hears it.
            _ => {
                push_reason(&mut entries[earlier].1, CheckReason::Conflicting);
                push_reason(&mut entries[later].1, CheckReason::Conflicting);
            }
        }
    }
    let mut at = 0;
    entries.retain(|_| {
        let keep = !gone.contains(&at);
        at += 1;
        keep
    });
    gone.len() as u32
}

/// The runs of digits in `text`, with grouping commas removed: `4,500` → `4500`.
fn numbers_in(text: &str) -> Vec<String> {
    let mut numbers = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_digit() {
            current.push(c);
        } else if c == ','
            && !current.is_empty()
            && chars.get(i + 1).is_some_and(char::is_ascii_digit)
        {
            // A grouping comma inside a number.
        } else if !current.is_empty() {
            numbers.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        numbers.push(current);
    }
    numbers
}

/// `text` with everything but digits turned to spaces and grouping commas
/// closed up, so a stated number can be looked for as a whole.
fn digits_of(text: &str) -> String {
    numbers_in(text).join(" ")
}

/// How alike two texts are, from 0 to 1: the share of their two-character
/// pieces they have in common.
fn likeness(a: &str, b: &str) -> f32 {
    let pieces = |text: &str| -> HashSet<(char, char)> {
        let chars: Vec<char> = bare(text).chars().collect();
        chars.windows(2).map(|pair| (pair[0], pair[1])).collect()
    };
    let (a, b) = (pieces(a), pieces(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    2.0 * a.intersection(&b).count() as f32 / (a.len() + b.len()) as f32
}

/// Two items say the same thing when they are in the same section and their
/// texts are mostly the same, or nearly the same and about the same line.
/// Numbers must agree: "40만 원" and "280만 원" are different facts however
/// alike the sentences around them.
fn same_thing(a: &Item, b: &Item) -> bool {
    if a.section != b.section {
        return false;
    }
    let alike = likeness(&a.entry.text, &b.entry.text);
    let same_line = a.lines.start() == b.lines.start();
    let numbers_agree = {
        let mut x = numbers_in(&a.entry.text);
        let mut y = numbers_in(&b.entry.text);
        x.sort();
        y.sort();
        x == y
    };
    numbers_agree && (alike >= 0.75 || (same_line && alike >= 0.5))
}

struct Composed {
    /// `(section key, entry)` in the order of the recording.
    entries: Vec<(String, Entry)>,
}

/// Merges the items that say the same thing. The summary is written later,
/// from what is left once everything is checked.
fn compose(items: Vec<Item>) -> Composed {
    let mut merged: Vec<Item> = Vec::new();
    for item in items {
        match merged.iter_mut().find(|kept| same_thing(kept, &item)) {
            Some(kept) => {
                for source in item.entry.sources {
                    if !kept.entry.sources.iter().any(|s| s.ms == source.ms) {
                        kept.entry.sources.push(source);
                    }
                }
                kept.entry.sources.sort_by_key(|source| source.ms);
                for reason in item.entry.reasons {
                    push_reason(&mut kept.entry, reason);
                }
                // What one telling leaves out, the other may give.
                if kept.entry.owner.is_empty() {
                    kept.entry.owner = item.entry.owner;
                }
                if kept.entry.due.is_empty() {
                    kept.entry.due = item.entry.due;
                }
            }
            None => merged.push(item),
        }
    }
    merged.sort_by_key(|item| item.entry.sources.first().map_or(0, |source| source.ms));
    Composed {
        entries: merged
            .into_iter()
            .map(|item| (item.section, item.entry))
            .collect(),
    }
}

/// The section of the site's facts (address, size, floor), which the checker
/// reads beside their lines like tasks and decisions.
const SITE_SECTION: &str = "site";

/// Whether an entry in `section` states a commitment: a task, or something
/// decided. Notes on what was discussed do not.
fn commits(template: &Template, section: &str) -> bool {
    section != OPEN_SECTION
        && template
            .section(section)
            .is_some_and(|s| s.kind == SectionKind::Actions || section == "decisions")
}

/// The items as the summary's writer reads them. All of them when they fit;
/// for a long meeting, the tasks and decisions first and then what room is
/// left, because more than a model reads is cut off without a word.
fn summary_listing(template: &Template, items: &[(String, Entry)]) -> String {
    let line = |(key, entry): &(String, Entry)| {
        let section = template
            .section(key)
            .map_or(key.as_str(), |section| section.title.as_str());
        format!("- [{section}] {}\n", entry.text)
    };
    let whole: String = items.iter().map(line).collect();
    if whole.chars().count() <= SUMMARY_CHARS {
        return whole;
    }
    let first = items.iter().filter(|(key, _)| commits(template, key));
    let rest = items.iter().filter(|(key, _)| !commits(template, key));
    let mut listing = String::new();
    let mut used = 0;
    for item in first.chain(rest) {
        let line = line(item);
        let size = line.chars().count();
        if used + size > SUMMARY_CHARS {
            break;
        }
        used += size;
        listing.push_str(&line);
    }
    listing
}

/// One sentence on who talked about what and how it ended, written from the
/// checked items alone.
fn summarize(
    input: &Input<'_>,
    template: &Template,
    items: &[(String, Entry)],
    title: &str,
) -> Result<String> {
    let listing = summary_listing(template, items);
    let system = "당신은 회의록 맨 위에 들어갈 한 줄 요약을 쓰는 서기입니다. 녹음에서 뽑아 확인을 마친 항목들이 주어집니다.\n\n\
         - 누구와 무엇을 이야기했고 결론이 무엇인지, 사람이 말하듯 자연스러운 한 문장으로 씁니다. 100자 안쪽.\n\
         - 항목을 나열하지 않습니다. '~함', '~음'으로 끝내지 않습니다.\n\
         - 주어진 항목에 있는 내용만 씁니다. 새로운 사실, 숫자, 이름을 보태지 않습니다.\n\
         - '~했다', '~하기로 했다'처럼 담백하게 끝냅니다.";
    let user = format!(
        "녹음 종류: {}\n제목: {}\n\n확인을 마친 항목:\n{listing}",
        template.name,
        if title.is_empty() { "(없음)" } else { title }
    );
    let schema = json!({
        "type": "object",
        "properties": { "summary": { "type": "string" } },
        "required": ["summary"],
        "additionalProperties": false
    });
    let answer = ask_again(input, system, &user, &schema)?;
    let summary: String = answer
        .get("summary")
        .and_then(Value::as_str)
        .map(|summary| summary.trim().chars().take(300).collect())
        .unwrap_or_default();
    // A number in the summary that no item has was made up.
    let known = digits_of(&listing);
    if numbers_in(&summary)
        .iter()
        .any(|n| !known.contains(n.as_str()))
    {
        return Err(anyhow!("the summary states a number the items do not have"));
    }
    Ok(summary)
}

/// The most of the items the story's writer reads, in characters.
const STORY_CHARS: usize = 14_000;
/// The most parts a story has, and the most times shown with one.
const STORY_PARTS: usize = 6;
const STORY_TIMES: usize = 3;

/// The story of the meeting in a few parts, each a heading and a paragraph
/// written from the checked entries. Every sentence names the entries it
/// rests on; one that names none, or states a number they do not have, is
/// left out, and so is a part left with no sentence.
fn tell(
    input: &Input<'_>,
    template: &Template,
    entries: &[(String, Entry)],
    title: &str,
) -> Result<Vec<StoryPart>> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    // In the order they were said; tasks and decisions are kept first when
    // not all fit.
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by_key(|&at| entries[at].1.sources.first().map_or(0, |s| s.ms));
    let mut chosen: Vec<usize> = Vec::new();
    let mut used = 0;
    for pass in [true, false] {
        for &at in &order {
            if commits(template, &entries[at].0) != pass {
                continue;
            }
            let size = entries[at].1.text.chars().count() + 24;
            if used + size > STORY_CHARS {
                continue;
            }
            used += size;
            chosen.push(at);
        }
    }
    chosen.sort_by_key(|&at| entries[at].1.sources.first().map_or(0, |s| s.ms));
    let listing: String = chosen
        .iter()
        .enumerate()
        .map(|(number, &at)| {
            let (key, entry) = &entries[at];
            let section = template
                .section(key)
                .map_or(key.as_str(), |s| s.title.as_str());
            let ms = entry.sources.first().map_or(0, |s| s.ms);
            format!(
                "{}. [{}] [{section}] {}\n",
                number + 1,
                clock(ms),
                entry.text
            )
        })
        .collect();

    let system = "당신은 회의록에 들어갈 '이야기 요약'을 쓰는 서기입니다. 녹음에서 뽑아 확인을 마친 항목들이 번호, 말한 시각, 갈래와 함께 주어집니다.\n\n\
         - 회의가 어떻게 시작했고, 무엇을 이야기했고, 어디서 의견이 갈렸고, 어떻게 정리됐는지가 흐르듯 읽히게 씁니다. 항목을 나열하지 않습니다.\n\
         - 짧은 소제목을 붙인 부분 3~6개로 나눕니다. 부분마다 2~4문장입니다.\n\
         - 문장은 '~했다', '~하기로 했다'처럼 평서문으로 끝냅니다. '~함', '~음', '~됨'으로 끝내지 않습니다.\n\
         - 숫자, 금액, 날짜, 이름은 항목에 적힌 그대로 살립니다.\n\
         - 문장마다 그 문장의 근거가 된 항목 번호(items)를 적습니다. 항목에 없는 내용은 쓰지 않습니다.";
    let user = format!(
        "녹음 종류: {}\n제목: {}\n\n확인을 마친 항목:\n{listing}",
        template.name,
        if title.is_empty() { "(없음)" } else { title }
    );
    let schema = json!({
        "type": "object",
        "properties": {
            "parts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "sentences": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "text": { "type": "string" },
                                    "items": { "type": "array", "items": { "type": "integer" } }
                                },
                                "required": ["text", "items"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["title", "sentences"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["parts"],
        "additionalProperties": false
    });
    let answer = ask_again(input, system, &user, &schema)?;
    let cited: Vec<&Entry> = chosen.iter().map(|&at| &entries[at].1).collect();
    Ok(read_story(&answer, &cited))
}

/// The parts of a story answer that rest on `cited` (the entries by their
/// number, from 1).
fn read_story(answer: &Value, cited: &[&Entry]) -> Vec<StoryPart> {
    let mut parts: Vec<StoryPart> = Vec::new();
    for part in answer
        .get("parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let title = part
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let mut sentences: Vec<String> = Vec::new();
        let mut sources: Vec<Source> = Vec::new();
        for sentence in part
            .get("sentences")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let text = sentence
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            let backing: Vec<&Entry> = sentence
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .filter_map(|n| (n as usize).checked_sub(1))
                .filter_map(|at| cited.get(at).copied())
                .collect();
            if text.is_empty() || backing.is_empty() {
                continue;
            }
            // A number its entries and their quotes do not have was made up.
            let known: String = backing
                .iter()
                .map(|entry| {
                    let quotes: Vec<&str> =
                        entry.sources.iter().map(|s| s.quote.as_str()).collect();
                    format!("{} {}", entry.text, quotes.join(" "))
                })
                .map(|text| digits_of(&text))
                .collect::<Vec<_>>()
                .join(" ");
            if numbers_in(&text)
                .iter()
                .any(|n| !known.contains(n.as_str()))
            {
                continue;
            }
            for entry in backing {
                if let Some(source) = entry.sources.first() {
                    if !sources.iter().any(|s| s.ms == source.ms) {
                        sources.push(source.clone());
                    }
                }
            }
            sentences.push(text);
        }
        if sentences.is_empty() {
            continue;
        }
        sources.sort_by_key(|source| source.ms);
        // The first, the middle and the last place it rests on.
        if sources.len() > STORY_TIMES {
            let last = sources.len() - 1;
            sources = vec![
                sources[0].clone(),
                sources[last / 2].clone(),
                sources[last].clone(),
            ];
        }
        parts.push(StoryPart {
            title,
            text: sentences.join(" "),
            sources,
        });
        if parts.len() >= STORY_PARTS {
            break;
        }
    }
    parts
}

/// What the checker said of one entry: its verdict, and whether the owner
/// and the date are in the lines.
type Verdict = (String, bool, bool, String);

/// Groups entries of the given sizes (in characters) into requests: in
/// order, each of at most `VERIFY_ENTRIES` entries and `VERIFY_CHARS`
/// characters. An entry longer than that goes alone.
fn verify_batches(sizes: &[usize]) -> Vec<Vec<usize>> {
    let mut batches: Vec<Vec<usize>> = Vec::new();
    let mut chars = 0;
    for (position, &size) in sizes.iter().enumerate() {
        let full = batches
            .last()
            .is_none_or(|batch| batch.len() >= VERIFY_ENTRIES || chars + size > VERIFY_CHARS);
        if full {
            batches.push(Vec::new());
            chars = 0;
        }
        if let Some(batch) = batches.last_mut() {
            batch.push(position);
        }
        chars += size;
    }
    batches
}

/// The checker's answer to one request, by the number each entry had in it.
fn read_verdicts(answer: &Value) -> BTreeMap<usize, Verdict> {
    let mut verdicts: BTreeMap<usize, Verdict> = BTreeMap::new();
    for result in answer
        .get("results")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
    {
        if let Some(id) = result.get("id").and_then(Value::as_u64) {
            verdicts.insert(
                id as usize,
                (
                    result
                        .get("verdict")
                        .and_then(Value::as_str)
                        .unwrap_or("partly")
                        .to_string(),
                    result
                        .get("owner_stated")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                    result
                        .get("due_stated")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                    result
                        .get("doubt")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                ),
            );
        }
    }
    verdicts
}

/// Applies the checker's answers. `targets` are the entries that were put to
/// it, `batches` how they were grouped (positions in `targets`), `answers`
/// one per batch, `None` where the request failed. Entries the lines do not
/// back are removed (the count is returned); entries backed only in part,
/// or not answered for, are marked.
fn apply_verdicts(
    entries: &mut Vec<(String, Entry)>,
    targets: &[usize],
    batches: &[Vec<usize>],
    answers: &[Option<BTreeMap<usize, Verdict>>],
) -> u32 {
    let mut remove: Vec<usize> = Vec::new();
    for (batch, answer) in batches.iter().zip(answers) {
        for (number, &position) in batch.iter().enumerate() {
            let at = targets[position];
            let entry = &mut entries[at].1;
            match answer
                .as_ref()
                .and_then(|verdicts| verdicts.get(&(number + 1)))
            {
                Some((verdict, owner_stated, due_stated, doubt)) => {
                    if !owner_stated {
                        entry.owner.clear();
                    }
                    if !due_stated {
                        entry.due.clear();
                    }
                    match verdict.as_str() {
                        "supported" => {}
                        "unsupported" => remove.push(at),
                        // What exactly is in doubt, so the person knows what
                        // to listen for.
                        _ => push_reason(
                            entry,
                            match doubt.as_str() {
                                "who" => CheckReason::OwnerUnclear,
                                "when" => CheckReason::DueUnclear,
                                "decided" => CheckReason::NotSettled,
                                "differs" => CheckReason::SaidDifferently,
                                "number" => CheckReason::NumbersDiffer,
                                "subject" => CheckReason::OtherSubject,
                                _ => CheckReason::WeakSupport,
                            },
                        ),
                    }
                }
                // No answer for it is not a pass. Unverified is not the same
                // as verified: the entry says so.
                None => push_reason(entry, CheckReason::Unchecked),
            }
        }
    }
    remove.sort_unstable();
    let removed = remove.len() as u32;
    for at in remove.into_iter().rev() {
        entries.remove(at);
    }
    removed
}

/// Shows the model each task and decision beside the lines it cites and asks
/// whether those lines say it, a batch at a time. Entries they do not back
/// are removed (the count is returned); entries they back only in part are
/// marked. `None` if `cancelled` was set along the way.
fn verify(
    input: &Input<'_>,
    template: &Template,
    entries: &mut Vec<(String, Entry)>,
    index: &Index<'_>,
    transcript: &Transcript,
    progress: &dyn Fn(f32),
    cancelled: &AtomicBool,
) -> Option<u32> {
    // What states a commitment: tasks, and anything in a section named as
    // decided; and the site, which is easily taken from someone else's
    // story. Notes on what was discussed are not put to the question.
    let targets: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, (key, _))| commits(template, key) || key == SITE_SECTION)
        .map(|(i, _)| i)
        .collect();
    if targets.is_empty() {
        return Some(0);
    }

    // Each entry as the checker reads it, without its number.
    let blocks: Vec<String> = targets
        .iter()
        .map(|&at| {
            let (_, entry) = &entries[at];
            let mut block = format!("주장: {}", entry.text);
            if !entry.owner.is_empty() {
                block.push_str(&format!(" / 담당: {}", entry.owner));
            }
            if !entry.due.is_empty() {
                block.push_str(&format!(" / 기한: {}", entry.due));
            }
            block.push_str("\n근거가 된 대목:\n");
            for source in entry.sources.iter().take(VERIFY_SOURCES) {
                let at = index
                    .segments
                    .iter()
                    .position(|segment| segment.start_ms == source.ms)
                    .unwrap_or(0);
                let first = at.saturating_sub(2);
                let last = (at + 3).min(index.segments.len() - 1);
                for segment in &index.segments[first..=last] {
                    block.push_str(&format!(
                        "[{}] {}: {}\n",
                        clock(segment.start_ms),
                        transcript.speaker_name(segment.speaker),
                        segment.text
                    ));
                }
            }
            block
        })
        .collect();
    let sizes: Vec<usize> = blocks.iter().map(|block| block.chars().count()).collect();
    let batches = verify_batches(&sizes);

    let system = "당신은 회의록을 검수하는 사람입니다. 회의록에 실릴 주장과, 그 근거로 든 녹음의 대목이 번호별로 주어집니다. 대목에 적힌 말만 보고 판정합니다.\n\n\
         verdict:\n\
         - supported: 대목이 그 주장을 분명히 말하고 있다.\n\
         - partly: 비슷한 말은 있지만 확정된 것인지, 누가 하는지, 내용이 맞는지가 분명하지 않다.\n\
         - unsupported: 대목에 그런 말이 없다.\n\
         owner_stated: 주장에 담당이 적혀 있다면, 그 사람이 맡는다는 것이 대목에서 분명한가. 담당이 없으면 true.\n\
         due_stated: 주장에 기한이 적혀 있다면, 그 기한이 대목에 나오는가. 기한이 없으면 true.\n\
         doubt: partly일 때 무엇이 분명하지 않은지 하나를 고릅니다. who(누가 할지 말이 없음), when(기한이 말에 없음), decided(정한 것인지 이야기만 한 것인지 분명하지 않음), differs(말한 내용과 조금 다르게 적혔음), number(금액이나 숫자가 다르게 나옴), subject(다른 현장·다른 사람·말하는 사람 자기 집 이야기를 이 회의 고객의 일처럼 적었을 수 있음), other(그 밖). supported나 unsupported면 none.\n\
         대목이 다른 현장이나 말하는 사람 자신의 집 이야기인데 주장이 이 회의 고객의 사실로 적혀 있으면 unsupported입니다.\n\n\
         모든 번호에 대해 하나씩 답합니다.";
    let schema = json!({
        "type": "object",
        "properties": {
            "results": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "integer" },
                        "verdict": { "type": "string", "enum": ["supported", "partly", "unsupported"] },
                        "owner_stated": { "type": "boolean" },
                        "due_stated": { "type": "boolean" },
                        "doubt": { "type": "string", "enum": ["none", "who", "when", "decided", "differs", "number", "subject", "other"] }
                    },
                    "required": ["id", "verdict", "owner_stated", "due_stated", "doubt"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["results"],
        "additionalProperties": false
    });

    // A checker that cannot be reached is not asked again for the rest.
    let unreachable = AtomicBool::new(false);
    let total = batches.len();
    let (answers, finished) = in_rounds(
        &batches,
        input.provider.at_once(),
        &|_, batch: &Vec<usize>| {
            if unreachable.load(Ordering::Acquire) {
                return None;
            }
            let listing: String = batch
                .iter()
                .enumerate()
                .map(|(number, &position)| format!("### {}\n{}\n", number + 1, blocks[position]))
                .collect();
            match ask_again(input, system, &listing, &schema) {
                Ok(answer) => Some(read_verdicts(&answer)),
                Err(e) => {
                    warn!(
                        "Could not verify {} entries ({e:#}); marking them for a look",
                        batch.len()
                    );
                    if !format!("{e:#}").contains("JSON") {
                        unreachable.store(true, Ordering::Release);
                    }
                    None
                }
            }
        },
        &|answers: &[Option<BTreeMap<usize, Verdict>>]| {
            progress(0.85 + 0.15 * answers.len() as f32 / total.max(1) as f32);
            !cancelled.load(Ordering::Acquire)
        },
    );
    if !finished {
        return None;
    }
    Some(apply_verdicts(entries, &targets, &batches, &answers))
}

/// Who a task can be given to, beyond the people in the meeting: the side
/// that promised it, by the section it is in.
fn side_of(section: &str) -> Option<&'static str> {
    match section {
        "our_tasks" => Some("우리(상담 담당)"),
        "customer_tasks" => Some("고객"),
        _ => None,
    }
}

/// Reads the tasks together: drops what is not a promise for later, makes
/// one task of a promise told more than once (keeping every time it was
/// said), and gives each the one who does it, picked from `people` or the
/// side that promised. Nothing new is written: the text of a task is one
/// the extraction gave, an owner is one of the offered names, a date one of
/// those the task's tellings gave.
fn tidy_tasks(
    input: &Input<'_>,
    template: &Template,
    entries: &mut Vec<(String, Entry)>,
    transcript: &Transcript,
    people: &[String],
) -> Result<()> {
    let tasks: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, (key, _))| {
            template
                .section(key)
                .is_some_and(|section| section.kind == SectionKind::Actions)
        })
        .map(|(at, _)| at)
        .collect();
    if tasks.is_empty() {
        return Ok(());
    }
    let mut owners: Vec<String> = people
        .iter()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();
    for &at in &tasks {
        if let Some(side) = side_of(&entries[at].0) {
            if !owners.iter().any(|owner| owner == side) {
                owners.push(side.to_string());
            }
        }
    }
    let listing: String = tasks
        .iter()
        .enumerate()
        .map(|(number, &at)| {
            let (key, entry) = &entries[at];
            let section = template
                .section(key)
                .map_or(key.as_str(), |s| s.title.as_str());
            let mut line = format!("{}. [{section}] {}", number + 1, entry.text);
            if !entry.owner.is_empty() {
                line.push_str(&format!(" / 담당: {}", entry.owner));
            }
            if !entry.due.is_empty() {
                line.push_str(&format!(" / 기한: {}", entry.due));
            }
            if let Some(source) = entry.sources.first() {
                let at = transcript
                    .segments
                    .iter()
                    .position(|segment| segment.start_ms == source.ms);
                // The line before, where someone may have handed it over.
                if let Some(before) = at
                    .and_then(|at| at.checked_sub(1))
                    .and_then(|at| transcript.segments.get(at))
                {
                    line.push_str(&format!(
                        "\n   (앞 줄) {}: {}",
                        transcript.speaker_name(before.speaker),
                        before.text
                    ));
                }
                let speaker = at
                    .map(|at| transcript.speaker_name(transcript.segments[at].speaker))
                    .unwrap_or_default();
                line.push_str(&format!(
                    "\n   [{}] {speaker}: 「{}」",
                    clock(source.ms),
                    source.quote
                ));
            }
            line.push('\n');
            line
        })
        .collect();
    let system = "당신은 회의록의 할 일 목록을 다듬는 서기입니다. 녹음에서 뽑은 할 일 후보가 번호와 함께, 근거가 된 말과 그 말을 한 사람과 같이 주어집니다.\n\n\
         1. promise: 회의가 끝난 뒤 누군가 실제로 해야 하는 약속이면 true입니다. 회의 중에 이미 한 일이나 하고 있는 일을 묘사한 것(예: \"상담 중에 견적서를 쓰고 있음\", \"가전 자리를 반영해 제안함\"), 지나가며 한 말, 일반적인 설명, 가벼운 안내나 권유(예: \"사례 영상을 참고해 달라\", \"편하게 연락 달라\")는 false입니다. 할 일은 적을수록 좋습니다. 한두 시간 회의에서 남는 할 일은 보통 다섯에서 열 개입니다.\n\
         2. 같은 약속이 여러 번 나왔으면 한 묶음(ids)으로 모읍니다. 묶음의 첫 번호는 그 약속을 가장 잘 적은 것으로 합니다. 비슷해 보여도 다른 일이면 따로 둡니다.\n\
         3. owner: 그 일을 할 사람을 owners 목록에서만 고르고, 사람 이름을 먼저 봅니다. 근거가 된 말을 한 사람이 이름으로 적혀 있고 \"제가 하겠습니다\"처럼 자기가 하겠다고 했으면 그 사람입니다. 다른 사람이 이름을 대며 맡긴 말(\"그건 박 소장님이 봐 주실 거예요\")이 바로 앞에 있으면 그 사람입니다. 그렇게 사람을 알 수 없을 때만 약속한 쪽(목록에 있는 '우리(상담 담당)'이나 '고객')을 고르고, 그것도 알 수 없으면 빈 문자열로 둡니다.\n\
         4. weight: 3은 돈·일정·계약·고객의 결정에 바로 걸린 약속, 2는 누군가 실제로 해야 하는 작업, 1은 가벼운 안내·추천·참고 자료 보내기입니다.\n\
         5. 회의 중에 이미 끝낸 일(\"링크를 전달함\", \"도면을 고침\", \"사이즈를 확인함\", \"추천함\")은 앞으로 할 약속이 아니므로 promise를 false로 합니다.\n\
         6. 모든 번호가 정확히 한 묶음에 들어가야 합니다. 새 내용을 지어내지 않습니다.";
    let user = format!(
        "owners: {}\n\n할 일 후보:\n{listing}",
        if owners.is_empty() {
            "(없음)".to_string()
        } else {
            owners.join(", ")
        }
    );
    let schema = json!({
        "type": "object",
        "properties": {
            "groups": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "ids": { "type": "array", "items": { "type": "integer" } },
                        "promise": { "type": "boolean" },
                        "owner": { "type": "string" },
                        "weight": { "type": "integer" }
                    },
                    "required": ["ids", "promise", "owner", "weight"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["groups"],
        "additionalProperties": false
    });
    let answer = ask_again(input, system, &user, &schema)?;
    let mut groups = read_task_groups(&answer, tasks.len());
    keep_weightiest(&mut groups, entries, &tasks);
    apply_task_groups(entries, &tasks, &groups, &owners);
    Ok(())
}

/// A group of task numbers (from 0) that are one promise, or none.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskGroup {
    ids: Vec<usize>,
    promise: bool,
    owner: String,
    /// 3 for money, dates, the contract or the customer's decision; 2 for
    /// work to do; 1 for a tip or a suggestion.
    weight: u8,
}

/// Leaves a meeting a list of tasks people go through: light ones go when
/// there are more than a few, and no more than [`MOST_TASKS`] stay, the
/// weightiest and then the earliest. What goes is still in the story.
fn keep_weightiest(groups: &mut [TaskGroup], entries: &[(String, Entry)], tasks: &[usize]) {
    let first_ms = |group: &TaskGroup| {
        group
            .ids
            .iter()
            .filter_map(|&id| entries[tasks[id]].1.sources.first().map(|s| s.ms))
            .min()
            .unwrap_or(0)
    };
    let promises = groups.iter().filter(|g| g.promise).count();
    if promises > ENOUGH_TASKS {
        for group in groups.iter_mut().filter(|g| g.promise && g.weight <= 1) {
            group.promise = false;
        }
    }
    let mut kept: Vec<(u8, u64, usize)> = groups
        .iter()
        .enumerate()
        .filter(|(_, g)| g.promise)
        .map(|(at, g)| (g.weight, first_ms(g), at))
        .collect();
    if kept.len() > MOST_TASKS {
        kept.sort_by_key(|&(weight, ms, _)| (std::cmp::Reverse(weight), ms));
        for &(_, _, at) in kept.iter().skip(MOST_TASKS) {
            groups[at].promise = false;
        }
    }
}

/// The groups in an answer, each task in one group at most; numbers the
/// answer leaves out are kept as they were, each on its own.
fn read_task_groups(answer: &Value, count: usize) -> Vec<TaskGroup> {
    let mut seen = vec![false; count];
    let mut groups: Vec<TaskGroup> = Vec::new();
    for group in answer
        .get("groups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let ids: Vec<usize> = group
            .get("ids")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .filter_map(|id| (id as usize).checked_sub(1))
            .filter(|&id| id < count)
            .filter(|&id| !std::mem::replace(&mut seen[id], true))
            .collect();
        if ids.is_empty() {
            continue;
        }
        groups.push(TaskGroup {
            ids,
            promise: group
                .get("promise")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            owner: group
                .get("owner")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string(),
            weight: group
                .get("weight")
                .and_then(Value::as_u64)
                .map_or(2, |weight| weight.clamp(1, 3) as u8),
        });
    }
    for (id, seen) in seen.iter().enumerate() {
        if !seen {
            groups.push(TaskGroup {
                ids: vec![id],
                promise: true,
                owner: String::new(),
                weight: 2,
            });
        }
    }
    groups
}

/// Makes the groups so: the first task of each promise takes the times and
/// doubts of the others, and the others and what is no promise go.
fn apply_task_groups(
    entries: &mut Vec<(String, Entry)>,
    tasks: &[usize],
    groups: &[TaskGroup],
    owners: &[String],
) {
    let mut gone: HashSet<usize> = HashSet::new();
    for group in groups {
        let positions: Vec<usize> = group.ids.iter().map(|&id| tasks[id]).collect();
        if !group.promise {
            gone.extend(positions);
            continue;
        }
        let keep = positions[0];
        let mut sources: Vec<Source> = Vec::new();
        let mut reasons: Vec<CheckReason> = Vec::new();
        let mut due = String::new();
        let mut owner_said = String::new();
        let mut done = false;
        for &at in &positions {
            let entry = &entries[at].1;
            for source in &entry.sources {
                if !sources.iter().any(|s| s.ms == source.ms) {
                    sources.push(source.clone());
                }
            }
            for reason in &entry.reasons {
                if !reasons.contains(reason) {
                    reasons.push(*reason);
                }
            }
            if due.is_empty() {
                due = entry.due.clone();
            }
            if owner_said.is_empty() && !entry.owner.starts_with("화자") {
                owner_said = entry.owner.clone();
            }
            done |= entry.done;
        }
        sources.sort_by_key(|source| source.ms);
        let entry = &mut entries[keep].1;
        entry.sources = sources;
        entry.reasons = reasons;
        entry.due = due;
        entry.done = done;
        // An owner from the offered names; one the recording named stays
        // when the answer gives none of them.
        entry.owner = if owners.contains(&group.owner) {
            group.owner.clone()
        } else {
            owner_said
        };
        gone.extend(positions.into_iter().skip(1));
    }
    let mut at = 0;
    entries.retain(|_| {
        let keep = !gone.contains(&at);
        at += 1;
        keep
    });
}

/// Gives tasks to people by name where the recording says who: a task the
/// customer's side promised goes to the customer by name when the meeting
/// has one, and one left to our side (or to no one) goes to the speaker who
/// promised it in the first person ("제가 보내드릴게요"), when that speaker is
/// named and is not the customer. Nothing is guessed: without such words
/// the side stays.
fn owners_by_name(
    template: &Template,
    entries: &mut [(String, Entry)],
    transcript: &Transcript,
    customer: &str,
) {
    let customer = customer.trim();
    for (key, entry) in entries.iter_mut() {
        if !template
            .section(key)
            .is_some_and(|section| section.kind == SectionKind::Actions)
        {
            continue;
        }
        let side = side_of(key);
        if side == Some("고객") {
            if entry.owner == "고객" && !customer.is_empty() {
                entry.owner = customer.to_string();
            }
            continue;
        }
        if !(entry.owner.is_empty() || side.is_some_and(|side| entry.owner == side)) {
            continue;
        }
        let Some(source) = entry.sources.first() else {
            continue;
        };
        let Some(line) = transcript
            .segments
            .iter()
            .find(|segment| segment.start_ms == source.ms)
        else {
            continue;
        };
        let name = transcript.speaker_name(line.speaker);
        let named = !name.starts_with("화자") && !super::facts::is_customer(&name, customer);
        let squeezed: String = line.text.chars().filter(|c| !c.is_whitespace()).collect();
        let first_person = ["제가", "내가", "저는제가"]
            .iter()
            .any(|word| squeezed.contains(word));
        if named && first_person {
            entry.owner = name;
        }
    }
}

/// How much an entry matters to be checked: tasks and decisions, money and
/// dates first.
fn weight(template: &Template, key: &str, entry: &Entry) -> u32 {
    let mut weight = 0;
    if template
        .section(key)
        .is_some_and(|section| section.kind == SectionKind::Actions)
    {
        weight += 4;
    } else if key == "decisions" {
        weight += 3;
    }
    if key == "budget" || key == "schedule" || !entry.due.is_empty() {
        weight += 2;
    }
    if !numbers_in(&entry.text).is_empty() {
        weight += 2;
    }
    if entry.reasons.iter().any(|r| {
        matches!(
            r,
            CheckReason::NumberNotInSource | CheckReason::UnitOdd | CheckReason::SumsDisagree
        )
    }) {
        weight += 2;
    }
    // Said both ways, or maybe someone else's: what a person most needs to
    // hear before relying on the minutes.
    if entry
        .reasons
        .iter()
        .any(|r| matches!(r, CheckReason::Conflicting | CheckReason::OtherSubject))
    {
        weight += 3;
    }
    if entry.reasons.iter().any(|reason| {
        matches!(
            reason,
            CheckReason::WeakSupport
                | CheckReason::NumbersDiffer
                | CheckReason::OwnerUnclear
                | CheckReason::NotSettled
        )
    }) {
        weight += 1;
    }
    weight
}

/// Leaves at most `most` entries to be checked by ear, the weightiest. Of
/// the rest, one the checker found no clear support for is dropped (it may
/// not have been said); the others are kept, shown faintly. Returns how many
/// were dropped.
fn keep_checks(template: &Template, entries: &mut Vec<(String, Entry)>, most: usize) -> u32 {
    let mut doubted: Vec<(usize, u32, u64)> = entries
        .iter()
        .enumerate()
        .filter(|(_, (_, entry))| entry.needs_check())
        .map(|(at, (key, entry))| {
            let ms = entry.sources.first().map_or(0, |source| source.ms);
            (at, weight(template, key, entry), ms)
        })
        .collect();
    if doubted.len() <= most {
        return 0;
    }
    doubted.sort_by_key(|&(_, weight, ms)| (std::cmp::Reverse(weight), ms));
    let mut gone: HashSet<usize> = HashSet::new();
    for &(at, _, _) in doubted.iter().skip(most) {
        let entry = &mut entries[at].1;
        if entry
            .reasons
            .iter()
            .any(|r| matches!(r, CheckReason::WeakSupport | CheckReason::OtherSubject))
        {
            gone.insert(at);
        } else {
            entry.reasons.clear();
            entry.faint = true;
        }
    }
    let mut at = 0;
    entries.retain(|_| {
        let keep = !gone.contains(&at);
        at += 1;
        keep
    });
    gone.len() as u32
}

fn push_reason(entry: &mut Entry, reason: CheckReason) {
    if !entry.reasons.contains(&reason) {
        entry.reasons.push(reason);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seogi::templates::built_in;
    use crate::seogi::transcript::{parse_clock, Speaker};

    fn task(text: &str, owner: &str, due: &str, ms: u64, reasons: Vec<CheckReason>) -> Entry {
        Entry {
            text: text.into(),
            owner: owner.into(),
            due: due.into(),
            sources: vec![Source {
                ms,
                quote: text.into(),
            }],
            reasons,
            done: false,
            faint: false,
        }
    }

    fn consult() -> Template {
        built_in()
            .into_iter()
            .find(|template| template.id == "consult")
            .unwrap()
    }

    #[test]
    fn one_promise_told_three_times_is_one_task_and_what_was_done_goes() {
        let mut entries = vec![
            (
                "our_tasks".to_string(),
                task("견적서를 보내기로 함", "", "", 3_000, vec![]),
            ),
            (
                "our_tasks".to_string(),
                task("상담 중 견적서를 쓰고 있음", "", "", 4_000, vec![]),
            ),
            (
                "decisions".to_string(),
                task("마루는 살리기로 함", "", "", 5_000, vec![]),
            ),
            (
                "our_tasks".to_string(),
                task(
                    "견적서를 바로 보내 드림",
                    "",
                    "오늘",
                    9_000,
                    vec![CheckReason::UnsureSpeech],
                ),
            ),
            (
                "customer_tasks".to_string(),
                task("냉장고 치수를 재서 알려 주기", "화자 2", "", 7_000, vec![]),
            ),
        ];
        let tasks = vec![0, 1, 3, 4];
        let owners = vec![
            "민트".to_string(),
            "우리(상담 담당)".to_string(),
            "고객".to_string(),
        ];
        let answer = json!({ "groups": [
            { "ids": [1, 3], "promise": true, "owner": "민트" },
            { "ids": [2], "promise": false, "owner": "" },
            // A number twice, one out of range, and a name not offered.
            { "ids": [3, 9], "promise": true, "owner": "" },
        ]});
        let groups = read_task_groups(&answer, tasks.len());
        // Number 4 was left out of the answer: it stays as it was.
        assert_eq!(groups.len(), 3);
        apply_task_groups(&mut entries, &tasks, &groups, &owners);

        let texts: Vec<&str> = entries.iter().map(|(_, e)| e.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "견적서를 보내기로 함",
                "마루는 살리기로 함",
                "냉장고 치수를 재서 알려 주기"
            ]
        );
        let merged = &entries[0].1;
        assert_eq!(merged.owner, "민트");
        assert_eq!(merged.due, "오늘");
        assert_eq!(
            merged.sources.iter().map(|s| s.ms).collect::<Vec<_>>(),
            [3_000, 9_000]
        );
        assert_eq!(merged.reasons, [CheckReason::UnsureSpeech]);
        // A speaker label never becomes the owner.
        assert_eq!(entries[2].1.owner, "");

        let other = json!({ "groups": [{ "ids": [1], "promise": true, "owner": "아무개" }] });
        let groups = read_task_groups(&other, 1);
        let mut one = vec![(
            "our_tasks".to_string(),
            task("도면 보내기", "김서윤 실장", "", 0, vec![]),
        )];
        apply_task_groups(&mut one, &[0], &groups, &owners);
        assert_eq!(
            one[0].1.owner, "김서윤 실장",
            "a name not offered keeps the one said"
        );
    }

    #[test]
    fn a_story_keeps_only_sentences_that_rest_on_items() {
        let budget = task("예산은 1억 원을 넘기지 않기로 함", "", "", 60_000, vec![]);
        let kitchen = task("주방을 확장부로 옮기기로 함", "", "", 120_000, vec![]);
        let cited = vec![&budget, &kitchen];
        let answer = json!({ "parts": [
            { "title": "예산", "sentences": [
                { "text": "고객은 예산을 1억 원 안으로 잡았다.", "items": [1] },
                // A number no item has.
                { "text": "업체는 9천만 원을 제안했다.", "items": [1] },
                // Rests on nothing.
                { "text": "분위기는 좋았다.", "items": [] },
                { "text": "없는 항목에 기댄 문장이다.", "items": [7] }
            ]},
            { "title": "빈 부분", "sentences": [{ "text": "근거 없음", "items": [] }] },
            { "title": "주방", "sentences": [
                { "text": "주방은 확장부로 옮기기로 했다.", "items": [2, 1] }
            ]}
        ]});
        let story = read_story(&answer, &cited);
        assert_eq!(story.len(), 2);
        assert_eq!(story[0].title, "예산");
        assert_eq!(story[0].text, "고객은 예산을 1억 원 안으로 잡았다.");
        assert_eq!(
            story[0].sources.iter().map(|s| s.ms).collect::<Vec<_>>(),
            [60_000]
        );
        assert_eq!(
            story[1].sources.iter().map(|s| s.ms).collect::<Vec<_>>(),
            [60_000, 120_000]
        );
    }

    #[test]
    fn a_long_list_of_tasks_keeps_the_weightiest() {
        let entries: Vec<(String, Entry)> = (0..14)
            .map(|n| {
                (
                    "our_tasks".to_string(),
                    task(&format!("일 {n}"), "", "", 1_000 * n, vec![]),
                )
            })
            .collect();
        let tasks: Vec<usize> = (0..14).collect();
        let mut groups: Vec<TaskGroup> = (0..14)
            .map(|n| TaskGroup {
                ids: vec![n],
                promise: true,
                owner: String::new(),
                // Two light ones, three that matter most, the rest work.
                weight: match n {
                    0 | 1 => 1,
                    11..=13 => 3,
                    _ => 2,
                },
            })
            .collect();
        keep_weightiest(&mut groups, &entries, &tasks);
        let kept: Vec<usize> = groups
            .iter()
            .filter(|g| g.promise)
            .map(|g| g.ids[0])
            .collect();
        assert_eq!(kept.len(), MOST_TASKS);
        assert!(!kept.contains(&0) && !kept.contains(&1));
        assert!(kept.contains(&11) && kept.contains(&12) && kept.contains(&13));
        // Of the work, the earliest stay.
        assert!(kept.contains(&2) && !kept.contains(&10));

        // Few tasks: a light one stays.
        let mut few = vec![TaskGroup {
            ids: vec![0],
            promise: true,
            owner: String::new(),
            weight: 1,
        }];
        keep_weightiest(&mut few, &entries, &tasks);
        assert!(few[0].promise);
    }

    #[test]
    fn only_the_weightiest_doubts_are_put_to_the_person() {
        let template = consult();
        let mut entries: Vec<(String, Entry)> = Vec::new();
        for n in 0..12 {
            entries.push((
                "materials".to_string(),
                task(
                    &format!("자재 메모 {n}"),
                    "",
                    "",
                    60_000 * n,
                    vec![CheckReason::UnsureSpeech],
                ),
            ));
        }
        entries.push((
            "materials".to_string(),
            task(
                "근거가 약한 자재",
                "",
                "",
                1,
                vec![CheckReason::WeakSupport],
            ),
        ));
        entries.push((
            "our_tasks".to_string(),
            task(
                "견적서 보내기",
                "",
                "금요일까지",
                2,
                vec![CheckReason::UnsureSpeech],
            ),
        ));
        entries.push((
            "budget".to_string(),
            task("예산 1억", "", "", 3, vec![CheckReason::OverlappingSpeech]),
        ));
        entries.push((
            "decisions".to_string(),
            task("마루 유지", "", "", 4, vec![]),
        ));

        let dropped = keep_checks(&template, &mut entries, 3);
        let mut checked: Vec<&str> = entries
            .iter()
            .filter(|(_, e)| e.needs_check())
            .map(|(_, e)| e.text.as_str())
            .collect();
        checked.sort_unstable();
        // The task and the money first, then the earliest of the rest.
        assert_eq!(checked, ["견적서 보내기", "예산 1억", "자재 메모 0"]);
        // The unsupported one beyond the few is left out; the rest stay, faint.
        assert_eq!(dropped, 1);
        assert!(!entries.iter().any(|(_, e)| e.text == "근거가 약한 자재"));
        assert_eq!(entries.iter().filter(|(_, e)| e.faint).count(), 11);
        assert!(entries
            .iter()
            .any(|(_, e)| e.text == "마루 유지" && !e.faint));

        // Few enough: nothing changes.
        let mut few = vec![(
            "budget".to_string(),
            task("예산", "", "", 0, vec![CheckReason::WeakSupport]),
        )];
        assert_eq!(keep_checks(&template, &mut few, 3), 0);
        assert!(few[0].1.needs_check());
    }

    fn line(start_ms: u64, speaker: u32, text: &str) -> Segment {
        Segment {
            start_ms,
            end_ms: start_ms + 4_000,
            speaker,
            text: text.to_string(),
            confidence: Some(0.9),
            unsure: false,
            overlap: false,
        }
    }

    fn transcript() -> Transcript {
        Transcript {
            language: "ko".into(),
            model: "test".into(),
            segments: vec![
                line(1_000, 1, "안녕하세요 저는 디자인을 맡은 김서윤 실장입니다."),
                line(6_000, 2, "예산은 4,500만 원 안쪽으로 맞추고 싶어요."),
                line(
                    11_000,
                    1,
                    "네, 중문은 신발장 도면을 다시 그려서 금요일까지 보내드리겠습니다.",
                ),
                line(
                    16_000,
                    2,
                    "신발장이 줄어드는 건 조금 고민되네요. 일단 생각해 볼게요.",
                ),
            ],
            speakers: vec![
                Speaker {
                    id: 1,
                    name: "김서윤 실장".into(),
                },
                Speaker {
                    id: 2,
                    name: "화자 2".into(),
                },
            ],
            corrections: Vec::new(),
        }
    }

    fn raw(section: &str, text: &str, owner: &str, due: &str, ts: &str, quote: &str) -> RawItem {
        RawItem {
            section: section.into(),
            text: text.into(),
            owner: owner.into(),
            due: due.into(),
            near_ms: parse_clock(ts),
            quote: quote.into(),
        }
    }

    #[test]
    fn an_item_takes_its_time_from_where_its_quote_is_found() {
        let transcript = transcript();
        let index = Index::new(&transcript);
        let template = built_in().remove(1);
        // The model gives the wrong time; the quote says where it really is.
        let item = check_item(
            raw(
                "our_tasks",
                "신발장 도면을 다시 그려 금요일까지 보낸다",
                "김서윤 실장",
                "금요일까지",
                "00:00:01",
                "신발장 도면을 다시 그려서 금요일까지",
            ),
            &template,
            &index,
            &transcript,
        )
        .unwrap();
        assert_eq!(item.entry.sources[0].ms, 11_000);
        assert_eq!(item.entry.owner, "김서윤 실장");
        assert!(item.entry.reasons.is_empty());
    }

    #[test]
    fn an_item_whose_quote_is_not_in_the_recording_is_dropped() {
        let transcript = transcript();
        let index = Index::new(&transcript);
        let template = built_in().remove(1);
        assert!(check_item(
            raw(
                "decisions",
                "욕실은 전체 철거하기로 했다",
                "",
                "",
                "00:00:06",
                "욕실은 전체 철거하는 걸로 하죠"
            ),
            &template,
            &index,
            &transcript
        )
        .is_none());
    }

    #[test]
    fn a_number_that_is_not_in_the_cited_lines_marks_the_item() {
        let transcript = transcript();
        let index = Index::new(&transcript);
        let template = built_in().remove(1);
        let right = check_item(
            raw(
                "budget",
                "예산은 4500만 원 안쪽",
                "",
                "",
                "00:00:06",
                "예산은 4,500만 원 안쪽으로",
            ),
            &template,
            &index,
            &transcript,
        )
        .unwrap();
        assert!(right.entry.reasons.is_empty());
        let wrong = check_item(
            raw(
                "budget",
                "예산은 5400만 원 안쪽",
                "",
                "",
                "00:00:06",
                "예산은 4,500만 원 안쪽으로",
            ),
            &template,
            &index,
            &transcript,
        )
        .unwrap();
        assert_eq!(wrong.entry.reasons, vec![CheckReason::NumberNotInSource]);
    }

    #[test]
    fn a_speaker_label_is_never_an_owner() {
        let transcript = transcript();
        let index = Index::new(&transcript);
        let template = built_in().remove(1);
        let item = check_item(
            raw(
                "customer_tasks",
                "신발장 폭을 줄일지 생각해 본다",
                "화자 2",
                "",
                "00:00:16",
                "일단 생각해 볼게요",
            ),
            &template,
            &index,
            &transcript,
        )
        .unwrap();
        assert_eq!(item.entry.owner, "");
        // A name the recording never has is not an owner either.
        let item = check_item(
            raw(
                "our_tasks",
                "도면을 보낸다",
                "한지안 대리",
                "",
                "00:00:11",
                "금요일까지 보내드리겠습니다",
            ),
            &template,
            &index,
            &transcript,
        )
        .unwrap();
        assert_eq!(item.entry.owner, "");
    }

    #[test]
    fn an_unsure_line_marks_what_rests_on_it() {
        let mut transcript = transcript();
        transcript.segments[2].unsure = true;
        let index = Index::new(&transcript);
        let template = built_in().remove(1);
        let item = check_item(
            raw(
                "our_tasks",
                "도면을 보낸다",
                "",
                "",
                "00:00:11",
                "금요일까지 보내드리겠습니다",
            ),
            &template,
            &index,
            &transcript,
        )
        .unwrap();
        assert_eq!(item.entry.reasons, vec![CheckReason::UnsureSpeech]);
    }

    #[test]
    fn a_retyped_quote_is_found_loosely_and_marked() {
        let transcript = transcript();
        let index = Index::new(&transcript);
        // Spacing and punctuation never matter.
        let (lines, score) = index
            .find("신발장도면을 다시그려서, 금요일까지", None)
            .unwrap();
        assert_eq!((*lines.start(), score), (2, 1.0));
        // A few changed characters still find the line, as a loose match.
        let (lines, score) = index
            .find(
                "중문은 신발장 도면을 새로 그려서 금요일까지 보내드리겠습니다",
                None,
            )
            .unwrap();
        assert_eq!(*lines.start(), 2);
        assert!(score < 1.0);
        assert!(index
            .find("전혀 다른 말을 하고 있습니다 여기서는", None)
            .is_none());
    }

    #[test]
    fn numbers_are_read_whole_with_grouping_commas() {
        assert_eq!(
            numbers_in("예산 4,500만 원, 12월 8일"),
            vec!["4500", "12", "8"]
        );
        assert_eq!(numbers_in("숫자 없음"), Vec::<String>::new());
        assert!(digits_of("폭 2400에 280만 원").contains("2400"));
    }

    #[test]
    fn windows_keep_marks_in_place_and_overlap_by_two_lines() {
        let mut transcript = transcript();
        // Enough text for more than one window.
        for i in 0..40 {
            transcript.segments.push(line(
                30_000 + i * 10_000,
                (i % 2 + 1) as u32,
                &"자재와 일정에 대해서 계속 이야기를 이어 갑니다. ".repeat(6),
            ));
        }
        let windows = windows(&transcript, &[12_000], &[], None);
        assert!(windows.len() > 1);
        let (text, starts) = numbered(&windows[0]);
        // The mark sits between the lines it fell between, and has no number.
        let mark_at = text.find("   [중요 표시 00:00:12]").unwrap();
        assert!(text.find("3. [00:00:11]").unwrap() < mark_at);
        assert!(mark_at < text.find("4. [00:00:16]").unwrap());
        assert_eq!(starts[..4], [1_000, 6_000, 11_000, 16_000]);
        // The second window starts with the last two lines of the first.
        let first = &windows[0];
        assert_eq!(windows[1][0].text, first[first.len() - 2].text);
        assert!(windows.iter().all(|w| {
            w.iter().map(|l| l.text.chars().count()).sum::<usize>() <= WINDOW_CHARS + 400
        }));
    }

    #[test]
    fn an_extraction_answer_is_read_by_line_number() {
        let answer = json!({"lines": [
            {"line": 1, "items": []},
            {"line": 2, "items": [
                {"section": "budget", "text": "예산은 4500만 원 안쪽", "owner": "", "due": "", "quote": "예산은 4,500만 원"}
            ]},
            {"line": 9, "items": [{"section": "budget", "text": "없는 줄", "owner": "", "due": "", "quote": "x"}]},
            {"line": 3, "items": [{"section": "open", "text": "", "owner": "", "due": "", "quote": ""}]}
        ]});
        let (covered, items) = read_extraction(&answer, &[1_000, 6_000, 11_000]);
        assert_eq!(covered, 3);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].near_ms, Some(6_000));
    }

    fn item(section: &str, text: &str, line: usize, ms: u64) -> Item {
        Item {
            section: section.into(),
            entry: Entry {
                text: text.into(),
                owner: String::new(),
                due: String::new(),
                sources: vec![Source {
                    ms,
                    quote: text.into(),
                }],
                reasons: Vec::new(),
                done: false,
                faint: false,
            },
            lines: line..=line,
        }
    }

    #[test]
    fn the_same_thing_said_twice_is_one_entry_but_different_facts_stay_apart() {
        let a = item(
            "decisions",
            "방 3개와 거실은 강마루로 하기로 했다",
            5,
            88_000,
        );
        let b = item("decisions", "방 3개와 거실은 강마루로 한다", 5, 88_000);
        assert!(same_thing(&a, &b));
        // Alike sentences with different numbers are different facts.
        let c = item(
            "budget",
            "붙박이장은 슬라이딩으로 하면 280만 원 정도이다",
            9,
            134_000,
        );
        let d = item(
            "budget",
            "붙박이장은 여닫이로 하면 40만 원 정도 줄어든다",
            10,
            145_000,
        );
        assert!(!same_thing(&c, &d));
        // The same words in different sections are not merged.
        let e = item(
            "requests",
            "방 3개와 거실은 강마루로 하기로 했다",
            5,
            88_000,
        );
        assert!(!same_thing(&a, &e));
        // Different topics in one section stay apart.
        let f = item(
            "decisions",
            "샷시는 교체하지 않고 실리콘만 다시 쏜다",
            12,
            152_000,
        );
        assert!(!same_thing(&a, &f));
    }

    #[test]
    fn requests_run_a_few_at_a_time_and_keep_their_order() {
        use std::sync::atomic::AtomicUsize;
        let things: Vec<usize> = (0..10).collect();
        let running = AtomicUsize::new(0);
        let most = AtomicUsize::new(0);
        let rounds = std::cell::Cell::new(0);
        let (results, finished) = in_rounds(
            &things,
            3,
            &|at, thing: &usize| {
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(15));
                running.fetch_sub(1, Ordering::SeqCst);
                (at, thing * 2)
            },
            &|_| {
                rounds.set(rounds.get() + 1);
                true
            },
        );
        assert!(finished);
        assert_eq!(results.len(), 10);
        assert!(results
            .iter()
            .enumerate()
            .all(|(at, &(index, doubled))| index == at && doubled == at * 2));
        // Never more than three at once, and really side by side.
        assert_eq!(most.load(Ordering::SeqCst), 3);
        assert_eq!(rounds.get(), 4);

        // One at a time runs on the caller's own thread.
        let here = std::thread::current().id();
        let (same, _) = in_rounds(
            &things,
            1,
            &|_, _| std::thread::current().id() == here,
            &|_| true,
        );
        assert!(same.iter().all(|same| *same));

        // Saying stop after a round leaves the rest undone.
        let (some, finished) = in_rounds(&things, 3, &|at, _| at, &|done: &[usize]| done.len() < 6);
        assert!(!finished);
        assert_eq!(some, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_long_meetings_entries_are_checked_a_batch_at_a_time() {
        // Ninety entries of some 300 characters: no request over the limits,
        // every entry in exactly one, in order.
        let sizes = vec![300usize; 90];
        let batches = verify_batches(&sizes);
        assert!(batches.len() >= 6);
        for batch in &batches {
            assert!(batch.len() <= VERIFY_ENTRIES);
            assert!(batch.iter().map(|&at| sizes[at]).sum::<usize>() <= VERIFY_CHARS);
        }
        let all: Vec<usize> = batches.iter().flatten().copied().collect();
        assert_eq!(all, (0..90).collect::<Vec<_>>());

        // Short entries are held to the count, and one too long for a
        // request still gets asked about, alone.
        assert!(verify_batches(&[40; 50])
            .iter()
            .all(|batch| batch.len() <= VERIFY_ENTRIES));
        assert_eq!(
            verify_batches(&[200, 9_000, 200]),
            vec![vec![0], vec![1], vec![2]]
        );
        assert!(verify_batches(&[]).is_empty());
    }

    #[test]
    fn a_failed_batch_marks_only_its_own_entries() {
        let entry = |text: &str| {
            let mut entry = item("decisions", text, 1, 1_000).entry;
            entry.owner = "김서윤 실장".into();
            entry.due = "금요일".into();
            ("decisions".to_string(), entry)
        };
        // Five entries; the second is a note that is not put to the checker.
        let mut entries = vec![
            entry("가"),
            (
                "discussed".to_string(),
                item("discussed", "나", 1, 2_000).entry,
            ),
            entry("다"),
            entry("라"),
            entry("마"),
        ];
        let targets = vec![0, 2, 3, 4];
        // Two requests of two; numbers start at 1 in each.
        let batches = vec![vec![0, 1], vec![2, 3]];
        let first: BTreeMap<usize, Verdict> = BTreeMap::from([
            (
                1,
                ("supported".to_string(), true, false, "none".to_string()),
            ),
            (
                2,
                ("unsupported".to_string(), true, true, "none".to_string()),
            ),
        ]);
        // The second request failed.
        let removed = apply_verdicts(&mut entries, &targets, &batches, &[Some(first), None]);

        assert_eq!(removed, 1);
        let texts: Vec<&str> = entries.iter().map(|(_, e)| e.text.as_str()).collect();
        assert_eq!(texts, ["가", "나", "라", "마"]);
        // Backed, but its date was not in the lines.
        assert!(entries[0].1.reasons.is_empty());
        assert_eq!(entries[0].1.owner, "김서윤 실장");
        assert!(entries[0].1.due.is_empty());
        // The note was never in question.
        assert!(entries[1].1.reasons.is_empty());
        // The failed request's entries are marked, not passed.
        assert_eq!(entries[2].1.reasons, vec![CheckReason::Unchecked]);
        assert_eq!(entries[3].1.reasons, vec![CheckReason::Unchecked]);

        // An answer that skips a number leaves that entry marked.
        let mut entries = vec![entry("가"), entry("나")];
        let partial: BTreeMap<usize, Verdict> =
            BTreeMap::from([(2, ("partly".to_string(), true, true, "who".to_string()))]);
        let removed = apply_verdicts(&mut entries, &[0, 1], &[vec![0, 1]], &[Some(partial)]);
        assert_eq!(removed, 0);
        assert_eq!(entries[0].1.reasons, vec![CheckReason::Unchecked]);
        // What is in doubt is said: here, who does it.
        assert_eq!(entries[1].1.reasons, vec![CheckReason::OwnerUnclear]);
    }

    #[test]
    fn the_summary_of_a_long_meeting_reads_its_tasks_and_decisions_first() {
        let template = built_in().remove(0);
        let actions = template
            .sections
            .iter()
            .find(|section| section.kind == SectionKind::Actions)
            .map(|section| section.key.clone())
            .unwrap();
        let note = template
            .sections
            .iter()
            .find(|section| !commits(&template, &section.key) && section.key != OPEN_SECTION)
            .map(|section| section.key.clone())
            .unwrap();

        // A short meeting is listed whole, in the order it was said.
        let short = vec![
            item(&note, "창호 견적을 두 곳에서 받아 보았다", 1, 1_000),
            item(&actions, "도면을 금요일까지 보낸다", 2, 2_000),
        ];
        let pairs = |items: Vec<Item>| -> Vec<(String, Entry)> {
            items.into_iter().map(|i| (i.section, i.entry)).collect()
        };
        let listing = summary_listing(&template, &pairs(short));
        assert!(listing.find("창호 견적").unwrap() < listing.find("도면을").unwrap());

        // A long one is cut to what a model reads, the tasks kept.
        let mut long: Vec<Item> = (0..400u64)
            .map(|n| {
                item(
                    &note,
                    &format!("{n}번째로 나눈 이야기는 자재와 일정에 관한 것이었다"),
                    n as usize,
                    n * 1_000,
                )
            })
            .collect();
        long.push(item(&actions, "도면을 금요일까지 보낸다", 400, 400_000));
        let listing = summary_listing(&template, &pairs(long));
        assert!(listing.chars().count() <= SUMMARY_CHARS);
        assert!(listing.starts_with("- ["));
        assert!(listing
            .lines()
            .next()
            .unwrap()
            .contains("도면을 금요일까지 보낸다"));
    }

    #[test]
    fn a_request_that_fails_in_passing_is_made_again() {
        let no_wait = [std::time::Duration::ZERO; 2];
        // Twice it fails, then it answers.
        let mut calls = 0;
        let answer = patiently(&no_wait, || {
            calls += 1;
            if calls < 3 {
                Err(anyhow!("Claude Code failed: overloaded"))
            } else {
                Ok(calls)
            }
        });
        assert_eq!(answer.unwrap(), 3);

        // It is not asked for ever.
        let mut calls = 0;
        let answer: Result<()> = patiently(&no_wait, || {
            calls += 1;
            Err(anyhow!("Claude Code failed: overloaded"))
        });
        assert!(answer.is_err());
        assert_eq!(calls, 3);

        // What waiting does not help comes back at once.
        for hopeless in [
            llm::CUT_OFF,
            "Claude Code did not answer within 15 minutes",
            "claude sonnet did not answer with JSON",
            "Claude Code is not installed",
        ] {
            let mut calls = 0;
            let answer: Result<()> = patiently(&no_wait, || {
                calls += 1;
                Err(anyhow!("{hopeless}"))
            });
            assert!(answer.is_err());
            assert_eq!(calls, 1, "{hopeless}");
        }
    }

    #[test]
    fn the_later_word_stands_and_an_unsettled_pair_is_marked_both_ways() {
        use crate::seogi::facts::Clash;
        let mut entries = vec![
            (
                "budget".to_string(),
                task("예산은 4,000만 원", "", "", 1_000, vec![]),
            ),
            (
                "budget".to_string(),
                task("예산은 5,000만 원", "", "", 9_000, vec![]),
            ),
            (
                "decisions".to_string(),
                task("방은 4개로 유지", "", "", 2_000, vec![]),
            ),
            (
                "open".to_string(),
                task("방 개수는 아직 미정", "", "", 8_000, vec![]),
            ),
            (
                "site".to_string(),
                task("현장은 32평", "", "", 3_000, vec![]),
            ),
            (
                "site".to_string(),
                task("현장은 58평", "", "", 7_000, vec![]),
            ),
        ];
        let pairs = vec![
            (0, 1, Clash::Numbers),
            (2, 3, Clash::SettledAndOpen),
            (4, 5, Clash::Numbers),
        ];
        // The budget was raised; the rooms were only asked about again; the
        // size could not be told (and the third answer never came).
        let verdicts: BTreeMap<usize, String> =
            [(1, "later".to_string()), (2, "earlier".to_string())].into();
        let gone = apply_reconciled(&mut entries, &pairs, &verdicts);
        assert_eq!(gone, 2);
        let texts: Vec<&str> = entries.iter().map(|(_, e)| e.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "예산은 5,000만 원",
                "방은 4개로 유지",
                "현장은 32평",
                "현장은 58평"
            ]
        );
        assert_eq!(entries[2].1.reasons, [CheckReason::Conflicting]);
        assert_eq!(entries[3].1.reasons, [CheckReason::Conflicting]);
        assert!(entries[0].1.reasons.is_empty());
    }

    #[test]
    fn what_is_said_after_the_meeting_ended_is_marked_and_not_kept() {
        let transcript = transcript();
        let index = Index::new(&transcript);
        let template = built_in().remove(1);
        let windows = windows(&transcript, &[], &[], Some(16_000));
        let text: Vec<&str> = windows[0].iter().map(|l| l.text.as_str()).collect();
        assert_eq!(text[3], AFTER_THE_END);
        let item = check_item(
            raw(
                "open",
                "신발장은 고민 중",
                "",
                "",
                "00:00:16",
                "신발장이 줄어드는 건 조금 고민되네요",
            ),
            &template,
            &index,
            &transcript,
        )
        .unwrap();
        assert!(after_the_end(&item, &index, Some(16_000)));
        assert!(!after_the_end(&item, &index, Some(20_000)));
        assert!(!after_the_end(&item, &index, None));
    }

    #[test]
    fn a_fact_from_the_speakers_own_home_is_marked_but_the_customers_is_not() {
        let mut transcript = transcript();
        transcript
            .segments
            .push(line(21_000, 1, "우리 집은 고층이라 진동이 있어요."));
        transcript
            .segments
            .push(line(26_000, 2, "저희 집은 32평이고 3층이에요."));
        transcript.speakers[1].name = "고객".into();
        let index = Index::new(&transcript);
        let template = built_in().remove(1);
        let check = |text: &str, ts: &str, quote: &str| {
            let mut item = check_item(
                raw("site", text, "", "", ts, quote),
                &template,
                &index,
                &transcript,
            )
            .unwrap();
            mark_elsewhere(&mut item, &template, &index, &transcript, "");
            item.entry.reasons
        };
        assert_eq!(
            check(
                "현장은 고층이다",
                "00:00:21",
                "우리 집은 고층이라 진동이 있어요"
            ),
            [CheckReason::OtherSubject]
        );
        assert!(check(
            "현장은 32평 3층이다",
            "00:00:26",
            "저희 집은 32평이고 3층이에요"
        )
        .is_empty());
    }

    #[test]
    fn numbers_are_compared_as_values_and_odd_amounts_are_marked() {
        let mut transcript = transcript();
        transcript
            .segments
            .push(line(21_000, 1, "천장은 아홉 평 정도 새로 합니다."));
        transcript.segments.push(line(
            26_000,
            1,
            "시공비까지 하면 80,900만원 정도 더 올라갈 수 있습니다.",
        ));
        transcript.segments.push(line(
            31_000,
            1,
            "복도 끝은 자당 20 기준이고 서재도 자당 25만 원입니다.",
        ));
        let index = Index::new(&transcript);
        let template = built_in().remove(1);
        let reasons = |text: &str, ts: &str, quote: &str| {
            check_item(
                raw("materials", text, "", "", ts, quote),
                &template,
                &index,
                &transcript,
            )
            .unwrap()
            .entry
            .reasons
        };
        // Said in words, written in digits: the same number.
        assert!(reasons(
            "천장 9평을 새로 한다",
            "00:00:21",
            "천장은 아홉 평 정도 새로 합니다"
        )
        .is_empty());
        assert_eq!(
            reasons(
                "시공비까지 80,900만원 더 든다",
                "00:00:26",
                "80,900만원 정도 더 올라갈 수"
            ),
            [CheckReason::UnitOdd]
        );
        assert_eq!(
            reasons(
                "복도 끝 수납장은 자당 20",
                "00:00:31",
                "복도 끝은 자당 20 기준이고"
            ),
            [CheckReason::NumbersDiffer]
        );
        let mut entries = vec![
            (
                "budget".to_string(),
                task(
                    "견적이 1억 5천에서 2억 1천으로 올랐다",
                    "",
                    "",
                    1_000,
                    vec![],
                ),
            ),
            (
                "budget".to_string(),
                task("5,100만 원 늘었다", "", "", 2_000, vec![]),
            ),
        ];
        mark_sums(&mut entries);
        assert_eq!(entries[0].1.reasons, [CheckReason::SumsDisagree]);
        assert_eq!(entries[1].1.reasons, [CheckReason::SumsDisagree]);
    }

    #[test]
    fn a_task_goes_to_the_person_who_promised_it_by_name() {
        let template = consult();
        let mut transcript = transcript();
        transcript
            .segments
            .push(line(21_000, 2, "제가 집에 가서 냉장고 치수를 재 볼게요."));
        transcript
            .segments
            .push(line(26_000, 1, "저희가 견적은 다음 주에 드리겠습니다."));
        let mut entries = vec![
            (
                "our_tasks".to_string(),
                task(
                    "신발장 도면 다시 그려 보내기",
                    "우리(상담 담당)",
                    "",
                    11_000,
                    vec![],
                ),
            ),
            (
                "customer_tasks".to_string(),
                task("냉장고 치수 재기", "고객", "", 21_000, vec![]),
            ),
            (
                "our_tasks".to_string(),
                task("견적 보내기", "우리(상담 담당)", "", 26_000, vec![]),
            ),
        ];
        // Line 11 s is the named designer's own "보내드리겠습니다" without
        // "제가": the side stays; "저희가" is the company, not a person.
        owners_by_name(&template, &mut entries, &transcript, "이서진");
        assert_eq!(entries[0].1.owner, "우리(상담 담당)");
        assert_eq!(entries[1].1.owner, "이서진");
        assert_eq!(entries[2].1.owner, "우리(상담 담당)");

        transcript.segments[2].text =
            "네, 중문은 제가 신발장 도면을 다시 그려서 보내드리겠습니다.".into();
        owners_by_name(&template, &mut entries, &transcript, "이서진");
        assert_eq!(entries[0].1.owner, "김서윤 실장");
    }
}
