//! A recording's transcript: who said what, and when in the original audio.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::vocab::Vocabulary;

/// One stretch of speech by one speaker. Times are milliseconds from the
/// start of the original recording, so any line can be played back from it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    /// The speaker's number, from 1; 0 when the speaker is not known.
    pub speaker: u32,
    pub text: String,
    /// How sure the speech model was of this line, 0 to 1, when it says.
    #[serde(default)]
    pub confidence: Option<f32>,
    /// The speech model's own measures say this line may be wrong: worth a
    /// listen before it is relied on.
    #[serde(default)]
    pub unsure: bool,
    /// Someone else talks over most of this line, so the words may be a mix
    /// of two people.
    #[serde(default)]
    pub overlap: bool,
    /// The user's words for this line, when they fixed it on the screen.
    /// The speech model's words stay in `text`: they are the record, and
    /// `전사.txt` is written from them. `Some("")` when the user took the
    /// line's words out (they went into the line before, or were wrong).
    /// Absent in meetings saved before lines could be fixed.
    #[serde(default)]
    pub edited: Option<String>,
    /// Words fixed one at a time. The user's vocabulary fixes words as the
    /// recording is transcribed: those are in `text` already (as `전사.txt`
    /// has always had them), and their fix only remembers what was heard.
    /// The user's own fixes (picking a misheard word) are not: `edited` is
    /// `text` with them put in. Empty when the line is as transcribed or
    /// when the user rewrote it whole (then `edited` alone says what it
    /// reads). Absent in meetings saved before words could be fixed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<WordFix>,
}

/// A word fixed in one line, beside the line's words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WordFix {
    /// Where the word starts in `Segment::text`, in characters: the heard
    /// words `from` for the user's fix, the right words `to` for the
    /// vocabulary's (which `text` has already).
    pub at: u32,
    /// The words heard there.
    pub from: String,
    /// What they read as.
    pub to: String,
    /// The user's vocabulary fixed it as the recording was transcribed, in
    /// `text` itself; transcribing again fixes it again. Otherwise the user
    /// fixed it, in `edited` only.
    #[serde(default)]
    pub auto: bool,
}

impl WordFix {
    /// How many characters of `Segment::text` it covers.
    fn written_len(&self) -> usize {
        if self.auto {
            self.to.chars().count()
        } else {
            self.from.chars().count()
        }
    }
}

impl Segment {
    /// The line as the user reads it: their words when they fixed it, the
    /// speech model's otherwise.
    pub fn said(&self) -> &str {
        self.edited.as_deref().unwrap_or(&self.text)
    }

    /// The user rewrote the line whole (#57), rather than word by word.
    fn rewritten(&self) -> bool {
        self.edited.is_some() && self.fixes.iter().all(|fix| fix.auto)
    }

    /// Makes `edited` the words as transcribed with the user's word fixes
    /// put in.
    fn refix(&mut self) {
        self.fixes.sort_by_key(|fix| fix.at);
        if let Some(words) = with_fixes(&self.text, &self.fixes) {
            self.edited = (words != self.text).then_some(words);
        }
    }

    /// Where `n` characters from `at` of the line as read are in the line
    /// as transcribed.
    fn place(&self, at: usize, n: usize) -> Place {
        let mut read = 0;
        let mut written = 0;
        for (index, fix) in self.fixes.iter().enumerate() {
            let same = (fix.at as usize).saturating_sub(written);
            if at >= read && at + n <= read + same {
                return Place::Written(written + at - read);
            }
            read += same;
            written = fix.at as usize;
            let fixed = fix.to.chars().count();
            if at == read && n == fixed {
                return Place::Fix(index);
            }
            if at < read + fixed && read < at + n {
                return Place::Across;
            }
            read += fixed;
            written += fix.written_len();
        }
        let same = self.text.chars().count().saturating_sub(written);
        if at >= read && at + n <= read + same {
            Place::Written(written + at - read)
        } else {
            Place::Across
        }
    }
}

/// Where words picked in a line as read are in the line as transcribed.
enum Place {
    /// Unchanged words, from this character of `Segment::text`.
    Written(usize),
    /// Exactly the words a fix put in: the fix's index.
    Fix(usize),
    /// Partly in a fix, or past the end.
    Across,
}

/// `text` with the user's fixes among `fixes` (in order) put in (the
/// vocabulary's are in `text` already); `None` when a fix does not fit the
/// words.
pub fn with_fixes(text: &str, fixes: &[WordFix]) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut next = 0;
    for fix in fixes {
        let at = fix.at as usize;
        let end = at + fix.written_len();
        let there = if fix.auto { &fix.to } else { &fix.from };
        if at < next || end > chars.len() || chars[at..end].iter().collect::<String>() != *there {
            return None;
        }
        out.extend(&chars[next..at]);
        out.push_str(&fix.to);
        next = end;
    }
    out.extend(&chars[next..]);
    Some(out)
}

/// The byte where character `at` of `text` starts (the end for one past it).
fn byte_at(text: &str, at: usize) -> usize {
    text.char_indices()
        .nth(at)
        .map_or(text.len(), |(byte, _)| byte)
}

/// What fixing a misheard word did: how many places, and the lines as they
/// were before, to put back if the user undoes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct WordFixed {
    pub places: u32,
    pub before: Vec<Segment>,
}

/// A speaker in one recording. The name starts as "화자 N" and is the
/// user's to change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Speaker {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct Transcript {
    /// The language the recording was transcribed in (`ko`).
    pub language: String,
    /// The model that transcribed it.
    pub model: String,
    pub segments: Vec<Segment>,
    pub speakers: Vec<Speaker>,
    /// Words the user's vocabulary corrected in the text.
    #[serde(default)]
    pub corrections: Vec<Corrected>,
}

/// A word the user's vocabulary corrected, and where: the minutes list these
/// so a reader can see what was changed after the speech model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Corrected {
    /// What the speech model wrote.
    pub from: String,
    /// What it was changed to.
    pub to: String,
    /// The start of every line it was changed in.
    pub at_ms: Vec<u64>,
}

/// Which lines a new name for a speaker is given to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum NameScope {
    /// The speaker throughout the meeting.
    All,
    /// The speaker's lines from a moment on: the speaker model had two
    /// people as one, or changed its mind part way.
    FromHere,
    /// The speaker's lines in one stretch (a paragraph on the screen).
    Here,
}

impl Transcript {
    /// Gives the name `name` to `speaker`'s lines within `scope`: from
    /// `from_ms` on, or from `from_ms` to before `to_ms` for one stretch.
    /// Lines that move to another name go to the speaker already called that,
    /// or to a new one. Named throughout with the person's name another
    /// speaker already has, the two are one person and become one speaker
    /// (never for a role such as "고객", which two people can share). Returns the times (start, end) the change covers,
    /// and the speaker's name before, so what refers to it can follow.
    pub fn name_speaker(
        &mut self,
        speaker: u32,
        name: &str,
        scope: NameScope,
        from_ms: u64,
        to_ms: u64,
    ) -> Option<(String, u64, u64)> {
        let name = name.trim().to_string();
        let old = self.speakers.iter().find(|s| s.id == speaker)?.name.clone();
        if name.is_empty() {
            return None;
        }
        if scope == NameScope::All {
            if let Some(entry) = self.speakers.iter_mut().find(|s| s.id == speaker) {
                entry.name = name;
            }
            self.join_named(speaker);
            return Some((old, 0, u64::MAX));
        }
        let (from, to) = match scope {
            NameScope::FromHere => (from_ms, u64::MAX),
            _ => (from_ms, to_ms.max(from_ms + 1)),
        };
        let target = match self
            .speakers
            .iter()
            .find(|s| s.name == name && s.id != speaker)
        {
            Some(existing) => existing.id,
            None => {
                let id = self.speakers.iter().map(|s| s.id).max().unwrap_or(0) + 1;
                self.speakers.push(Speaker {
                    id,
                    name: name.clone(),
                });
                id
            }
        };
        for segment in &mut self.segments {
            if segment.speaker == speaker && segment.start_ms >= from && segment.start_ms < to {
                segment.speaker = target;
            }
        }
        Some((old, from, to))
    }
}

/// Who said one line, by when it starts: part of a naming kept so it can
/// be put back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct LineSpeaker {
    pub start_ms: u64,
    pub speaker: u32,
}

impl Transcript {
    /// Puts back who said what as it was kept before a naming: the speakers
    /// and each line's speaker. Refused, changing nothing, when the lines are
    /// no longer the ones kept (the recording was transcribed again).
    pub fn put_back_speakers(
        &mut self,
        speakers: &[Speaker],
        lines: &[LineSpeaker],
    ) -> std::result::Result<(), &'static str> {
        if lines.len() != self.segments.len()
            || lines
                .iter()
                .zip(&self.segments)
                .any(|(kept, segment)| kept.start_ms != segment.start_ms)
        {
            return Err("the transcript has changed since");
        }
        if lines
            .iter()
            .any(|line| !speakers.iter().any(|s| s.id == line.speaker) && line.speaker != 0)
        {
            return Err("a line names a speaker that is not there");
        }
        self.speakers = speakers.to_vec();
        for (segment, kept) in self.segments.iter_mut().zip(lines) {
            segment.speaker = kept.speaker;
        }
        Ok(())
    }
}

impl Transcript {
    /// The transcript as the user reads it: each line in their words where
    /// they fixed it, a line they emptied left out. For the minutes and
    /// for searching; `전사.txt` is written from the transcript as it was
    /// transcribed.
    pub fn as_read(&self) -> Transcript {
        let mut read = self.clone();
        read.segments = self
            .segments
            .iter()
            .filter(|segment| {
                segment
                    .edited
                    .as_deref()
                    .is_none_or(|words| !words.trim().is_empty())
            })
            .map(|segment| Segment {
                text: segment.said().to_string(),
                edited: None,
                fixes: Vec::new(),
                ..segment.clone()
            })
            .collect();
        read
    }

    /// Whether the user fixed any line.
    pub fn edited(&self) -> bool {
        self.segments.iter().any(|segment| segment.edited.is_some())
    }

    /// Gives the lines from the one starting at `from_ms` up to `to_ms` (a
    /// paragraph on the screen) the user's words `text`. The words are
    /// spread back over the lines they came from, so a fixed word stays
    /// with its own line and its time; a line that ends up as transcribed
    /// is no longer marked as fixed. The transcribed words are never
    /// changed. Refused when no line starts at `from_ms`.
    pub fn edit_lines(
        &mut self,
        from_ms: u64,
        to_ms: u64,
        text: &str,
    ) -> std::result::Result<(), &'static str> {
        let first = self
            .segments
            .iter()
            .position(|segment| segment.start_ms == from_ms)
            .ok_or("no line starts there")?;
        let speaker = self.segments[first].speaker;
        let mut last = first;
        while self
            .segments
            .get(last + 1)
            .is_some_and(|next| next.speaker == speaker && next.end_ms <= to_ms)
        {
            last += 1;
        }
        let lines = &mut self.segments[first..=last];
        let now: Vec<&str> = lines.iter().map(|segment| segment.said()).collect();
        let parts = spread(&now, text.trim());
        for (segment, part) in lines.iter_mut().zip(parts) {
            if part == segment.said().trim() {
                // Unchanged: a line fixed word by word keeps its marks.
                continue;
            }
            segment.edited = (part != segment.text.trim()).then_some(part);
            segment.fixes.clear();
        }
        Ok(())
    }

    /// Reads the misheard words `from` as `to`: the ones the user picked,
    /// `n` characters from character `at` of the line starting at `at_ms`
    /// as it reads now, and every other place in the meeting `elsewhere`
    /// corrects (built with [`Vocabulary::one_correction`], so the same
    /// letters inside a longer word are left alone). The words as
    /// transcribed stay; a line rewritten whole gets the new words in its
    /// rewritten words. Refused, changing nothing, when the picked words are
    /// not there or are partly a word fixed before.
    pub fn fix_word(
        &mut self,
        at_ms: u64,
        at: usize,
        from: &str,
        to: &str,
        elsewhere: &Vocabulary,
    ) -> std::result::Result<WordFixed, &'static str> {
        let to = to.trim();
        if from.trim().is_empty() || to.is_empty() {
            return Err("a word is needed");
        }
        if from == to {
            return Err("the words are the same");
        }
        let picked = self
            .segments
            .iter()
            .position(|segment| segment.start_ms == at_ms)
            .ok_or("no line starts there")?;
        let n = from.chars().count();
        {
            let line = &self.segments[picked];
            let read: String = line.said().chars().skip(at).take(n).collect();
            if read != from {
                return Err("the words are not there any more");
            }
            if !line.rewritten() && matches!(line.place(at, n), Place::Across) {
                return Err("part of the words was fixed before");
            }
        }

        let mut fixed = WordFixed {
            places: 0,
            before: Vec::new(),
        };
        for (index, line) in self.segments.iter_mut().enumerate() {
            let here = (index == picked).then_some((at, n));
            let mut next = line.clone();
            let mut places = 0;
            if line.rewritten() {
                let words = line.said().to_string();
                let mine = here.map(|(at, n)| (byte_at(&words, at), byte_at(&words, at + n)));
                let mut ranges: Vec<(usize, usize)> = elsewhere
                    .spans(&words)
                    .into_iter()
                    .map(|span| (span.start, span.end))
                    .filter(|&(start, end)| mine.is_none_or(|(a, b)| end <= a || b <= start))
                    .chain(mine)
                    .collect();
                if ranges.is_empty() {
                    continue;
                }
                ranges.sort_unstable();
                let mut out = String::with_capacity(words.len());
                let mut from_byte = 0;
                for (start, end) in &ranges {
                    out.push_str(&words[from_byte..*start]);
                    out.push_str(to);
                    from_byte = *end;
                }
                out.push_str(&words[from_byte..]);
                places = ranges.len();
                next.edited = (out != line.text).then_some(out);
            } else {
                let mut taken: Vec<(usize, usize)> = line
                    .fixes
                    .iter()
                    .map(|fix| {
                        let at = fix.at as usize;
                        (at, at + fix.written_len())
                    })
                    .collect();
                if let Some((at, n)) = here {
                    match line.place(at, n) {
                        Place::Written(written) => {
                            next.fixes.push(WordFix {
                                at: written as u32,
                                from: from.to_string(),
                                to: to.to_string(),
                                auto: false,
                            });
                            taken.push((written, written + n));
                        }
                        Place::Fix(fix) if next.fixes[fix].auto => {
                            // The vocabulary's words, in the line as
                            // transcribed: the user's fix goes over them.
                            let words = next.fixes[fix].to.clone();
                            next.fixes[fix] = WordFix {
                                at: next.fixes[fix].at,
                                from: words,
                                to: to.to_string(),
                                auto: false,
                            };
                        }
                        Place::Fix(fix) => {
                            if next.fixes[fix].from == to {
                                next.fixes.remove(fix);
                            } else {
                                next.fixes[fix].to = to.to_string();
                            }
                        }
                        Place::Across => unreachable!("refused above"),
                    }
                    places += 1;
                }
                for span in elsewhere.spans(&line.text) {
                    let start = line.text[..span.start].chars().count();
                    let end = start + span.from.chars().count();
                    if taken.iter().any(|&(a, b)| start < b && a < end) {
                        continue;
                    }
                    taken.push((start, end));
                    next.fixes.push(WordFix {
                        at: start as u32,
                        from: span.from,
                        to: to.to_string(),
                        auto: false,
                    });
                    places += 1;
                }
                if places == 0 {
                    continue;
                }
                next.refix();
            }
            fixed.places += places as u32;
            fixed.before.push(std::mem::replace(line, next));
        }
        Ok(fixed)
    }

    /// How many places [`Transcript::fix_word`] would fix for the misheard
    /// words `from` picked at `at` of the line starting at `at_ms` and read
    /// as `to`, with the user's `vocabulary` deciding where else the same
    /// words are a word of their own (so is `to`: the same letters starting
    /// it are left alone). With `to` not given yet, the places for a right
    /// word that is no other word. Counted on a copy; nothing changes.
    pub fn word_places(
        &self,
        at_ms: u64,
        at: usize,
        from: &str,
        to: &str,
        vocabulary: &Vocabulary,
    ) -> std::result::Result<u32, &'static str> {
        let to = to.trim();
        let to = if to.is_empty() || to == from {
            format!("{from}\u{2060}")
        } else {
            to.to_string()
        };
        let elsewhere = Vocabulary::one_correction(from, &to, vocabulary);
        let mut copy = self.clone();
        copy.fix_word(at_ms, at, from, &to, &elsewhere)
            .map(|fixed| fixed.places)
    }

    /// Reads the words fixed from `from` to `to` in the lines from the one
    /// starting at `from_ms` up to `to_ms` (a paragraph on the screen) as
    /// heard again: the user's fix goes; the vocabulary's, which is in the
    /// words as transcribed and in `전사.txt`, becomes the user's fix back
    /// to the heard words. Returns how many places.
    pub fn unfix_word(&mut self, from_ms: u64, to_ms: u64, from: &str, to: &str) -> u32 {
        let mut undone = 0;
        for line in &mut self.segments {
            if line.start_ms < from_ms || line.start_ms > to_ms {
                continue;
            }
            let mut changed = false;
            line.fixes.retain_mut(|fix| {
                if fix.from != from || fix.to != to {
                    return true;
                }
                changed = true;
                undone += 1;
                if !fix.auto {
                    return false;
                }
                // The vocabulary's fix is in the words as transcribed:
                // the heard words come back as the user's fix over them.
                *fix = WordFix {
                    at: fix.at,
                    from: to.to_string(),
                    to: from.to_string(),
                    auto: false,
                };
                true
            });
            if changed {
                line.refix();
            }
        }
        undone
    }

    /// Puts lines back as they were before a fix (`WordFixed::before`).
    /// Refused, changing nothing, when one of them is no longer there (the
    /// recording was transcribed again).
    pub fn put_back_lines(&mut self, before: &[Segment]) -> std::result::Result<(), &'static str> {
        let mut at = Vec::with_capacity(before.len());
        for kept in before {
            let index = self
                .segments
                .iter()
                .position(|line| line.start_ms == kept.start_ms && line.text == kept.text)
                .ok_or("the transcript has changed since")?;
            at.push(index);
        }
        for (index, kept) in at.into_iter().zip(before) {
            let line = &mut self.segments[index];
            line.edited = kept.edited.clone();
            line.fixes = kept.fixes.clone();
        }
        Ok(())
    }
}

/// Splits `text`, the new words for `lines` read one after another with a
/// space between, back into one piece per line. What did not change at the
/// start and the end keeps its line; the changed stretch goes to the line
/// it starts in.
fn spread(lines: &[&str], text: &str) -> Vec<String> {
    let mut joined: Vec<char> = Vec::new();
    let mut spans = Vec::with_capacity(lines.len());
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            spans.push((joined.len(), joined.len()));
            continue;
        }
        if !joined.is_empty() {
            joined.push(' ');
        }
        let start = joined.len();
        joined.extend(line.chars());
        spans.push((start, joined.len()));
    }
    let new: Vec<char> = text.chars().collect();
    let most = joined.len().min(new.len());
    let same_start = joined.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let same_end = joined
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(most - same_start)
        .take_while(|(a, b)| a == b)
        .count();
    let mut old_end = joined.len() - same_end;
    let mut new_end = new.len() - same_end;
    // A change across lines takes the rest of the last line it reaches,
    // so a full stop the old and new words happen to end with is not left
    // alone on that line.
    if spans
        .iter()
        .any(|&(start, end)| end > start && start > same_start && start < old_end)
    {
        let reach = spans
            .iter()
            .filter(|&&(start, end)| end > start && start < old_end)
            .map(|&(_, end)| end)
            .max()
            .unwrap_or(old_end);
        if reach > old_end {
            new_end += reach - old_end;
            old_end = reach;
        }
    }
    // Where a line's start and end land in the new words. A line the change
    // starts at the start of takes what was put in there; a line the change
    // ends at the end of takes what was added after it.
    let start_at = |at: usize| {
        if at <= same_start {
            at
        } else if at >= old_end {
            at - old_end + new_end
        } else {
            new_end
        }
    };
    let end_at = |at: usize| {
        if at >= old_end {
            at - old_end + new_end
        } else if at <= same_start {
            at
        } else {
            new_end
        }
    };
    let parts: Vec<String> = spans
        .into_iter()
        .map(|(start, end)| {
            if start == end {
                // A line already emptied stays empty.
                return String::new();
            }
            let (start, end) = (start_at(start), end_at(end).max(start_at(start)));
            new[start..end]
                .iter()
                .collect::<String>()
                .trim()
                .to_string()
        })
        .collect();
    // Never a word lost or doubled: when the pieces do not read as the new
    // words (a change that began on the space between two lines), the
    // whole of them goes to the first line.
    let letters = |text: &str| {
        text.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
    };
    if letters(&parts.concat()) == letters(text) {
        return parts;
    }
    let mut whole = vec![String::new(); parts.len()];
    if let Some(first) = whole.first_mut() {
        *first = text.trim().to_string();
    }
    whole
}

/// Names that two people can share: a role, or no name at all.
fn shared_name(name: &str) -> bool {
    const ROLES: [&str; 12] = [
        "고객",
        "손님",
        "남편",
        "아내",
        "부인",
        "사모님",
        "사장님",
        "직원",
        "담당",
        "상담",
        "우리",
        "저희",
    ];
    name.trim().is_empty()
        || name.starts_with("화자")
        || ROLES.iter().any(|role| name.contains(role))
}

impl Transcript {
    /// Puts `speaker`'s lines with the other speaker who has its name, when
    /// that name is a person's: the user named them the same, so they are
    /// one person the speaker model heard as two. Only the speaker just
    /// named moves; nothing is merged on the machine's word. Returns the
    /// speaker the lines went to.
    fn join_named(&mut self, speaker: u32) -> Option<u32> {
        let name = self.speakers.iter().find(|s| s.id == speaker)?.name.clone();
        if shared_name(&name) {
            return None;
        }
        let into = self
            .speakers
            .iter()
            .find(|s| s.id != speaker && s.name == name)?
            .id;
        for segment in &mut self.segments {
            if segment.speaker == speaker {
                segment.speaker = into;
            }
        }
        self.speakers.retain(|s| s.id != speaker);
        Some(into)
    }
}

/// Lines by the same speaker closer than this read as one paragraph.
const PARAGRAPH_GAP_MS: u64 = 2_000;
/// A paragraph is cut once it is this long, so the timestamps stay useful.
const PARAGRAPH_MAX_CHARS: usize = 400;

impl Transcript {
    /// The default name of speaker `id`.
    pub fn default_speaker_name(id: u32) -> String {
        if id == 0 {
            "화자".to_string()
        } else {
            format!("화자 {id}")
        }
    }

    pub fn speaker_name(&self, id: u32) -> String {
        self.speakers
            .iter()
            .find(|speaker| speaker.id == id)
            .map(|speaker| speaker.name.clone())
            .unwrap_or_else(|| Self::default_speaker_name(id))
    }

    /// Whether more than one speaker was told apart.
    pub fn has_speakers(&self) -> bool {
        self.speakers.len() > 1
    }

    pub fn duration_ms(&self) -> u64 {
        self.segments.last().map_or(0, |segment| segment.end_ms)
    }

    /// The segments grouped for reading: consecutive lines by one speaker
    /// become one paragraph, timed from its first line.
    pub fn paragraphs(&self) -> Vec<Segment> {
        let mut paragraphs: Vec<Segment> = Vec::new();
        for segment in &self.segments {
            if let Some(last) = paragraphs.last_mut() {
                let joins = last.speaker == segment.speaker
                    && segment.start_ms.saturating_sub(last.end_ms) <= PARAGRAPH_GAP_MS
                    && last.text.chars().count() + segment.text.chars().count()
                        <= PARAGRAPH_MAX_CHARS;
                if joins {
                    last.text.push(' ');
                    last.text.push_str(&segment.text);
                    last.end_ms = segment.end_ms;
                    // A paragraph is as sure as its least sure line.
                    last.confidence = match (last.confidence, segment.confidence) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    };
                    last.overlap |= segment.overlap;
                    last.unsure |= segment.unsure;
                    continue;
                }
            }
            paragraphs.push(segment.clone());
        }
        paragraphs
    }

    /// Plain text: one paragraph per line, with its time and speaker.
    pub fn to_text(&self) -> String {
        self.to_text_with_gaps(&[])
    }

    /// [`Transcript::to_text`], with a line `[time] (12초 끊김)` where the
    /// microphone was away while recording, so a reader knows the talk
    /// around it is not continuous.
    pub fn to_text_with_gaps(&self, gaps: &[super::store::Gap]) -> String {
        let mut out = String::new();
        let mut gaps: Vec<&super::store::Gap> = gaps.iter().collect();
        gaps.sort_by_key(|gap| gap.at_ms);
        let mut gaps = gaps.into_iter().peekable();
        let gap_line = |out: &mut String, gap: &super::store::Gap| {
            out.push_str(&format!("[{}] {}\n", clock(gap.at_ms), gap.note()));
        };
        for paragraph in self.paragraphs() {
            while let Some(gap) = gaps.next_if(|gap| gap.at_ms <= paragraph.start_ms) {
                gap_line(&mut out, gap);
            }
            out.push_str(&format!("[{}] ", clock(paragraph.start_ms)));
            if self.has_speakers() {
                out.push_str(&format!("{}: ", self.speaker_name(paragraph.speaker)));
            }
            out.push_str(&paragraph.text);
            out.push('\n');
        }
        for gap in gaps {
            gap_line(&mut out, gap);
        }
        out
    }

    /// Markdown: the speaker and time in bold, then the paragraph.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        for paragraph in self.paragraphs() {
            if self.has_speakers() {
                out.push_str(&format!(
                    "**{}** `{}`\n\n",
                    self.speaker_name(paragraph.speaker),
                    clock(paragraph.start_ms)
                ));
            } else {
                out.push_str(&format!("`{}`\n\n", clock(paragraph.start_ms)));
            }
            out.push_str(&paragraph.text);
            out.push_str("\n\n");
        }
        out
    }

    /// SubRip subtitles, one cue per segment.
    pub fn to_srt(&self) -> String {
        let mut out = String::new();
        for (index, segment) in self.segments.iter().enumerate() {
            out.push_str(&format!(
                "{}\n{} --> {}\n",
                index + 1,
                srt_time(segment.start_ms),
                srt_time(segment.end_ms.max(segment.start_ms + 1))
            ));
            if self.has_speakers() {
                out.push_str(&format!("{}: ", self.speaker_name(segment.speaker)));
            }
            out.push_str(&segment.text);
            out.push_str("\n\n");
        }
        out
    }

    /// The transcript as the summary model reads it: every segment on its own
    /// line with its time and speaker, so the model can cite the time.
    pub fn to_prompt_text(&self) -> String {
        let mut out = String::new();
        for paragraph in self.paragraphs() {
            out.push_str(&format!(
                "[{}] {}: {}\n",
                clock(paragraph.start_ms),
                self.speaker_name(paragraph.speaker),
                paragraph.text
            ));
        }
        out
    }
}

/// `hh:mm:ss` from milliseconds; the hour is always there so a time means
/// the same in a five-minute memo and a three-hour meeting.
pub fn clock(ms: u64) -> String {
    let total_secs = ms / 1000;
    format!(
        "{:02}:{:02}:{:02}",
        total_secs / 3600,
        total_secs / 60 % 60,
        total_secs % 60
    )
}

/// Milliseconds from `hh:mm:ss`, `mm:ss` or `h:mm:ss`; `None` if it isn't one.
pub fn parse_clock(text: &str) -> Option<u64> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let mut total: u64 = 0;
    for part in &parts {
        if part.is_empty() || part.len() > 3 || !part.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        total = total * 60 + part.parse::<u64>().ok()?;
    }
    Some(total * 1000)
}

fn srt_time(ms: u64) -> String {
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// Renumbers speakers in order of first appearance and names them, so the
/// first voice heard is always "화자 1".
pub fn number_speakers(segments: &mut [Segment]) -> Vec<Speaker> {
    let mut order: Vec<u32> = Vec::new();
    for segment in segments.iter_mut() {
        if segment.speaker == 0 {
            continue;
        }
        let position = match order.iter().position(|&id| id == segment.speaker) {
            Some(position) => position,
            None => {
                order.push(segment.speaker);
                order.len() - 1
            }
        };
        segment.speaker = position as u32 + 1;
    }
    (1..=order.len() as u32)
        .map(|id| Speaker {
            id,
            name: Transcript::default_speaker_name(id),
        })
        .collect()
}

/// Things a speech model writes when it hears no words: sign-offs it learned
/// from subtitles. Nobody says them in a meeting, so a segment that is only
/// one of these is dropped.
const HALLUCINATED_LINES: &[&str] = &[
    "시청해주셔서 감사합니다",
    "시청해 주셔서 감사합니다",
    "끝까지 시청해주셔서 감사합니다",
    "구독과 좋아요 부탁드립니다",
    "구독 좋아요 알림설정",
    "다음 영상에서 만나요",
    "다음 시간에 만나요",
    "자막 제공 및 광고를 포함하고 있습니다",
    "한글자막 by",
    "자막 by",
    "MBC 뉴스",
    "KBS 뉴스",
    "SBS 뉴스",
    "이 영상은 유료광고를 포함하고 있습니다",
    "Thank you for watching",
    "Thanks for watching",
    "Thank you",
];

/// Whether `text` is nothing but a line a model makes up over silence.
pub fn is_hallucinated_line(text: &str) -> bool {
    let bare: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '.' | '!' | '?' | ',' | '~' | '♪'))
        .collect::<String>()
        .to_lowercase();
    if bare.is_empty() {
        return true;
    }
    HALLUCINATED_LINES.iter().any(|line| {
        let line: String = line
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_lowercase();
        // The whole segment, give or take a reporter's name after a news tag.
        bare == line
            || (bare.starts_with(&line) && bare.chars().count() <= line.chars().count() + 6)
    })
}

/// Collapses a phrase the model got stuck repeating ("네 알겠습니다 네
/// 알겠습니다 네 알겠습니다 ...") to one copy. A phrase said twice is left
/// alone: people do repeat themselves.
pub fn collapse_loops(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut kept: Vec<&str> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        let mut collapsed = false;
        // Longest phrase first, so a long loop is not broken into short ones.
        for len in (1..=8usize).rev() {
            if i + len * 3 > words.len() {
                continue;
            }
            let phrase = &words[i..i + len];
            let mut repeats = 1;
            while i + (repeats + 1) * len <= words.len()
                && &words[i + repeats * len..i + (repeats + 1) * len] == phrase
            {
                repeats += 1;
            }
            if repeats >= 3 {
                kept.extend_from_slice(phrase);
                i += repeats * len;
                collapsed = true;
                break;
            }
        }
        if !collapsed {
            kept.push(words[i]);
            i += 1;
        }
    }
    kept.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spoken(lines: &[(u64, u32)]) -> Transcript {
        Transcript {
            segments: lines
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
        }
    }

    #[test]
    fn a_speaker_is_named_throughout_from_a_moment_or_in_one_stretch() {
        let lines = [(0, 1), (1_000, 2), (2_000, 1), (3_000, 1), (4_000, 2)];
        let speakers_of = |t: &Transcript| t.segments.iter().map(|s| s.speaker).collect::<Vec<_>>();

        let mut all = spoken(&lines);
        assert_eq!(
            all.name_speaker(1, "김서윤 실장", NameScope::All, 0, 0),
            Some(("화자 1".into(), 0, u64::MAX))
        );
        assert_eq!(all.speakers[0].name, "김서윤 실장");
        assert_eq!(speakers_of(&all), [1, 2, 1, 1, 2]);

        // From 2 s on, speaker 1 was someone else: a new speaker.
        let mut from = spoken(&lines);
        from.name_speaker(1, "박도현 소장", NameScope::FromHere, 2_000, 0);
        assert_eq!(speakers_of(&from), [1, 2, 3, 3, 2]);
        assert_eq!(from.speakers[2].name, "박도현 소장");

        // One stretch goes to a speaker who already has that name.
        let mut here = spoken(&lines);
        here.name_speaker(2, "고객", NameScope::All, 0, 0);
        here.name_speaker(1, "고객", NameScope::Here, 2_000, 2_900);
        assert_eq!(speakers_of(&here), [1, 2, 2, 1, 2]);
        assert_eq!(here.speakers.len(), 2);

        assert_eq!(here.name_speaker(9, "누구", NameScope::All, 0, 0), None);
        assert_eq!(here.name_speaker(1, "  ", NameScope::All, 0, 0), None);
    }

    #[test]
    fn a_naming_is_put_back_as_it_was() {
        let mut heard = spoken(&[(0, 1), (1_000, 2), (2_000, 1)]);
        let before = heard.clone();
        let kept: Vec<LineSpeaker> = heard
            .segments
            .iter()
            .map(|s| LineSpeaker {
                start_ms: s.start_ms,
                speaker: s.speaker,
            })
            .collect();
        heard.name_speaker(1, "김서윤 실장", NameScope::Here, 2_000, 2_900);
        heard.name_speaker(1, "박도현 소장", NameScope::All, 0, 0);
        assert_ne!(heard, before);
        heard
            .put_back_speakers(&before.speakers, &kept)
            .expect("the same lines");
        assert_eq!(heard, before);
        // Lines that are not the kept ones: nothing is changed.
        let mut other = spoken(&[(0, 1), (1_500, 2)]);
        let untouched = other.clone();
        assert!(other.put_back_speakers(&before.speakers, &kept).is_err());
        assert_eq!(other, untouched);
        // A line given a speaker the list does not have: refused.
        let mut lost = before.clone();
        let wrong = [
            LineSpeaker {
                start_ms: 0,
                speaker: 9,
            },
            kept[1],
            kept[2],
        ];
        assert!(lost.put_back_speakers(&before.speakers, &wrong).is_err());
    }

    #[test]
    fn one_person_heard_as_two_speakers_becomes_one_by_name() {
        let lines = [(0, 1), (1_000, 2), (2_000, 3), (3_000, 1)];
        let speakers_of = |t: &Transcript| t.segments.iter().map(|s| s.speaker).collect::<Vec<_>>();
        let mut heard = spoken(&lines);
        heard.speakers.push(Speaker {
            id: 3,
            name: "화자 3".into(),
        });
        heard.name_speaker(1, "김서윤 실장", NameScope::All, 0, 0);
        // Naming the third the same: it was the same person all along.
        heard.name_speaker(3, "김서윤 실장", NameScope::All, 0, 0);
        assert_eq!(speakers_of(&heard), [1, 2, 1, 1]);
        assert_eq!(heard.speakers.len(), 2);

        // Two given the same role stay two people.
        let mut couple = spoken(&lines);
        couple.name_speaker(1, "고객", NameScope::All, 0, 0);
        couple.name_speaker(2, "고객", NameScope::All, 0, 0);
        assert_eq!(speakers_of(&couple), [1, 2, 3, 1]);
        assert_eq!(couple.speakers.len(), 2);
    }

    fn segment(start_ms: u64, end_ms: u64, speaker: u32, text: &str) -> Segment {
        Segment {
            start_ms,
            end_ms,
            speaker,
            text: text.to_string(),
            confidence: None,
            unsure: false,
            overlap: false,
            edited: None,
            fixes: Vec::new(),
        }
    }

    #[test]
    fn clock_always_shows_the_hour() {
        assert_eq!(clock(0), "00:00:00");
        assert_eq!(clock(754_000), "00:12:34");
        assert_eq!(clock(3_725_999), "01:02:05");
    }

    #[test]
    fn parse_clock_reads_both_forms() {
        assert_eq!(parse_clock("12:34"), Some(754_000));
        assert_eq!(parse_clock("01:02:05"), Some(3_725_000));
        assert_eq!(parse_clock(" 0:07 "), Some(7_000));
        assert_eq!(parse_clock("12"), None);
        assert_eq!(parse_clock("ab:cd"), None);
        assert_eq!(parse_clock("1:2:3:4"), None);
    }

    #[test]
    fn speakers_are_numbered_by_first_appearance() {
        let mut segments = vec![
            segment(0, 1000, 3, "먼저"),
            segment(1000, 2000, 1, "다음"),
            segment(2000, 3000, 3, "다시"),
            segment(3000, 4000, 0, "모름"),
        ];
        let speakers = number_speakers(&mut segments);
        assert_eq!(
            segments.iter().map(|s| s.speaker).collect::<Vec<_>>(),
            vec![1, 2, 1, 0]
        );
        assert_eq!(speakers.len(), 2);
        assert_eq!(speakers[1].name, "화자 2");
    }

    #[test]
    fn paragraphs_join_one_speakers_close_lines() {
        let transcript = Transcript {
            segments: vec![
                segment(0, 1000, 1, "안녕하세요."),
                segment(1200, 2000, 1, "시작하겠습니다."),
                segment(2100, 3000, 2, "네."),
                segment(9000, 9500, 2, "한참 뒤에."),
            ],
            speakers: vec![
                Speaker {
                    id: 1,
                    name: "화자 1".into(),
                },
                Speaker {
                    id: 2,
                    name: "김 실장".into(),
                },
            ],
            ..Default::default()
        };
        let paragraphs = transcript.paragraphs();
        assert_eq!(paragraphs.len(), 3);
        assert_eq!(paragraphs[0].text, "안녕하세요. 시작하겠습니다.");
        assert_eq!(paragraphs[0].end_ms, 2000);
        assert!(transcript.to_text().contains("[00:00:02] 김 실장: 네."));
    }

    #[test]
    fn a_single_speaker_transcript_has_no_speaker_labels() {
        let transcript = Transcript {
            segments: vec![segment(0, 1000, 1, "혼자 말합니다.")],
            speakers: vec![Speaker {
                id: 1,
                name: "화자 1".into(),
            }],
            ..Default::default()
        };
        assert_eq!(transcript.to_text(), "[00:00:00] 혼자 말합니다.\n");
        assert!(transcript
            .to_srt()
            .contains("00:00:00,000 --> 00:00:01,000\n혼자 말합니다."));
    }

    #[test]
    fn where_the_microphone_was_away_is_a_line_of_the_transcript() {
        use super::super::store::Gap;
        let transcript = Transcript {
            segments: vec![
                segment(0, 1000, 1, "앞의 이야기."),
                segment(5000, 6000, 1, "뒤의 이야기."),
            ],
            speakers: vec![Speaker {
                id: 1,
                name: "화자 1".into(),
            }],
            ..Default::default()
        };
        let gaps = [
            Gap {
                at_ms: 9_000,
                gap_ms: 30_000,
            },
            Gap {
                at_ms: 2_000,
                gap_ms: 12_000,
            },
        ];
        assert_eq!(
            transcript.to_text_with_gaps(&gaps),
            "[00:00:00] 앞의 이야기.\n[00:00:02] (12초 끊김)\n[00:00:05] 뒤의 이야기.\n[00:00:09] (30초 끊김)\n"
        );
    }

    #[test]
    fn subtitle_sign_offs_are_hallucinations_but_speech_is_not() {
        assert!(is_hallucinated_line("시청해주셔서 감사합니다."));
        assert!(is_hallucinated_line(" MBC 뉴스 김철수입니다 "));
        assert!(is_hallucinated_line("..."));
        assert!(is_hallucinated_line("Thank you."));
        assert!(!is_hallucinated_line("감사합니다."));
        assert!(!is_hallucinated_line("뉴스에서 봤는데 타일 값이 올랐대요."));
        assert!(!is_hallucinated_line(
            "MBC 뉴스 보셨어요? 어제 아파트 리모델링 얘기가 나왔더라고요."
        ));
    }

    #[test]
    fn loops_collapse_but_a_repeat_stays() {
        assert_eq!(
            collapse_loops("네 알겠습니다 네 알겠습니다 네 알겠습니다 네 알겠습니다"),
            "네 알겠습니다"
        );
        assert_eq!(collapse_loops("그 그 그 그 타일은"), "그 타일은");
        assert_eq!(collapse_loops("네 네 알겠습니다"), "네 네 알겠습니다");
        assert_eq!(
            collapse_loops("거실은 마루 방은 마루 주방은 타일"),
            "거실은 마루 방은 마루 주방은 타일"
        );
    }

    fn lines(texts: &[&str]) -> Transcript {
        Transcript {
            segments: texts
                .iter()
                .enumerate()
                .map(|(i, text)| segment(i as u64 * 1000, i as u64 * 1000 + 900, 1, text))
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_fixed_word_stays_with_its_own_line_and_the_transcribed_words_stay() {
        let mut transcript = lines(&[
            "네 안녕하세요.",
            "우물천장탄을 내리면 답답하지 않습니다.",
            "좋아요.",
        ]);
        transcript
            .edit_lines(
                0,
                2900,
                "네 안녕하세요. 우물천장을 내리면 답답하지 않습니다. 좋아요.",
            )
            .unwrap();
        let s = &transcript.segments;
        assert_eq!(s[0].edited, None);
        assert_eq!(
            s[1].edited.as_deref(),
            Some("우물천장을 내리면 답답하지 않습니다.")
        );
        assert_eq!(s[1].text, "우물천장탄을 내리면 답답하지 않습니다.");
        assert_eq!(s[2].edited, None);
        assert!(transcript.edited());
        // The plain transcript is the one transcribed; reading gives the fix.
        assert!(transcript.to_text().contains("우물천장탄을"));
        assert_eq!(
            transcript.as_read().segments[1].text,
            "우물천장을 내리면 답답하지 않습니다."
        );
    }

    #[test]
    fn words_put_back_as_they_were_are_no_longer_a_fix() {
        let mut transcript = lines(&["하나 둘.", "셋 넷."]);
        transcript.edit_lines(0, 1900, "하나 둘. 셋 다섯.").unwrap();
        assert_eq!(transcript.segments[1].edited.as_deref(), Some("셋 다섯."));
        transcript.edit_lines(0, 1900, "하나 둘. 셋 넷.").unwrap();
        assert!(!transcript.edited());
    }

    #[test]
    fn a_paragraph_written_anew_goes_to_its_first_line() {
        let mut transcript = lines(&["가나다.", "라마바.", "사아자."]);
        transcript.edit_lines(0, 2900, "전혀 다른 말.").unwrap();
        let said: Vec<&str> = transcript.segments.iter().map(|s| s.said()).collect();
        assert_eq!(said, vec!["전혀 다른 말.", "", ""]);
        // A line emptied is left out of what is read.
        assert_eq!(transcript.as_read().segments.len(), 1);
        // Only the paragraph asked for: a line after `to_ms` is untouched.
        let mut transcript = lines(&["가나다.", "라마바.", "사아자."]);
        transcript.edit_lines(0, 1900, "가나다. 라마.").unwrap();
        assert_eq!(transcript.segments[1].said(), "라마.");
        assert_eq!(transcript.segments[2].edited, None);
        assert!(transcript.edit_lines(500, 900, "없는 줄").is_err());
    }

    #[test]
    fn words_added_at_a_line_s_end_stay_with_it() {
        let mut transcript = lines(&["가나다.", "라마바."]);
        transcript
            .edit_lines(0, 1900, "가나다 하고. 라마바.")
            .unwrap();
        assert_eq!(transcript.segments[0].said(), "가나다 하고.");
        assert_eq!(transcript.segments[1].edited, None);
    }

    #[test]
    fn words_added_at_either_end_of_a_paragraph_are_kept() {
        assert_eq!(spread(&["라라"], "라라라"), vec!["라라라"]);
        assert_eq!(
            spread(
                &["답답하지 않습니다.", "좋아요."],
                "답답하지 않습니다. 좋아요. 그리고 또."
            ),
            vec!["답답하지 않습니다.", "좋아요. 그리고 또."]
        );
        assert_eq!(
            spread(
                &["답답하지 않습니다.", "좋아요."],
                "음 답답하지 않습니다. 좋아요."
            ),
            vec!["음 답답하지 않습니다.", "좋아요."]
        );
        // Put in where one line ends and the next starts: with the first.
        assert_eq!(spread(&["가", "나"], "가 X나"), vec!["가", "X나"]);
    }

    #[test]
    fn a_paragraph_replaced_or_emptied_loses_nothing_it_was_given() {
        assert_eq!(
            spread(&["가나다.", "라마바."], "전혀 다른 말"),
            vec!["전혀 다른 말", ""]
        );
        assert_eq!(spread(&["가나다.", "라마바."], ""), vec!["", ""]);
        // A change that starts on the space between two lines.
        let parts = spread(&["나나라", "다. 라", "."], "나나라다. 라라 .");
        assert_eq!(parts.concat().replace(' ', ""), "나나라다.라라.");
        // New words for a paragraph whose lines were all taken out.
        assert_eq!(spread(&["", ""], "다시 적은 말"), vec!["다시 적은 말", ""]);
    }

    #[test]
    fn no_edit_loses_or_doubles_a_word() {
        // Many edits of made-up lines, the same every run.
        let mut seed: u64 = 0x5eed_0057;
        let mut next = |below: usize| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((seed >> 33) as usize) % below.max(1)
        };
        let alphabet: Vec<char> = "가나다라마바 .".chars().collect();
        for _ in 0..5_000 {
            let lines: Vec<String> = (0..1 + next(4))
                .map(|_| {
                    (0..next(7))
                        .map(|_| alphabet[next(alphabet.len())])
                        .collect()
                })
                .collect();
            let now: Vec<&str> = lines.iter().map(String::as_str).collect();
            let mut text: Vec<char> = now
                .iter()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .collect();
            for _ in 0..1 + next(3) {
                let at = next(text.len() + 1);
                match next(3) {
                    0 => text.insert(at, alphabet[next(alphabet.len())]),
                    1 if at < text.len() => {
                        text.remove(at);
                    }
                    _ if at < text.len() => text[at] = alphabet[next(alphabet.len())],
                    _ => {}
                }
            }
            let text: String = text.into_iter().collect();
            let parts = spread(&now, &text);
            assert_eq!(parts.len(), now.len());
            let letters = |t: &str| t.chars().filter(|c| !c.is_whitespace()).collect::<String>();
            assert_eq!(
                letters(&parts.concat()),
                letters(&text),
                "{now:?} -> {text:?} gave {parts:?}"
            );
        }
    }

    fn vocabulary(text: &str) -> Vocabulary {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("v.txt");
        std::fs::write(&file, text).unwrap();
        Vocabulary::load(&file).unwrap()
    }

    #[test]
    fn a_misheard_word_is_fixed_where_picked_and_wherever_it_safely_is() {
        let mut transcript = lines(&[
            "우물천장탄을 살리고 간접등은",
            "그 우물천장탄은 너무 낮아요. 우물천장탄",
            "큰우물천장탄 이야기",
        ]);
        let known = vocabulary("우물천장\n");
        assert_eq!(
            transcript.word_places(1000, 2, "우물천장탄", "우물천장", &known),
            Ok(3)
        );
        let elsewhere = Vocabulary::one_correction("우물천장탄", "우물천장", &known);
        let fixed = transcript
            .fix_word(1000, 2, "우물천장탄", "우물천장", &elsewhere)
            .unwrap();
        // The picked place, the other two at a word's start; not inside
        // "큰우물천장탄".
        assert_eq!(fixed.places, 3);
        assert_eq!(fixed.before.len(), 2);
        let s = &transcript.segments;
        assert_eq!(s[0].said(), "우물천장을 살리고 간접등은");
        assert_eq!(s[1].said(), "그 우물천장은 너무 낮아요. 우물천장");
        assert_eq!(s[2].edited, None);
        // The words as transcribed stay, and so does the plain transcript.
        assert_eq!(s[1].text, "그 우물천장탄은 너무 낮아요. 우물천장탄");
        assert!(transcript.to_text().contains("우물천장탄은"));
        assert_eq!(
            s[1].fixes,
            vec![
                WordFix {
                    at: 2,
                    from: "우물천장탄".into(),
                    to: "우물천장".into(),
                    auto: false
                },
                WordFix {
                    at: 17,
                    from: "우물천장탄".into(),
                    to: "우물천장".into(),
                    auto: false
                },
            ]
        );
        // Undone in one paragraph, and back as it was everywhere.
        assert_eq!(transcript.unfix_word(0, 900, "우물천장탄", "우물천장"), 1);
        assert_eq!(transcript.segments[0].edited, None);
        transcript.put_back_lines(&fixed.before).unwrap();
        assert!(!transcript.edited());
        assert!(transcript.segments.iter().all(|s| s.fixes.is_empty()));
    }

    #[test]
    fn a_short_or_mid_word_pick_is_fixed_only_where_picked() {
        let mut transcript = lines(&["가나 가나다 가나", "가나는 좋아요"]);
        let none = Vocabulary::default();
        // Picked in the middle of a word: only there.
        let elsewhere = Vocabulary::one_correction("나다", "라마", &none);
        let fixed = transcript
            .fix_word(0, 4, "나다", "라마", &elsewhere)
            .unwrap();
        assert_eq!(fixed.places, 1);
        assert_eq!(transcript.segments[0].said(), "가나 가라마 가나");
        // Two letters go on only alone or before particles.
        let elsewhere = Vocabulary::one_correction("가나", "다라", &none);
        let fixed = transcript
            .fix_word(0, 0, "가나", "다라", &elsewhere)
            .unwrap();
        assert_eq!(fixed.places, 3);
        assert_eq!(transcript.segments[0].said(), "다라 가라마 다라");
        assert_eq!(transcript.segments[1].said(), "다라는 좋아요");
        // One letter: only where picked, and a word the user knows is
        // never another place.
        let elsewhere = Vocabulary::one_correction("좋", "싫", &none);
        assert!(elsewhere.corrections.is_empty());
    }

    #[test]
    fn a_fixed_word_can_be_fixed_again_but_not_half_of_it() {
        let mut transcript = lines(&["걸레바지는 흰색"]);
        let none = Vocabulary::default();
        let one = |from: &str, to: &str| Vocabulary::one_correction(from, to, &none);
        transcript
            .fix_word(0, 0, "걸레바지", "걸래받이", &one("걸레바지", "걸래받이"))
            .unwrap();
        assert_eq!(transcript.segments[0].said(), "걸래받이는 흰색");
        // The fix's own words, picked again, change the fix.
        transcript
            .fix_word(0, 0, "걸래받이", "걸레받이", &one("걸래받이", "걸레받이"))
            .unwrap();
        assert_eq!(transcript.segments[0].said(), "걸레받이는 흰색");
        assert_eq!(transcript.segments[0].fixes.len(), 1);
        assert_eq!(transcript.segments[0].fixes[0].from, "걸레바지");
        // Half a fix is refused, changing nothing.
        assert_eq!(
            transcript.fix_word(0, 2, "받이는", "바지는", &one("받이는", "바지는")),
            Err("part of the words was fixed before")
        );
        // Words that are not there any more are refused.
        assert!(transcript.fix_word(0, 0, "흰색", "검정", &none).is_err());
        // Fixed back to the words as transcribed, it is no fix.
        transcript
            .fix_word(0, 0, "걸레받이", "걸레바지", &one("걸레받이", "걸레바지"))
            .unwrap();
        assert!(!transcript.edited());
    }

    #[test]
    fn a_line_rewritten_whole_takes_the_word_in_its_own_words() {
        let mut transcript = lines(&["우물천장탄 하나", "우물천장탄 둘"]);
        transcript.edit_lines(0, 900, "우물천장탄 하나 더").unwrap();
        let none = Vocabulary::default();
        let elsewhere = Vocabulary::one_correction("우물천장탄", "우물천장", &none);
        let fixed = transcript
            .fix_word(1000, 0, "우물천장탄", "우물천장", &elsewhere)
            .unwrap();
        assert_eq!(fixed.places, 2);
        let s = &transcript.segments;
        assert_eq!(s[0].said(), "우물천장 하나 더");
        assert!(s[0].fixes.is_empty());
        assert_eq!(s[1].said(), "우물천장 둘");
        assert_eq!(s[1].fixes.len(), 1);
        // A whole-line edit over word fixes takes their place.
        transcript.edit_lines(1000, 1900, "다른 말").unwrap();
        assert!(transcript.segments[1].fixes.is_empty());
        assert_eq!(transcript.segments[1].said(), "다른 말");
    }

    #[test]
    fn word_fixes_read_in_older_records_and_are_left_out_when_absent() {
        let line: Segment = serde_json::from_str(
            r#"{"start_ms":0,"end_ms":1,"speaker":1,"text":"말","edited":"글"}"#,
        )
        .unwrap();
        assert!(line.fixes.is_empty());
        let json = serde_json::to_string(&line).unwrap();
        assert!(!json.contains("fixes"));
        assert_eq!(with_fixes("가나다", &[]).as_deref(), Some("가나다"));
        let wrong = WordFix {
            at: 1,
            from: "다".into(),
            to: "라".into(),
            auto: true,
        };
        assert_eq!(with_fixes("가나다", &[wrong]), None);
    }

    #[test]
    fn the_count_shown_is_what_the_fix_changes() {
        let mut transcript = lines(&["우물천 하나", "우물천장은 높아요", "우물천에서"]);
        let none = Vocabulary::default();
        // The right word starts with the misheard one: where it is already
        // written right, nothing is counted or changed.
        let counted = transcript
            .word_places(0, 0, "우물천", "우물천장", &none)
            .unwrap();
        let elsewhere = Vocabulary::one_correction("우물천", "우물천장", &none);
        let fixed = transcript
            .fix_word(0, 0, "우물천", "우물천장", &elsewhere)
            .unwrap();
        assert_eq!(counted, 2);
        assert_eq!(fixed.places, counted);
        assert_eq!(transcript.segments[1].edited, None);
        assert_eq!(transcript.segments[2].said(), "우물천장에서");
    }
}
