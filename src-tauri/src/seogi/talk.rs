//! How the talking went, measured from the transcript's times and speakers:
//! who spoke how much, how often someone took over before the other had
//! finished, and the questions asked. No language model is involved; these
//! are counts, as rough as the speaker separation they rest on.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::transcript::{Segment, Transcript};

/// A speaker's next line starting this soon after the other's ended (or
/// before) counts as taking over.
const CUT_IN_GAP_MS: i64 = 250;
/// Lines of one speaker this close together are one turn.
const TURN_GAP_MS: u64 = 2_000;
/// The most moments of taking over shown to be heard.
const MOMENTS: usize = 3;

/// One speaker's share of the talking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SpeakerTalk {
    /// The speaker's number in the transcript; the name is read from it
    /// where it is shown, so renaming a speaker carries over.
    pub speaker: u32,
    pub talk_ms: u64,
    /// Of everyone's talking, 0 to 100.
    pub share: f32,
    /// Times the speaker took the floor.
    pub turns: u32,
    /// The longest stretch the speaker talked alone, and where it began.
    pub longest_ms: u64,
    pub longest_at_ms: u64,
    /// Lines ending in a question.
    pub questions: u32,
    /// Times the speaker took over before the one talking had finished.
    pub cut_ins: u32,
}

/// A moment someone took over before the other had finished.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CutIn {
    pub at_ms: u64,
    /// Who took over, and from whom.
    pub by: u32,
    pub from: u32,
    /// How long the two talked over each other, 0 when the second started
    /// just after the first stopped.
    pub overlap_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct TalkStats {
    /// The speakers, the one who talked most first.
    pub speakers: Vec<SpeakerTalk>,
    /// The clearest moments of taking over, in the order they happened.
    pub moments: Vec<CutIn>,
}

/// A line that only acknowledges ("네", "예, 예", "아 그렇죠"): not a turn
/// of its own, and not taking over.
fn backchannel(text: &str) -> bool {
    const WORDS: [&str; 14] = [
        "네",
        "예",
        "아",
        "어",
        "음",
        "응",
        "그렇죠",
        "그쵸",
        "맞아요",
        "맞습니다",
        "그러네요",
        "아하",
        "네네",
        "좋아요",
    ];
    let words: Vec<String> = text
        .split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
        })
        .filter(|word| !word.is_empty())
        .collect();
    !words.is_empty() && words.len() <= 3 && words.iter().all(|word| WORDS.contains(&word.as_str()))
}

/// Whether a line ends as a sentence does.
fn finished(text: &str) -> bool {
    let text = text.trim_end();
    if text.ends_with(['.', '?', '!', '…']) {
        return true;
    }
    ["요", "다", "죠", "까", "네", "게"]
        .iter()
        .any(|end| text.ends_with(end))
}

fn spoken(segments: &[Segment]) -> impl Iterator<Item = &Segment> {
    segments
        .iter()
        .filter(|segment| segment.speaker != 0 && segment.end_ms > segment.start_ms)
}

/// The talking in `transcript`, measured.
pub fn measure(transcript: &Transcript) -> TalkStats {
    let mut speakers: Vec<SpeakerTalk> = Vec::new();
    let slot = |speakers: &mut Vec<SpeakerTalk>, id: u32| -> usize {
        match speakers.iter().position(|s| s.speaker == id) {
            Some(at) => at,
            None => {
                speakers.push(SpeakerTalk {
                    speaker: id,
                    talk_ms: 0,
                    share: 0.0,
                    turns: 0,
                    longest_ms: 0,
                    longest_at_ms: 0,
                    questions: 0,
                    cut_ins: 0,
                });
                speakers.len() - 1
            }
        }
    };
    let mut cut_ins: Vec<CutIn> = Vec::new();
    // The turn in progress: who, from when, until when.
    let mut turn: Option<(u32, u64, u64)> = None;
    let mut previous: Option<&Segment> = None;
    let close_turn = |speakers: &mut Vec<SpeakerTalk>, turn: Option<(u32, u64, u64)>| {
        if let Some((id, from, to)) = turn {
            if let Some(entry) = speakers.iter_mut().find(|s| s.speaker == id) {
                entry.turns += 1;
                if to - from > entry.longest_ms {
                    entry.longest_ms = to - from;
                    entry.longest_at_ms = from;
                }
            }
        }
    };
    for segment in spoken(&transcript.segments) {
        let at = slot(&mut speakers, segment.speaker);
        speakers[at].talk_ms += segment.end_ms - segment.start_ms;
        if segment.text.trim_end().ends_with('?') {
            speakers[at].questions += 1;
        }
        let acknowledges = backchannel(&segment.text);
        if let Some(before) = previous {
            let gap = segment.start_ms as i64 - before.end_ms as i64;
            if before.speaker != segment.speaker
                && !acknowledges
                && !backchannel(&before.text)
                && (segment.overlap || (gap < CUT_IN_GAP_MS && !finished(&before.text)))
            {
                speakers[at].cut_ins += 1;
                cut_ins.push(CutIn {
                    at_ms: segment.start_ms,
                    by: segment.speaker,
                    from: before.speaker,
                    overlap_ms: before.end_ms.saturating_sub(segment.start_ms),
                });
            }
        }
        // A word of acknowledgement does not take the floor.
        if !acknowledges {
            turn = match turn {
                Some((id, from, to))
                    if id == segment.speaker && segment.start_ms <= to + TURN_GAP_MS =>
                {
                    Some((id, from, segment.end_ms.max(to)))
                }
                other => {
                    close_turn(&mut speakers, other);
                    Some((segment.speaker, segment.start_ms, segment.end_ms))
                }
            };
            previous = Some(segment);
        }
    }
    close_turn(&mut speakers, turn);

    let total: u64 = speakers.iter().map(|s| s.talk_ms).sum();
    for entry in &mut speakers {
        entry.share = if total == 0 {
            0.0
        } else {
            (entry.talk_ms as f64 * 1000.0 / total as f64).round() as f32 / 10.0
        };
    }
    speakers.sort_by_key(|s| std::cmp::Reverse(s.talk_ms));

    // The clearest moments: the most talked over, then the earliest.
    let mut moments = cut_ins;
    moments.sort_by_key(|m| (std::cmp::Reverse(m.overlap_ms), m.at_ms));
    moments.truncate(MOMENTS);
    moments.sort_by_key(|m| m.at_ms);
    TalkStats { speakers, moments }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(start_ms: u64, end_ms: u64, speaker: u32, text: &str) -> Segment {
        Segment {
            start_ms,
            end_ms,
            speaker,
            text: text.into(),
            ..Default::default()
        }
    }

    #[test]
    fn talking_is_measured_per_speaker_with_takeovers_and_questions() {
        let transcript = Transcript {
            segments: vec![
                said(0, 10_000, 1, "오늘은 주방 배치부터 보겠습니다."),
                said(10_500, 12_000, 1, "냉장고는 몇 대 쓰세요?"),
                said(12_100, 20_000, 2, "두 대인데 하나는 김치냉장고라서"),
                // Takes over before the customer finished the sentence.
                said(20_100, 30_000, 1, "그럼 키친핏으로 바꾸시는 게 좋습니다."),
                // A word of acknowledgement is neither a turn nor taking over.
                said(30_050, 30_400, 2, "네 네"),
                said(30_500, 35_000, 1, "안쪽에 넣으면 됩니다."),
                // Talked over.
                Segment {
                    overlap: true,
                    ..said(34_000, 40_000, 2, "그건 좀 생각해 볼게요.")
                },
            ],
            ..Default::default()
        };
        let stats = measure(&transcript);
        assert_eq!(stats.speakers.len(), 2);
        let (ours, theirs) = (&stats.speakers[0], &stats.speakers[1]);
        assert_eq!(ours.speaker, 1);
        assert_eq!(ours.talk_ms, 10_000 + 1_500 + 9_900 + 4_500);
        assert_eq!(theirs.talk_ms, 7_900 + 350 + 6_000);
        assert!((ours.share + theirs.share - 100.0).abs() < 0.2);
        assert_eq!(ours.questions, 1);
        assert_eq!(ours.cut_ins, 1);
        assert_eq!(theirs.cut_ins, 1);
        // 0 to 12 s is one turn; 20.1 to 35 s another, the "네 네" between
        // them not counting.
        assert_eq!(ours.turns, 2);
        assert_eq!(ours.longest_ms, 14_900);
        assert_eq!(ours.longest_at_ms, 20_100);
        assert_eq!(stats.moments.len(), 2);
        assert_eq!(stats.moments[0].at_ms, 20_100);
        assert_eq!(stats.moments[1].overlap_ms, 1_000);
    }

    #[test]
    fn acknowledgements_and_finished_sentences_are_not_taking_over() {
        assert!(backchannel("네"));
        assert!(backchannel("아, 네네."));
        assert!(!backchannel("네 그렇게 해 주세요"));
        assert!(finished("그렇게 하겠습니다."));
        assert!(finished("괜찮으세요?"));
        assert!(finished("좋겠네"));
        assert!(!finished("그런데 그 부분이"));
        assert_eq!(measure(&Transcript::default()), TalkStats::default());
    }
}
