//! Checks of what the minutes state against the shape of the meeting, by
//! machine: when the meeting ended (what comes after is not part of it),
//! whether a fact is about this meeting's people or someone else's, and
//! which entries say different things about the same matter.
//!
//! These are plain functions of the text. They do not decide what is true;
//! they find what the model must be asked about again, or what a person
//! should hear before relying on it.

use std::collections::HashSet;

use super::transcript::{Segment, Transcript};

/// Words people part with. Said near the end by the people who leave, they
/// close the meeting.
const FAREWELLS: &[&str] = &[
    "안녕히가",
    "안녕히계",
    "조심히가",
    "조심히들어가",
    "들어가세요",
    "살펴가",
    "수고하셨",
    "수고많으셨",
    "고생하셨",
    "고생많으셨",
    "감사합니다",
    "감사했습니다",
    "다음에뵙",
    "다음에봬",
    "또뵙",
];

/// Words of a promise for later: said after a farewell, the meeting is not
/// over yet.
const PROMISES: &[&str] = &[
    "드릴게요",
    "드리겠습니다",
    "하겠습니다",
    "할게요",
    "보내드",
    "정리하면",
];

/// The farewell must come in the last part of the recording: after this
/// share of it.
const END_AFTER_SHARE: f64 = 0.6;
/// What follows the farewell is short: at most this long, and at most this
/// share of the recording. A longer stretch is more meeting, not chat.
const TAIL_MOST_MS: u64 = 5 * 60_000;
const TAIL_MOST_SHARE: f64 = 0.25;
/// Someone who said at least this share before the farewell and nothing
/// after it has left: the meeting is over.
const LEFT_SHARE: f64 = 0.1;

fn squeezed(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn is_farewell(text: &str) -> bool {
    let text = squeezed(text);
    FAREWELLS.iter().any(|word| text.contains(word))
}

/// When the meeting ended, if the recording goes on after it: the end of the
/// last farewell, when it comes late in the recording, little is said after
/// it, and someone who talked a fair share before says nothing after. What
/// is said after that (chat once the customer has left, another call) is
/// not this meeting's decisions or tasks. `None` when the recording ends
/// with the meeting, or the signs are not all there.
pub fn meeting_end(transcript: &Transcript) -> Option<u64> {
    let segments = &transcript.segments;
    let last_end = segments.iter().map(|s| s.end_ms).max()?;
    let first_start = segments.iter().map(|s| s.start_ms).min()?;
    let length = last_end.saturating_sub(first_start).max(1);
    let farewell = segments.iter().rposition(|s| is_farewell(&s.text))?;
    // Farewells are said back and forth: the meeting ends with the last of
    // them, and the chat begins with the first line after that.
    let end_ms = segments[farewell].end_ms;
    let tail: Vec<&Segment> = segments[farewell + 1..]
        .iter()
        .filter(|s| s.start_ms >= end_ms.saturating_sub(500))
        .collect();
    if tail.is_empty() {
        return None;
    }
    let tail_ms = last_end.saturating_sub(end_ms);
    let late = (end_ms - first_start) as f64 >= END_AFTER_SHARE * length as f64;
    let short = tail_ms <= TAIL_MOST_MS && tail_ms as f64 <= TAIL_MOST_SHARE * length as f64;
    if !late || !short {
        return None;
    }
    // Who talked how much before the end, and who still talks after.
    let mut before: std::collections::BTreeMap<u32, u64> = Default::default();
    let mut spoken = 0u64;
    for segment in &segments[..=farewell] {
        let ms = segment.end_ms.saturating_sub(segment.start_ms);
        *before.entry(segment.speaker).or_default() += ms;
        spoken += ms;
    }
    // A promise after the farewell ("정리하면 제가 금요일까지 보내드릴게요")
    // is still the meeting.
    if tail.iter().any(|s| {
        let text = squeezed(&s.text);
        PROMISES.iter().any(|word| text.contains(word))
    }) {
        return None;
    }
    let after: HashSet<u32> = tail.iter().map(|s| s.speaker).collect();
    let someone_left = before.iter().any(|(speaker, ms)| {
        *speaker != 0 && *ms as f64 >= LEFT_SHARE * spoken as f64 && !after.contains(speaker)
    });
    someone_left.then_some(end_ms)
}

/// Words that put what is said on someone or somewhere other than this
/// meeting's customer and site: another site, another customer, a past case.
const ELSEWHERE: &[&str] = &[
    "다른현장",
    "다른집",
    "다른고객",
    "다른분",
    "다른아파트",
    "그현장",
    "그집은",
    "지난번현장",
    "예전현장",
    "옆집",
    "사례",
    "우리사무실",
    "저희사무실",
];

/// A speaker's own home: the customer's is the site, anyone else's is not.
const OWN_HOME: &[&str] = &["우리집", "저희집", "제집", "우리아파트", "저희아파트"];

/// Words in an entry's own text that already say whose it is.
const SAYS_WHOSE: &[&str] = &[
    "사례",
    "다른현장",
    "다른집",
    "다른고객",
    "참고로",
    "예시",
    "직원",
    "실장님집",
    "소장님집",
];

/// Whether `lines` (the words an entry rests on) talk about another place
/// or person than this meeting's, while the entry's `text` does not say so.
/// Such an entry may have moved someone else's story onto the customer.
/// `by_customer` says the lines were the customer's own: their home is the
/// site, so "저희 집" from them is no one else's.
pub fn about_someone_else(text: &str, lines: &str, by_customer: bool) -> bool {
    let lines = squeezed(lines);
    let text = squeezed(text);
    let elsewhere = ELSEWHERE.iter().any(|word| lines.contains(word))
        || (!by_customer && OWN_HOME.iter().any(|word| lines.contains(word)));
    elsewhere && !SAYS_WHOSE.iter().any(|word| text.contains(word))
}

/// Whether a speaker of that name is the customer: called so, or named as
/// the customer of the meeting.
pub fn is_customer(name: &str, customer: &str) -> bool {
    let name = squeezed(name);
    name.contains("고객") || (!customer.trim().is_empty() && name.contains(&squeezed(customer)))
}

/// Words too common to say two entries are about the same thing.
const COMMON: &[&str] = &[
    "고객",
    "고객님",
    "우리",
    "저희",
    "하기",
    "하기로",
    "했다",
    "한다",
    "합니다",
    "정도",
    "것",
    "있음",
    "없음",
    "기로",
    "으로",
    "에서",
    "까지",
    "그리고",
    "다시",
    "아직",
    "지금",
    "이번",
    "다음",
    "말함",
    "이야기",
    "확인",
    "결정",
    "정함",
    "정했다",
    "생각",
    "같이",
    "위해",
    "대해",
    "관련",
    "기준",
    "포함",
];

/// The content words of a text: runs of letters or digits of two or more,
/// with a particle at the end taken off, and the common ones left out.
pub fn content_words(text: &str) -> HashSet<String> {
    const PARTICLES: &[&str] = &[
        "으로는",
        "에서는",
        "으로",
        "에서",
        "에는",
        "에게",
        "까지",
        "부터",
        "보다",
        "은",
        "는",
        "이",
        "가",
        "을",
        "를",
        "에",
        "도",
        "만",
        "의",
        "로",
        "와",
        "과",
    ];
    let mut words = HashSet::new();
    for raw in text.split(|c: char| !c.is_alphanumeric()) {
        let mut word = raw.to_lowercase();
        for particle in PARTICLES {
            if word.chars().count() > particle.chars().count() + 1 && word.ends_with(particle) {
                word.truncate(word.len() - particle.len());
                break;
            }
        }
        if word.chars().count() >= 2 && !COMMON.contains(&word.as_str()) {
            words.insert(word);
        }
    }
    words
}

/// How two entries may disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clash {
    /// Something settled and the same thing called unsettled.
    SettledAndOpen,
    /// The same matter with different numbers of the same kind.
    Numbers,
    /// Two near-alike statements in the same section that differ.
    Alike,
}

/// A section of facts and choices whose entries may contradict each other.
/// Tasks are read on their own, later.
pub struct Said<'a> {
    pub section: &'a str,
    pub text: &'a str,
    pub ms: u64,
}

/// Pairs of entries (by position in `said`, the earlier first) that may say
/// different things about the same matter, and how. `open` is the key of
/// the section of unsettled things. Only a pair the model is then asked
/// about; at most `most` of them, the closest first.
pub fn clashes(said: &[Said<'_>], open: &str, most: usize) -> Vec<(usize, usize, Clash)> {
    let words: Vec<HashSet<String>> = said.iter().map(|s| content_words(s.text)).collect();
    let mut found: Vec<(usize, usize, Clash, usize)> = Vec::new();
    for a in 0..said.len() {
        for b in a + 1..said.len() {
            let (x, y) = if said[a].ms <= said[b].ms {
                (a, b)
            } else {
                (b, a)
            };
            let shared = words[x].intersection(&words[y]).count();
            if shared == 0 {
                continue;
            }
            let open_x = said[x].section == open;
            let open_y = said[y].section == open;
            let clash = if open_x != open_y
                && (said[x].section == "decisions" || said[y].section == "decisions")
            {
                Some(Clash::SettledAndOpen)
            } else if !open_x && !open_y && numbers_disagree(said[x].text, said[y].text) {
                Some(Clash::Numbers)
            } else if said[x].section == said[y].section
                && !open_x
                && said[x].text != said[y].text
                && shared >= 2
                && shared * 2 >= words[x].len().min(words[y].len())
            {
                Some(Clash::Alike)
            } else {
                None
            };
            if let Some(clash) = clash {
                found.push((x, y, clash, shared));
            }
        }
    }
    // Something settled called open first (the costliest to get wrong),
    // then different numbers; within each, the pairs that share the most.
    // An entry in one pair at most.
    let rank = |clash: Clash| match clash {
        Clash::SettledAndOpen => 0,
        Clash::Numbers => 1,
        Clash::Alike => 2,
    };
    found.sort_by_key(|&(_, _, clash, shared)| (rank(clash), std::cmp::Reverse(shared)));
    let mut used: HashSet<usize> = HashSet::new();
    let mut pairs = Vec::new();
    for (x, y, clash, _) in found {
        if used.contains(&x) || used.contains(&y) {
            continue;
        }
        used.insert(x);
        used.insert(y);
        pairs.push((x, y, clash));
        if pairs.len() >= most {
            break;
        }
    }
    pairs
}

/// The kind of thing a number counts, from the word after it.
fn number_kinds(text: &str) -> Vec<(String, String)> {
    const KINDS: &[&str] = &[
        "평",
        "층",
        "억",
        "만원",
        "만",
        "원",
        "개",
        "명",
        "m",
        "미터",
        "센티",
        "밀리",
        "%",
        "퍼센트",
        "주",
        "일",
        "시",
        "월",
        "년",
        "동",
        "호",
    ];
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut number = String::new();
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == ',' || chars[i] == '.') {
            if chars[i] != ',' {
                number.push(chars[i]);
            }
            i += 1;
        }
        let rest: String = chars[i..]
            .iter()
            .filter(|c| !c.is_whitespace())
            .take(3)
            .collect();
        if let Some(kind) = KINDS.iter().find(|kind| rest.starts_with(*kind)) {
            found.push((kind.to_string(), number.trim_end_matches('.').to_string()));
        }
    }
    found
}

/// Whether two texts give different numbers of a kind, and neither gives
/// the other's: "32평" and "58평", not "32평" and "32평 3층".
fn numbers_disagree(a: &str, b: &str) -> bool {
    let x = number_kinds(a);
    let y = number_kinds(b);
    let values = |found: &[(String, String)], kind: &str| -> HashSet<String> {
        found
            .iter()
            .filter(|(k, _)| k == kind)
            .map(|(_, v)| v.clone())
            .collect()
    };
    x.iter().any(|(kind, _)| {
        let (mine, theirs) = (values(&x, kind), values(&y, kind));
        !theirs.is_empty() && mine.is_disjoint(&theirs)
    })
}

/// Whether a recording seems to start in the middle of talk: words from the
/// first moment, a long first line, and nothing in the first minute and a
/// half that opens a meeting. The people should know the minutes miss the
/// start.
pub fn starts_mid_talk(transcript: &Transcript) -> bool {
    const OPENINGS: &[&str] = &[
        "안녕하세요",
        "안녕하십니까",
        "반갑습니다",
        "처음뵙",
        "시작하겠습니다",
        "시작할게요",
        "시작하죠",
        "시작합시다",
        "오늘은",
        "오늘안건",
        "녹음",
    ];
    let Some(first) = transcript.segments.first() else {
        return false;
    };
    if first.start_ms > 1_500 || first.text.chars().filter(|c| !c.is_whitespace()).count() < 30 {
        return false;
    }
    let opening = transcript
        .segments
        .iter()
        .take_while(|s| s.start_ms <= first.start_ms + 90_000)
        .any(|s| {
            let text = squeezed(&s.text);
            OPENINGS.iter().any(|word| text.contains(word))
        });
    !opening
}

/// Whether a line heard in a Korean meeting is the speech model's noise:
/// a broken character, mostly letters of a script that is neither Korean
/// nor Latin, or mostly lowercase Latin words. It is shown faintly and the
/// minutes do not rest on it; the transcript keeps it as heard. The same
/// rule as `isGibberish` in src/seogi/lib/gibberish.ts; both are tested
/// against gibberish.cases.json.
pub fn is_gibberish(text: &str) -> bool {
    if text.contains('\u{FFFD}') {
        return true;
    }
    let is_hangul = |c: char| matches!(c, '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' | '\u{AC00}'..='\u{D7AF}');
    let (mut letters, mut hangul, mut latin, mut foreign) = (0usize, 0usize, 0usize, 0usize);
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        letters += 1;
        if is_hangul(c) {
            hangul += 1;
        } else if c.is_ascii_alphabetic() {
            latin += 1;
        } else {
            foreign += 1;
        }
    }
    if letters < 4 {
        return false;
    }
    let share = hangul as f32 / letters as f32;
    if share < 0.5 && foreign >= 2 {
        return true;
    }
    // A lowercase Latin word of three letters or more, not an acronym.
    let chars: Vec<char> = text.chars().collect();
    let lower_word = (0..chars.len()).any(|i| {
        (i == 0 || !chars[i - 1].is_ascii_alphabetic())
            && chars[i..]
                .iter()
                .take(3)
                .filter(|c| c.is_ascii_lowercase())
                .count()
                == 3
    });
    share < 0.3 && latin >= 4 && lower_word
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seogi::transcript::Speaker;

    fn talk(lines: &[(u32, &str)]) -> Transcript {
        let mut at = 0;
        let segments = lines
            .iter()
            .map(|(speaker, text)| {
                let start = at;
                at += 10_000;
                Segment {
                    start_ms: start,
                    end_ms: start + 9_000,
                    speaker: *speaker,
                    text: (*text).to_string(),
                    confidence: None,
                    unsure: false,
                    overlap: false,
                }
            })
            .collect();
        Transcript {
            language: "ko".into(),
            model: String::new(),
            segments,
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
            corrections: Vec::new(),
        }
    }

    #[test]
    fn chat_after_the_customer_leaves_is_after_the_end() {
        let mut lines: Vec<(u32, &str)> = Vec::new();
        for _ in 0..8 {
            lines.push((1, "도면은 이렇게 바꾸겠습니다."));
            lines.push((2, "네 좋아요 그렇게 해 주세요."));
        }
        lines.push((1, "오늘 감사합니다."));
        lines.push((2, "네 감사합니다. 안녕히 계세요."));
        lines.push((1, "아까 다른 현장은 9000만 원 나왔다고 했어요."));
        let transcript = talk(&lines);
        assert_eq!(meeting_end(&transcript), Some(17 * 10_000 + 9_000));
    }

    #[test]
    fn a_thanks_while_everyone_goes_on_talking_is_no_end() {
        let mut lines: Vec<(u32, &str)> = Vec::new();
        for _ in 0..8 {
            lines.push((1, "도면은 이렇게 바꾸겠습니다."));
            lines.push((2, "네 좋아요."));
        }
        lines.push((2, "감사합니다."));
        lines.push((1, "그리고 하나 더 있습니다."));
        lines.push((2, "네 말씀하세요."));
        assert_eq!(meeting_end(&talk(&lines)), None);
        // A promise after a thanks is still the meeting.
        let mut promised: Vec<(u32, &str)> = lines[..16].to_vec();
        promised.push((2, "네 감사합니다."));
        promised.push((1, "정리하면 제가 금요일까지 도면 보내드릴게요."));
        assert_eq!(meeting_end(&talk(&promised)), None);
        // A recording that ends with the farewell has nothing after it.
        lines.push((1, "감사합니다. 들어가세요."));
        assert_eq!(meeting_end(&talk(&lines)), None);
    }

    #[test]
    fn another_sites_story_is_not_the_customers() {
        assert!(about_someone_else(
            "현장은 58평 고층이다",
            "저희가 지난달에 한 다른 현장은 58평 고층이었는데",
            false
        ));
        assert!(about_someone_else(
            "현장은 고층이다",
            "우리 집은 20층이라 바람이 세요",
            false
        ));
        assert!(!about_someone_else(
            "다른 현장 사례로 58평에 9000만 원이 들었다",
            "저희가 한 다른 현장은 58평이었는데",
            false
        ));
        // The customer's own home is the site.
        assert!(!about_someone_else(
            "현장은 32평이다",
            "저희 집은 32평이고 3층이에요",
            true
        ));
        assert!(is_customer("여자고객님", ""));
        assert!(is_customer("이서진 고객", "이서진"));
        assert!(!is_customer("김서윤 실장", "이서진"));
    }

    #[test]
    fn settled_and_open_on_the_same_matter_clash() {
        let said = [
            Said {
                section: "decisions",
                text: "방은 4개 그대로 유지하기로 했다",
                ms: 1_000,
            },
            Said {
                section: "materials",
                text: "주방 상판은 화이트로",
                ms: 2_000,
            },
            Said {
                section: "open",
                text: "방을 3개로 할지 4개로 할지 아직 정하지 않았다",
                ms: 9_000,
            },
        ];
        assert_eq!(
            clashes(&said, "open", 10),
            vec![(0, 2, Clash::SettledAndOpen)]
        );
    }

    #[test]
    fn different_numbers_of_a_kind_clash_and_the_same_do_not() {
        let said = [
            Said {
                section: "site",
                text: "현장은 한빛아파트 32평 3층",
                ms: 1_000,
            },
            Said {
                section: "site",
                text: "한빛아파트 현장은 58평",
                ms: 5_000,
            },
            Said {
                section: "budget",
                text: "예산은 4,000만 원",
                ms: 2_000,
            },
            Said {
                section: "budget",
                text: "예산을 5,000만 원으로 올림",
                ms: 8_000,
            },
        ];
        let found = clashes(&said, "open", 10);
        assert!(found.contains(&(0, 1, Clash::Numbers)));
        assert!(found.contains(&(2, 3, Clash::Numbers)));
        assert!(!numbers_disagree("현장은 32평", "32평 아파트 3층"));
    }

    #[test]
    fn a_recording_that_opens_mid_sentence_is_noticed() {
        let mid = talk(&[
            (
                1,
                "그래서 이 벽을 이렇게 옮기면 거실이 훨씬 넓어 보이는 구조가 되는 거고요 수납은",
            ),
            (2, "네"),
        ]);
        assert!(starts_mid_talk(&mid));
        let opened = talk(&[
            (
                1,
                "안녕하세요 오늘은 거실 구조를 보겠습니다 먼저 도면부터 말씀드리면 이렇게",
            ),
            (2, "네"),
        ]);
        assert!(!starts_mid_talk(&opened));
    }

    #[test]
    fn noise_lines_are_told_from_korean_with_a_few_latin_words() {
        #[derive(serde::Deserialize)]
        struct Case {
            text: String,
            gibberish: bool,
        }
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../../../src/seogi/lib/gibberish.cases.json"))
                .unwrap();
        for case in cases {
            assert_eq!(is_gibberish(&case.text), case.gibberish, "{}", case.text);
        }
    }
}
