//! The story and the feedback, checked against the recording.
//!
//! Tasks and decisions are put to the checker beside the lines they cite;
//! the story and the consultation feedback are written afterwards, from those
//! entries, and a writer can still say more than was said: call a plan that
//! was only talked about "무리", say a worry went unanswered when it was
//! answered a line later, or put a time on something the lines do not. So
//! every sentence of the story and every point of the feedback is shown to
//! the checker beside the transcript lines it rests on (the lines around
//! each place its entries were said), and the checker says whether those
//! lines say it.
//!
//! What the lines say stays as it is. What they say less strongly is
//! replaced by the checker's softer sentence, when that adds no number the
//! sentence and the lines do not have; otherwise it is left out. What they
//! do not say is left out. Nothing new is written.
//!
//! The request and the reading of its answer are plain functions here; the
//! minutes ask the model.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::transcript::{clock, Transcript};

/// The most places one claim's lines are taken from.
pub const CLAIM_PLACES: usize = 4;
/// Lines shown before and after each place.
const BEFORE: usize = 2;
const AFTER: usize = 3;
/// The most claims, and characters, in one request.
const BATCH_CLAIMS: usize = 12;
const BATCH_CHARS: usize = 9_000;

/// A sentence of the story or a point of the feedback, with the lines it
/// rests on as the checker reads them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub text: String,
    /// The entries of the minutes it rests on, as written: they say what it
    /// points at, but may themselves be written too strongly.
    pub items: Vec<String>,
    /// The transcript lines around the places it rests on, one per line:
    /// `[00:12:04] 이름: 말`.
    pub lines: String,
}

/// What becomes of a claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judgement {
    /// The lines say it (or the checker could not be asked: it stays as
    /// written, as before there was a check).
    Keep,
    /// The lines say less: `text` is what they say, `beyond` the words of
    /// the claim that went past them, when the checker named them.
    Soften { text: String, beyond: String },
    /// The lines do not say it, or say less and no softer sentence could be
    /// taken. `beyond` is the words that go past the lines, as written in
    /// the claim, when the checker named them.
    Drop { beyond: String },
}

impl Judgement {
    /// The words that went past the lines, for a claim that is not kept as
    /// written.
    pub fn beyond(&self) -> &str {
        match self {
            Judgement::Drop { beyond } | Judgement::Soften { beyond, .. } => beyond,
            Judgement::Keep => "",
        }
    }
}

/// The lines of `transcript` around `places` (start times of segments), as
/// the checker reads them; overlapping stretches are shown once.
pub fn lines_around(transcript: &Transcript, places: &[u64]) -> String {
    let segments = &transcript.segments;
    if segments.is_empty() {
        return String::new();
    }
    let mut ranges: Vec<(usize, usize)> = places
        .iter()
        .take(CLAIM_PLACES)
        .map(|&ms| {
            // The segment said at that time, or the last one before it.
            let at = segments
                .iter()
                .rposition(|segment| segment.start_ms <= ms)
                .unwrap_or(0);
            (
                at.saturating_sub(BEFORE),
                (at + AFTER).min(segments.len() - 1),
            )
        })
        .collect();
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (first, last) in ranges {
        match merged.last_mut() {
            Some((_, end)) if first <= *end + 1 => *end = (*end).max(last),
            _ => merged.push((first, last)),
        }
    }
    let mut out = String::new();
    for (n, (first, last)) in merged.iter().enumerate() {
        if n > 0 {
            out.push_str("…\n");
        }
        for segment in &segments[*first..=*last] {
            out.push_str(&format!(
                "[{}] {}: {}\n",
                clock(segment.start_ms),
                transcript.speaker_name(segment.speaker),
                segment.text
            ));
        }
    }
    out
}

/// Groups claims into requests, in order: at most `BATCH_CLAIMS` claims and
/// `BATCH_CHARS` characters each; a claim longer than that goes alone.
pub fn batches(claims: &[Claim]) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut chars = 0;
    for (at, claim) in claims.iter().enumerate() {
        let size = claim.text.chars().count()
            + claim.lines.chars().count()
            + claim.items.iter().map(|i| i.chars().count()).sum::<usize>();
        let full = out
            .last()
            .is_none_or(|batch| batch.len() >= BATCH_CLAIMS || chars + size > BATCH_CHARS);
        if full {
            out.push(Vec::new());
            chars = 0;
        }
        if let Some(batch) = out.last_mut() {
            batch.push(at);
        }
        chars += size;
    }
    out
}

/// The instructions for the checker.
pub const SYSTEM: &str = "당신은 회의록을 검수하는 사람입니다. 회의록의 '이야기 요약' 문장과 '상담 피드백' 문장이 번호별로 주어집니다. 문장마다 그 문장이 기댄 회의록 항목(근거 항목)과, 그 항목들이 나온 녹음 대목이 함께 주어집니다. 판정은 녹음 대목으로 합니다. 근거 항목은 문장이 무엇을 가리키는지 알려 줄 뿐이고, 항목 자체가 대목보다 세게 적혔을 수도 있습니다. 받아쓰기라서 낱말이 조금 틀리거나 화자 이름이 잘못 붙어 있을 수 있습니다.\n\n\
verdict:\n\
- supported: 대목이 그 문장을 말하거나, 문장이 근거 항목을 줄이거나 다르게 표현했을 뿐이고 대목과 어긋나지 않는다. 대목은 회의의 일부만 보여 주므로, 대목에 안 보이는 세부가 근거 항목에 있으면 supported입니다.\n\
- overstated: 문장이 대목보다 세게, 넓게, 확정적으로 말한다. 예: 이야기만 하거나 제안만 한 것을 정했다고 함, 일부에 한 말을 전체에 한 말로 넓힘('창가 끝까지는 어렵다'를 '옮기는 것은 무리'로), 대목에 없는 판단·평가·이유를 붙임. 근거 항목이 그렇게 적었더라도 대목이 그만큼 말하지 않으면 overstated입니다.\n\
- unsupported: 대목이 반대로 말한다. 또는 문장이 말하는 시각·순서가 대목의 시각과 맞지 않는다. 또는 문장의 내용이 근거 항목에도 대목에도 없다.\n\
'~하면 좋겠다', '~해 둔다', '물었으면 좋았겠다'처럼 권하는 문장은 그 안에 든 사실(무엇이 있었다, 없었다)만 판정합니다. 권하는 말 자체는 대목에 없어도 supported이고, 권하는 문장을 있었던 일로 고쳐 쓰지 않습니다. 그 사실이 대목과 어긋나면 unsupported입니다.\n\
'~하지 않았다', '답한 기록이 없다', '항목에 없다', '~뒤에야 나왔다'처럼 없었다거나 늦었다고 말하는 문장은, 대목에 그 반대(답했다, 일찍 나왔다)가 보이면 unsupported입니다. 반대가 보이지 않으면 supported로 둡니다.\n\
문장의 일부만 대목을 넘거나 대목에 없으면 문장 전체를 버리지 말고 overstated로 하고, softer에 그 부분만 빼거나 대목만큼 줄인 문장을 씁니다. unsupported는 문장이 말하려는 요점이 대목과 어긋나거나 어디에도 없을 때만 씁니다.\n\
softer: overstated일 때만, 대목이 말하는 만큼만 말하도록 고쳐 쓴 한 문장. 원래 문장과 대목에 있는 낱말로만 쓰고 말투(~했다)를 살립니다. 새 낱말, 이름, 숫자를 보태지 않습니다. 그 밖에는 빈 문자열.\n\
beyond: overstated나 unsupported일 때, 문장에서 대목을 넘어선 낱말을 문장에 쓰인 그대로 짧게(예: '무리'). 그 밖에는 빈 문자열.\n\n\
모든 번호에 대해 하나씩 답합니다.";

/// The request for one batch: the claims numbered from 1.
pub fn request(claims: &[Claim], batch: &[usize]) -> String {
    batch
        .iter()
        .enumerate()
        .map(|(number, &at)| {
            let claim = &claims[at];
            let items: String = claim
                .items
                .iter()
                .map(|item| format!("- {item}\n"))
                .collect();
            format!(
                "### {}\n문장: {}\n근거 항목:\n{items}녹음 대목:\n{}\n",
                number + 1,
                claim.text,
                claim.lines
            )
        })
        .collect()
}

/// The shape of the checker's answer.
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "results": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "integer" },
                        "verdict": { "type": "string", "enum": ["supported", "overstated", "unsupported"] },
                        "softer": { "type": "string" },
                        "beyond": { "type": "string" }
                    },
                    "required": ["id", "verdict", "softer", "beyond"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["results"],
        "additionalProperties": false
    })
}

/// Words that tell how something was said, not what: a softer sentence may
/// use them though no one said them ("이야기했다", "제안했다", "아직").
const TELLING: &[&str] = &[
    "이야", "말했", "말하", "말을", "제안", "안내", "설명", "했다", "했고", "했으", "하기", "하고",
    "하는", "하면", "한다", "되었", "됐다", "된다", "정도", "아직", "다만", "우리", "고객", "같이",
    "함께", "조금", "다시", "그리", "또는", "보기", "보자", "의견", "생각", "검토", "물었", "답했",
];

/// Whole small words that carry no fact of their own: bound nouns with a
/// particle ("것은", "수도", "때는") and a few adverbs. Any other word is
/// checked against what was said.
const BOUND: &[&str] = &[
    "것은", "것이", "것을", "것도", "것과", "수도", "수는", "수가", "수를", "때는", "때도", "때에",
    "등을", "등이", "등은", "데는", "데도", "점은", "점을", "점이", "쪽은", "쪽도", "쪽이", "중에",
    "중인", "안에", "더는", "또는", "그런", "이런", "저런", "그냥", "이미", "아직", "바로", "먼저",
    "모두", "따로",
];

/// Single letters that are particles, counters or adverbs ("두 곳", "더",
/// "좀"), never a name. Not "안" or "못": a negation must have been said.
const SMALL: &[char] = &[
    '이', '가', '은', '는', '을', '를', '도', '만', '및', '등', '수', '것', '때', '더', '또', '잘',
    '꼭', '좀', '곧', '한', '두', '세', '네', '그', '저', '와', '과', '로', '에',
];

/// Whether every word of `softer` comes from `source`: each word's first two
/// letters (its stem, near enough for Korean endings) are found there, or
/// it only tells how something was said. A softer sentence may reword, but
/// not bring in a word no one said ("메모했다", a name, a material).
fn words_from(softer: &str, source: &str) -> bool {
    let source: String = source.chars().filter(|c| c.is_alphanumeric()).collect();
    softer.split_whitespace().all(|word| {
        let letters: Vec<char> = word.chars().filter(|c| c.is_alphanumeric()).collect();
        let stem: String = letters.iter().take(2).collect();
        let whole: String = letters.iter().collect();
        match letters.len() {
            0 => true,
            // A single letter: a particle or counter, or one that was said
            // (a one-letter surname is not).
            1 => SMALL.contains(&letters[0]) || source.contains(letters[0]),
            _ => {
                BOUND.contains(&whole.as_str())
                    || TELLING.contains(&stem.as_str())
                    || source.contains(&stem)
            }
        }
    })
}

/// Whether a softer sentence may stand for `claim`: it is a sentence, not the
/// claim again, not much longer, made of words the claim and its lines have,
/// adds no number they do not have, and no longer has the words that went
/// too far.
fn softer_taken(softer: &str, claim: &Claim, beyond: &str) -> bool {
    let softer = softer.trim();
    let bare = |text: &str| -> String { text.chars().filter(|c| c.is_alphanumeric()).collect() };
    let source = format!("{} {}", claim.text, claim.lines);
    !softer.is_empty()
        && bare(softer) != bare(&claim.text)
        && softer.chars().count() <= claim.text.chars().count() + 20
        && words_from(softer, &source)
        && super::amounts::backed(softer, &source)
        && super::amounts::backed(&super::numbers::spoken_to_digits(softer), &source)
        && (bare(beyond).chars().count() < 2 || !bare(softer).contains(&bare(beyond)))
}

/// The checker's answer for one batch, by position in `claims`. A claim the
/// answer skips is not in the map.
pub fn read(answer: &Value, claims: &[Claim], batch: &[usize]) -> BTreeMap<usize, Judgement> {
    let mut out = BTreeMap::new();
    for result in answer
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(at) = result
            .get("id")
            .and_then(Value::as_u64)
            .and_then(|id| (id as usize).checked_sub(1))
            .and_then(|n| batch.get(n).copied())
        else {
            continue;
        };
        let text = |key: &str| {
            result
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let beyond = text("beyond");
        let judgement = match result.get("verdict").and_then(Value::as_str) {
            Some("supported") => Judgement::Keep,
            Some("overstated") => {
                let softer = text("softer");
                if softer_taken(&softer, &claims[at], &beyond) {
                    Judgement::Soften {
                        text: softer,
                        beyond,
                    }
                } else {
                    Judgement::Drop { beyond }
                }
            }
            Some("unsupported") => Judgement::Drop { beyond },
            _ => continue,
        };
        out.insert(at, judgement);
    }
    out
}

/// Whether the words that went past the lines are in `text`: the entry a
/// sentence rests on said it so already.
pub fn says(text: &str, beyond: &str) -> bool {
    let bare = |text: &str| -> String { text.chars().filter(|c| c.is_alphanumeric()).collect() };
    let words = bare(beyond);
    words.chars().count() >= 2 && bare(text).contains(&words)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seogi::transcript::{Segment, Speaker};

    fn transcript(lines: &[(u64, u32, &str)]) -> Transcript {
        Transcript {
            language: "ko".into(),
            model: "test".into(),
            segments: lines
                .iter()
                .map(|&(ms, speaker, text)| Segment {
                    start_ms: ms,
                    end_ms: ms + 2_000,
                    speaker,
                    text: text.into(),
                    confidence: Some(0.9),
                    unsure: false,
                    overlap: false,
                    edited: None,
                    fixes: Vec::new(),
                })
                .collect(),
            speakers: vec![
                Speaker {
                    id: 1,
                    name: "실장".into(),
                },
                Speaker {
                    id: 2,
                    name: "고객".into(),
                },
            ],
            corrections: Vec::new(),
        }
    }

    fn claim(text: &str, lines: &str) -> Claim {
        Claim {
            text: text.into(),
            items: vec!["근거 항목".into()],
            lines: lines.into(),
        }
    }

    #[test]
    fn the_lines_around_each_place_are_shown_once() {
        let t = transcript(&[
            (0, 1, "안녕하세요"),
            (3_000, 2, "주방을 옮기고 싶어요"),
            (6_000, 1, "주방 옮기는 건 괜찮습니다"),
            (9_000, 2, "창가 끝까지는요"),
            (12_000, 1, "그건 조금 어렵습니다"),
            (15_000, 2, "네"),
            (18_000, 1, "다음 안건입니다"),
            (21_000, 2, "네"),
            (24_000, 1, "타일은요"),
            (27_000, 2, "생각해 볼게요"),
            (30_000, 1, "네"),
        ]);
        let lines = lines_around(&t, &[3_000, 6_000]);
        assert!(lines.starts_with("[00:00:00] 실장: 안녕하세요\n"));
        assert!(lines.contains("[00:00:06] 실장: 주방 옮기는 건 괜찮습니다\n"));
        assert!(lines.ends_with("[00:00:15] 고객: 네\n"));
        assert!(!lines.contains("다음 안건"));
        assert_eq!(lines.matches("주방 옮기는 건").count(), 1);
        // Two places far apart are shown as two stretches.
        let apart = lines_around(&t, &[0, 30_000]);
        assert!(apart.contains("…\n[00:00:24]"));
    }

    #[test]
    fn what_the_lines_do_not_say_is_left_out_and_what_they_say_less_is_softened() {
        let lines = "[00:00:06] 실장: 주방 옮기는 건 괜찮습니다\n[00:00:12] 실장: 창가 끝까지 가면 그건 조금 어렵습니다\n";
        let claims = vec![
            claim("주방을 거실 쪽으로 옮길 수 있다고 안내했다.", lines),
            claim("주방을 옮기는 안은 무리라고 판단했다.", lines),
            claim("주방을 옮기는 안은 무리라고 판단했다.", lines),
            claim("고객의 걱정에 답한 기록이 없다.", lines),
            claim("주방 이전 비용은 500만 원이다.", lines),
            claim("창가 끝까지 옮기는 것은 무리라고 했다.", lines),
        ];
        let batch: Vec<usize> = (0..claims.len()).collect();
        let answer = json!({ "results": [
            { "id": 1, "verdict": "supported", "softer": "", "beyond": "" },
            { "id": 2, "verdict": "overstated",
              "softer": "주방 옮기는 건 괜찮고, 창가 끝까지 옮기는 것은 조금 어렵다고 했다.", "beyond": "무리" },
            // A softer sentence that still says it is no softer.
            { "id": 3, "verdict": "overstated", "softer": "주방 이전은 무리라고 했다.", "beyond": "무리" },
            { "id": 4, "verdict": "unsupported", "softer": "", "beyond": "답한 기록이 없다" },
            // A softer sentence with a number no one said is not taken.
            { "id": 5, "verdict": "overstated", "softer": "주방 이전 비용은 300만 원 정도다.", "beyond": "500만 원" },
            // Nor one with a word no one said.
            { "id": 6, "verdict": "overstated", "softer": "창가 끝까지 옮기는 것은 조금 어렵다고 메모했다.", "beyond": "무리" },
            { "id": 9, "verdict": "unsupported", "softer": "", "beyond": "" }
        ]});
        let read = read(&answer, &claims, &batch);
        assert_eq!(read[&0], Judgement::Keep);
        assert_eq!(
            read[&1],
            Judgement::Soften {
                text: "주방 옮기는 건 괜찮고, 창가 끝까지 옮기는 것은 조금 어렵다고 했다.".into(),
                beyond: "무리".into()
            }
        );
        assert_eq!(
            read[&2],
            Judgement::Drop {
                beyond: "무리".into()
            }
        );
        assert_eq!(read[&3].beyond(), "답한 기록이 없다");
        assert!(matches!(read[&4], Judgement::Drop { .. }));
        assert!(matches!(read[&5], Judgement::Drop { .. }));
        // A number not asked about is ignored.
        assert_eq!(read.len(), 6);

        let request = request(&claims, &batch[..2]);
        assert!(request.starts_with("### 1\n문장: 주방을 거실 쪽으로"));
        assert!(request.contains("근거 항목:\n- 근거 항목\n녹음 대목:\n[00:00:06]"));
        assert!(request.contains("### 2\n문장: 주방을 옮기는 안은 무리"));
        assert!(!request.contains("### 3"));
    }

    #[test]
    fn a_softer_sentence_brings_in_no_word_no_one_said() {
        let source =
            "주방을 옮기는 안은 무리라고 판단했다. [00:00:06] 소장: 주방 옮기는 건 괜찮습니다";
        assert!(words_from("주방 옮기는 건 괜찮다고 했다.", source));
        // A name, a room, a material; an amount in words; a verdict.
        assert!(!words_from(
            "옆집 소장이 안방 중문을 수입 자재로 한다고 했다.",
            source
        ));
        assert!(!words_from("비용은 이천만 원 정도라고 했다.", source));
        assert!(!words_from("주방 이전은 가능하다고 정했다.", source));
        // Two-letter words that only start like a bound noun, and a
        // one-letter surname.
        assert!(!words_from(
            "소장이 안방 이사 자재는 수입 한다고 했다.",
            source
        ));
        assert!(!words_from(
            "김 소장이 주방 옮기는 건 괜찮다고 했다.",
            source
        ));
        assert!(words_from(
            "그 소장이 주방 옮기는 것은 괜찮다고 했다.",
            source
        ));
        // A negation no one said turns the sentence round.
        assert!(!words_from("답을 안 했다.", "답을 했다"));
        let claim = Claim {
            text: "주방을 옮기는 안은 무리라고 판단했다.".into(),
            items: Vec::new(),
            lines: "[00:00:06] 소장: 주방 옮기는 건 괜찮습니다".into(),
        };
        assert!(!softer_taken("주방 이전은 이천만 원이다.", &claim, "무리"));
    }

    #[test]
    fn claims_go_in_batches_of_a_size() {
        let small = claim("짧은 문장이다.", "[00:00:01] 실장: 네\n");
        let claims = vec![small.clone(); BATCH_CLAIMS + 1];
        assert_eq!(
            batches(&claims).iter().map(Vec::len).collect::<Vec<_>>(),
            [BATCH_CLAIMS, 1]
        );
        let long = claim("긴 문장이다.", &"가".repeat(BATCH_CHARS));
        let mixed = vec![small.clone(), long, small];
        assert_eq!(batches(&mixed), [vec![0], vec![1], vec![2]]);
    }

    #[test]
    fn the_entry_said_it_already_when_its_words_are_there() {
        assert!(says(
            "내력벽 때문에 주방을 옮기는 안은 무리라고 판단함",
            "무리"
        ));
        assert!(!says("주방을 옮기는 안을 이야기함", "무리"));
        // A single letter says nothing.
        assert!(!says("주방을 옮기는 안", "안"));
    }
}
