//! A recording's transcript: who said what, and when in the original audio.

use serde::{Deserialize, Serialize};
use specta::Type;

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
        let mut out = String::new();
        for paragraph in self.paragraphs() {
            out.push_str(&format!("[{}] ", clock(paragraph.start_ms)));
            if self.has_speakers() {
                out.push_str(&format!("{}: ", self.speaker_name(paragraph.speaker)));
            }
            out.push_str(&paragraph.text);
            out.push('\n');
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
}
