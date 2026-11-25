//! Spoken numbers written as digits: "이십 초" → "20초", "삼천오백 원" →
//! "3,500원", "오후 세 시 반" → "오후 3시 반".
//!
//! A plain function of the text: no model, nothing made up, well under a
//! millisecond. A number becomes digits only when a unit follows it (초, 분,
//! 원, 명, …), and anything that could just as well be a word is left alone,
//! because a wrong change is worse than none:
//! - a number inside a word is never read ("삼성", "오늘", "구성", "사이");
//! - a number of one syllable ("이", "한", "사", "십") is a word about as
//!   often as it is a number ("이번", "이분", "한 번 보자", "사원", "십분
//!   활용"). It is written as a digit only with a space before a unit known to
//!   be safe with it ("삼 년", "두 시"), or with a reason around it: the
//!   number before it ("11월 이 일", "3시 이 분"), a time of day ("오후 한
//!   시"), or a length of time after it ("이 분 동안", "한 시 반");
//! - digits, English and numbers without a unit (a phone number, "하나 둘")
//!   stay as they are, and so do ranges ("이삼 일", "두세 개").
//!
//! Money keeps the Korean grouping: "오십만 원" → "50만 원", "이억 삼천만 원"
//! → "2억 3,000만 원".

/// Which set of number words a number was said in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// 일 이 삼 … 십 백 천 만 억 조.
    Sino,
    /// 한 두 세 … 열 스물 … 아흔.
    Native,
}

/// A number read from the text.
#[derive(Debug)]
struct Spoken {
    /// Index just past the number's words.
    end: usize,
    value: u64,
    /// The digits after "점", for "삼 점 오".
    fraction: Option<String>,
    kind: Kind,
    /// The number's only syllable, when it is said in one ("이", "한").
    lone: Option<char>,
}

/// A word that counts or measures, after which a number is a number.
struct Unit {
    word: &'static str,
    /// Said with 한 두 세 … too, not only with 일 이 삼 ….
    native: bool,
    /// Thousands get a comma (3,500원), unlike years and numbers that name
    /// something (2026년, 1502호).
    comma: bool,
    /// The one-syllable Sino numbers that are safe before it with a space.
    lone_sino: &'static str,
    /// The one-syllable native numbers that are safe before it with a space.
    lone_native: &'static str,
}

const fn unit(
    word: &'static str,
    native: bool,
    comma: bool,
    lone_sino: &'static str,
    lone_native: &'static str,
) -> Unit {
    Unit {
        word,
        native,
        comma,
        lone_sino,
        lone_native,
    }
}

/// Every unit a number is written before. The one-syllable lists leave out
/// what is a common word: 이 (this) almost everywhere, 사원 · 사번 · 사회,
/// 오일 · 오차, 구분 · 구호 · 구도, 일원 · 일동, 육회, 십분 (fully); 한 and
/// 네 (yes) always need a reason around them.
const UNITS: &[Unit] = &[
    // Measures and counts said only one way, safe with every number.
    unit("퍼센트", false, false, "일이삼사오육칠팔구십백", ""),
    unit("프로", false, false, "일삼사오육칠팔구십백", ""),
    unit("개월", true, false, "일이삼사오육칠팔구십", "두세열"),
    unit("인분", false, true, "일이삼사오육칠팔구십", ""),
    unit("번째", true, false, "일이삼사오육칠팔구십백", "두세열"),
    unit("학년", false, false, "일이삼사오육", ""),
    unit("센티미터", false, true, "일이삼사오육칠팔구십백천", ""),
    unit("밀리미터", false, true, "일이삼사오육칠팔구십백천", ""),
    unit("킬로미터", false, true, "일이삼사오육칠팔구십백천", ""),
    unit("킬로그램", false, true, "일이삼사오육칠팔구십백천", ""),
    unit("미터", false, true, "일이삼사오육칠팔구십백천", ""),
    unit("센티", false, true, "일이삼사오육칠팔구십백천", ""),
    unit("센치", false, true, "일이삼사오육칠팔구십백천", ""),
    unit("밀리", false, true, "일이삼사오육칠팔구십백천", ""),
    unit("킬로", false, true, "일이삼사오육칠팔구십백천", ""),
    unit("키로", false, true, "일이삼사오육칠팔구십백천", ""),
    unit("그램", false, true, "일이삼사오육칠팔구십백천", ""),
    unit("리터", false, true, "일이삼사오육칠팔구십백천", ""),
    unit("페이지", false, false, "일이삼사오육칠팔구십백", ""),
    unit("주일", false, false, "일삼사오육칠팔구십", ""),
    unit("주년", false, false, "일이삼사오육칠팔구십백", ""),
    unit("호선", false, false, "일이삼사오육칠팔구", ""),
    unit("번지", false, false, "삼오육칠팔구십백", ""),
    unit("달러", false, true, "삼오육칠팔십백천", ""),
    unit("세기", false, false, "", ""),
    // Time.
    unit("시간", true, true, "", "두세"),
    unit("초", false, false, "일삼사오육칠팔구십", ""),
    unit("분", false, false, "일삼오육칠팔십", ""),
    unit("시", true, false, "", "두세열"),
    unit("년", false, false, "일삼사오육칠팔구십백천", ""),
    unit("월", false, false, "일삼사오칠팔구", ""),
    unit("일", false, false, "삼사육칠팔구십", ""),
    unit("주", true, false, "삼오육칠팔십", "두세열"),
    // Money, places, order.
    unit("원", false, true, "삼오육칠팔십백천", ""),
    unit("층", false, false, "일삼사오육칠팔구십", ""),
    unit("평", false, true, "일삼사오육칠팔구십", ""),
    unit("동", false, false, "", ""),
    unit("호", false, false, "", ""),
    unit("차", false, false, "일삼사육칠팔구십", ""),
    unit("회", false, false, "일삼오칠팔십", ""),
    unit("번", true, false, "일삼오육칠팔구십백", "두세열"),
    unit("위", false, false, "", ""),
    unit("세", false, false, "", ""),
    unit("점", false, false, "", ""),
    unit("도", false, false, "", ""),
    unit("쪽", false, false, "삼오육칠팔구십백", ""),
    unit("배", true, false, "", "두세열"),
    // Counted things, said with native numbers (and Sino ones from twenty).
    unit("개", true, true, "백천", "두세열쉰"),
    unit("명", true, true, "백천", "두세열쉰"),
    unit("살", true, false, "", "두세열쉰"),
    unit("달", true, false, "", "두세열"),
    unit("대", true, true, "백천", "두세"),
    unit("장", true, true, "백천", "두세열"),
    unit("권", true, true, "백천", "두세열"),
    unit("잔", true, true, "", "두세열"),
    unit("마리", true, true, "백천", "두세열"),
    unit("군데", true, true, "", "두세열"),
    unit("곳", true, true, "", "두세열"),
    unit("가지", true, true, "", "두세열"),
];

/// What may follow a unit in the same word: particles and the like
/// ("20초에", "3,500원짜리"). Anything else means the unit was the start of
/// another word ("오 분석", "삼십 평가").
const PARTICLES: &[&str] = &[
    "이",
    "가",
    "은",
    "는",
    "을",
    "를",
    "의",
    "에",
    "도",
    "만",
    "까지",
    "부터",
    "씩",
    "쯤",
    "째",
    "짜리",
    "와",
    "과",
    "로",
    "으로",
    "입니다",
    "예요",
    "인데",
    "인가",
    "인지",
    "였",
    "간",
    "동안",
    "정도",
    "전",
    "후",
    "뒤",
    "내",
    "경",
    "께",
    "마다",
    "밖에",
    "뿐",
    "대로",
    "랑",
    "하고",
    "니까",
    "면서",
    "무렵",
    "남짓",
    "가량",
    "어치",
    "라서",
];

/// Unit and following letters that make a word instead ("초과", "회의").
const NOT_AFTER: &[(&str, &str)] = &[
    ("초", "과"),
    ("초", "반"),
    ("분", "의"),
    ("차", "이"),
    ("회", "의"),
    ("평", "가"),
    ("주", "가"),
];

/// Letters that may follow particular units in the same word.
const ALSO_AFTER: &[(&str, &[&str])] = &[
    ("년", &["대", "생", "차", "말", "초", "반"]),
    ("월", &["말", "초", "중순"]),
    ("주", &["차"]),
    ("일", &["차"]),
    ("회", &["차"]),
    ("평", &["형"]),
    ("시", &["반"]),
    ("시간", &["반"]),
    ("개월", &["반"]),
    ("달", &["반"]),
];

/// Words just before a number that say it is one: a time of day, "about".
const BEFORE: &[&str] = &[
    "오전", "오후", "새벽", "아침", "저녁", "밤", "낮", "매일", "약", "대략", "총", "최소", "최대",
    "무려",
];

/// Words just after a unit that say the number is one: a length of time,
/// "about", "before", "after".
const AFTER: &[&str] = &[
    "동안", "만에", "이내", "남짓", "가량", "정도", "이상", "이하", "미만", "간", "째", "씩", "쯤",
    "경", "무렵", "전", "후", "뒤", "반",
];

/// The units a number may continue with straight after another, for the
/// second of two numbers said together ("11월 이 일", "3시 이 분").
const CHAINS: &[(&str, &[&str])] = &[
    ("년", &["월", "개월"]),
    ("월", &["일"]),
    ("시", &["분"]),
    ("시간", &["분"]),
    ("분", &["초"]),
    ("동", &["호"]),
    ("층", &["호"]),
];

const TENS: &[(&str, u64)] = &[
    ("스물", 20),
    ("스무", 20),
    ("서른", 30),
    ("마흔", 40),
    ("예순", 60),
    ("일흔", 70),
    ("여든", 80),
    ("아흔", 90),
    ("열", 10),
    ("쉰", 50),
];

const ONES: &[(&str, u64)] = &[
    ("다섯", 5),
    ("여섯", 6),
    ("일곱", 7),
    ("여덟", 8),
    ("아홉", 9),
    ("한", 1),
    ("두", 2),
    ("세", 3),
    ("네", 4),
];

fn is_hangul(c: char) -> bool {
    ('가'..='힣').contains(&c)
}

fn sino_digit(c: char) -> Option<u64> {
    Some(match c {
        '일' => 1,
        '이' => 2,
        '삼' => 3,
        '사' => 4,
        '오' => 5,
        '육' => 6,
        '칠' => 7,
        '팔' => 8,
        '구' => 9,
        _ => return None,
    })
}

fn sino_place(c: char) -> Option<u64> {
    Some(match c {
        '십' => 10,
        '백' => 100,
        '천' => 1_000,
        _ => return None,
    })
}

fn sino_big(c: char) -> Option<u64> {
    Some(match c {
        '만' => 10_000,
        '억' => 100_000_000,
        '조' => 1_000_000_000_000,
        _ => return None,
    })
}

fn is_sino(c: char) -> bool {
    sino_digit(c).is_some() || sino_place(c).is_some() || sino_big(c).is_some()
}

fn starts_with(chars: &[char], at: usize, word: &str) -> bool {
    (at..)
        .zip(word.chars())
        .all(|(i, w)| chars.get(i) == Some(&w))
}

fn len(word: &str) -> usize {
    word.chars().count()
}

/// `at`, or past one space there.
fn past_space(chars: &[char], at: usize) -> usize {
    if chars.get(at) == Some(&' ') {
        at + 1
    } else {
        at
    }
}

/// The longest unit written at `at`.
fn unit_at(chars: &[char], at: usize) -> Option<&'static Unit> {
    UNITS
        .iter()
        .filter(|u| starts_with(chars, at, u.word))
        .max_by_key(|u| len(u.word))
}

/// Reads a Sino number at `start`: "삼천오백", "이억 삼천만", "삼 점 오".
/// With `spaces`, a number may go on past a space where the words plainly
/// continue it ("이천 이십육", "이억 삼천만", "십 오 분").
fn read_sino(chars: &[char], start: usize, spaces: bool) -> Option<Spoken> {
    let mut words: Vec<char> = Vec::new();
    let mut i = start;
    while i < chars.len() {
        let c = chars[i];
        if is_sino(c) {
            words.push(c);
            i += 1;
            continue;
        }
        if spaces && c == ' ' && !words.is_empty() {
            let run_start = i + 1;
            let run_end = (run_start..chars.len())
                .find(|&k| !is_sino(chars[k]))
                .unwrap_or(chars.len());
            let run = &chars[run_start..run_end];
            let last = words[words.len() - 1];
            let has_place = run
                .iter()
                .any(|&c| sino_place(c).is_some() || sino_big(c).is_some());
            let join = !run.is_empty()
                && if sino_big(last).is_some() {
                    has_place
                } else if sino_place(last).is_some() {
                    has_place
                        || (run.len() == 1
                            && sino_digit(run[0]).is_some()
                            && unit_at(chars, past_space(chars, run_end)).is_some())
                } else {
                    false
                };
            if join {
                i += 1;
                continue;
            }
        }
        break;
    }

    // "영 점 오": zero only before a point.
    let zero = words.is_empty() && chars.get(start) == Some(&'영');
    if zero {
        i = start + 1;
    } else if words.is_empty() {
        return None;
    }

    let mut value = 0u64;
    if !zero {
        if words.len() == 1 && sino_big(words[0]).is_some() {
            return None; // "만 원", "조": a word as often as a number.
        }
        let mut group = 0u64;
        let mut pending: Option<u64> = None;
        let mut last_place = u64::MAX;
        let mut last_big = u64::MAX;
        for &c in &words {
            if let Some(d) = sino_digit(c) {
                if pending.is_some() {
                    return None; // "이삼": a range, or not a number.
                }
                pending = Some(d);
            } else if let Some(p) = sino_place(c) {
                if p >= last_place {
                    return None;
                }
                group += pending.take().unwrap_or(1) * p;
                last_place = p;
            } else if let Some(b) = sino_big(c) {
                if b >= last_big {
                    return None;
                }
                let mut g = group + pending.take().unwrap_or(0);
                if g == 0 {
                    if value != 0 {
                        return None;
                    }
                    g = 1; // "만 오천"
                }
                value = value.checked_add(g.checked_mul(b)?)?;
                group = 0;
                last_place = u64::MAX;
                last_big = b;
            }
        }
        value = value.checked_add(group + pending.unwrap_or(0))?;
    }

    // "삼 점 오": digits after a point, read one by one.
    let mut fraction = None;
    if !words.iter().any(|&c| sino_big(c).is_some()) {
        let mut j = past_space(chars, i);
        if chars.get(j) == Some(&'점') {
            j = past_space(chars, j + 1);
            let digits: String = chars[j.min(chars.len())..]
                .iter()
                .map_while(|&c| match c {
                    '영' | '공' => Some('0'),
                    c => sino_digit(c).and_then(|d| char::from_digit(d as u32, 10)),
                })
                .collect();
            if !digits.is_empty() {
                i = j + digits.chars().count();
                fraction = Some(digits);
            }
        }
    }
    if zero && fraction.is_none() {
        return None;
    }

    let lone = (words.len() == 1 && fraction.is_none()).then(|| words[0]);
    Some(Spoken {
        end: i,
        value,
        fraction,
        kind: Kind::Sino,
        lone,
    })
}

/// Reads a native number at `start`: "세", "열두", "스물 다섯".
fn read_native(chars: &[char], start: usize) -> Option<Spoken> {
    let mut i = start;
    let mut value;
    let mut syllables = 0;
    if let Some(&(tens, v)) = TENS.iter().find(|(w, _)| starts_with(chars, start, w)) {
        value = v;
        i += len(tens);
        syllables += len(tens);
        if tens != "스무" {
            let j = past_space(chars, i);
            if let Some(&(ones, v)) = ONES.iter().find(|(w, _)| starts_with(chars, j, w)) {
                let k = j + len(ones);
                // "열 두 시" goes on only when a unit follows.
                if j == i || unit_at(chars, past_space(chars, k)).is_some() {
                    value += v;
                    i = k;
                    syllables += len(ones);
                }
            }
        }
    } else if let Some(&(ones, v)) = ONES.iter().find(|(w, _)| starts_with(chars, start, w)) {
        value = v;
        i += len(ones);
        syllables += len(ones);
    } else {
        return None;
    }
    Some(Spoken {
        end: i,
        value,
        fraction: None,
        kind: Kind::Native,
        lone: (syllables == 1).then(|| chars[start]),
    })
}

/// Whether what follows the unit at `at` keeps it a unit: the end of the
/// word, or a particle.
fn ends_well(unit: &Unit, chars: &[char], at: usize) -> bool {
    let rest: String = chars[at..].iter().take_while(|&&c| is_hangul(c)).collect();
    if rest.is_empty() {
        return true;
    }
    if rest.starts_with("요일") {
        return false;
    }
    if NOT_AFTER
        .iter()
        .any(|(u, after)| *u == unit.word && rest.starts_with(after))
    {
        return false;
    }
    let also = ALSO_AFTER
        .iter()
        .find(|(u, _)| *u == unit.word)
        .map(|(_, list)| *list)
        .unwrap_or(&[]);
    also.iter().any(|a| rest.starts_with(a))
        || PARTICLES.iter().any(|p| rest.starts_with(p))
        || rest == "요"
        || rest == "죠"
}

/// The word that ends at `end` (exclusive), back to the space before it.
fn word_before(chars: &[char], end: usize) -> &[char] {
    let begin = chars[..end]
        .iter()
        .rposition(|c| c.is_whitespace())
        .map_or(0, |p| p + 1);
    &chars[begin..end]
}

/// The word after a unit ending at `at` starts with one of `words`: attached
/// ("삼 년간") or after one space ("이 분 동안").
fn followed_by(chars: &[char], at: usize, words: &[&str]) -> bool {
    let check = |from: usize| {
        words.iter().any(|w| {
            if !starts_with(chars, from, w) {
                return false;
            }
            // A one-letter word must stand alone or take a particle:
            // "전에", not "전체".
            let next = chars.get(from + len(w)).copied();
            len(w) > 1
                || next.is_none_or(|c| !is_hangul(c))
                || matches!(
                    next,
                    Some('에' | '이' | '은' | '는' | '도' | '로' | '부' | '까' | '으' | '만')
                )
        })
    };
    check(at) || (chars.get(at) == Some(&' ') && check(at + 1))
}

/// Around a number, what says it is one.
struct Around {
    /// The number before it goes with it ("11월 이 일").
    chain: bool,
    /// "오후", "약" just before it.
    before: bool,
    /// "동안", "반", "전" just after the unit.
    after: bool,
    /// "동안" or "만에" just after the unit: a length of time.
    duration: bool,
    /// After "시", minutes ("한 시 삼십 분").
    minutes_next: bool,
}

fn around(
    chars: &[char],
    start: usize,
    unit: &Unit,
    unit_end: usize,
    last: Option<(usize, &str)>,
) -> Around {
    let before_end = if start > 0 && chars[start - 1] == ' ' {
        start - 1
    } else {
        start
    };
    let prev = word_before(chars, before_end);
    // The unit of the number just before: one this function wrote, or
    // digits already in the text ("3시").
    let prev_unit: Option<String> = match last {
        Some((end, word)) if end == before_end => Some(word.to_string()),
        _ if prev.first().is_some_and(|c| c.is_ascii_digit()) => {
            let tail: String = prev
                .iter()
                .rev()
                .take_while(|c| is_hangul(**c))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            Some(tail)
        }
        _ => None,
    };
    let chain = prev_unit.is_some_and(|p| {
        CHAINS
            .iter()
            .any(|(from, to)| *from == p && to.contains(&unit.word))
    });
    let prev_word: String = prev.iter().collect();
    let minutes_next = unit.word == "시" && {
        let at = past_space(chars, unit_end);
        read_sino(chars, at, true).is_some_and(|n| {
            unit_at(chars, past_space(chars, n.end)).is_some_and(|u| u.word == "분")
        })
    };
    Around {
        chain,
        before: BEFORE.contains(&prev_word.as_str()),
        after: followed_by(chars, unit_end, AFTER),
        duration: followed_by(chars, unit_end, &["동안", "만에"]),
        minutes_next,
    }
}

/// Whether a number may be written as digits before this unit.
fn allowed(number: &Spoken, unit: &Unit, spaced: bool, around: &Around) -> bool {
    if number.kind == Kind::Native && !unit.native {
        return false;
    }
    let Some(lone) = number.lone else {
        return true; // Two syllables or more: "이십", "열두", "다섯".
    };
    match (lone, unit.word) {
        ('한', "번") => return false,               // "한 번 보자"
        ('이', "번" | "일") => return around.chain, // "이번", "이 일은"
        ('일', "주일") if !spaced => return false,  // "일주일" is the word for a week
        // "한 달 뒤", "한 명도": 한 reads as a digit only for the hour.
        ('한', unit) if unit != "시" && unit != "시간" => return false,
        _ => {}
    }
    let safe = match number.kind {
        Kind::Sino => unit.lone_sino.contains(lone),
        Kind::Native => unit.lone_native.contains(lone),
    };
    if (safe && spaced) || around.chain {
        return true;
    }
    if lone == '이' {
        return around.duration;
    }
    around.before || around.after || around.minutes_next
}

/// `n` with a comma every three digits when `comma`.
fn grouped(n: u64, comma: bool) -> String {
    let plain = n.to_string();
    if !comma || plain.len() <= 3 {
        return plain;
    }
    let mut out = String::new();
    for (i, c) in plain.chars().enumerate() {
        if i > 0 && (plain.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The number as written, and whether it ends in 만/억/조 (then the unit
/// takes a space: "50만 원").
fn written(number: &Spoken, comma: bool) -> (String, bool) {
    if let Some(fraction) = &number.fraction {
        return (format!("{}.{}", number.value, fraction), false);
    }
    let value = number.value;
    if value < 10_000 {
        return (grouped(value, comma), false);
    }
    let mut parts = Vec::new();
    for (size, word) in [
        (1_000_000_000_000u64, "조"),
        (100_000_000, "억"),
        (10_000, "만"),
    ] {
        let part = (value / size) % 10_000;
        if part > 0 || (word == "조" && value / size > 0) {
            parts.push(format!("{}{}", grouped(value / size % 10_000, true), word));
        }
    }
    let rest = value % 10_000;
    if rest > 0 {
        parts.push(grouped(rest, comma));
    }
    (parts.join(" "), rest == 0)
}

/// The text with spoken numbers that come before a unit written as digits.
pub fn spoken_to_digits(text: &str) -> String {
    if !text
        .chars()
        .any(|c| is_sino(c) || "한두세네다여일열스서마쉰예아유시영".contains(c))
    {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut last: Option<(usize, &'static str)> = None;
    let mut i = 0;
    while i < chars.len() {
        let word_start = i == 0 || !(is_hangul(chars[i - 1]) || chars[i - 1].is_alphanumeric());
        if word_start {
            if let Some((written, end, unit)) = convert_at(&chars, i, last) {
                out.push_str(&written);
                last = Some((end, unit));
                i = end;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// The number and unit at `start` written with digits, the index past the
/// unit, and the unit.
fn convert_at(
    chars: &[char],
    start: usize,
    last: Option<(usize, &'static str)>,
) -> Option<(String, usize, &'static str)> {
    // 유월 and 시월 are how 6월 and 10월 are said.
    for (word, month) in [("유월", "6월"), ("시월", "10월")] {
        if starts_with(chars, start, word) {
            let end = start + 2;
            let month_unit = unit_at(&['월'], 0)?;
            if ends_well(month_unit, chars, end) {
                return Some((month.to_string(), end, "월"));
            }
        }
    }
    let candidates = [
        read_native(chars, start),
        read_sino(chars, start, true),
        read_sino(chars, start, false),
    ];
    for number in candidates.into_iter().flatten() {
        let at = past_space(chars, number.end);
        let spaced = at != number.end;
        let Some(unit) = unit_at(chars, at) else {
            continue;
        };
        let unit_end = at + len(unit.word);
        if !ends_well(unit, chars, unit_end) {
            continue;
        }
        let context = around(chars, start, unit, unit_end, last);
        if !allowed(&number, unit, spaced, &context) {
            continue;
        }
        let (digits, ends_big) = written(&number, unit.comma);
        let unit_text = match unit.word {
            "퍼센트" | "프로" => "%",
            word => word,
        };
        let gap = if ends_big { " " } else { "" };
        return Some((format!("{digits}{gap}{unit_text}"), unit_end, unit.word));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::spoken_to_digits as d;

    #[test]
    fn writes_numbers_before_units_as_digits() {
        let cases = [
            (
                "이십 초 동안 녹음을 하는 중입니다",
                "20초 동안 녹음을 하는 중입니다",
            ),
            ("삼천오백 원이에요", "3,500원이에요"),
            ("오십만 원 정도 들어요", "50만 원 정도 들어요"),
            ("천이백만 원으로 맞춰 주세요", "1,200만 원으로 맞춰 주세요"),
            ("이억 삼천만 원짜리 집", "2억 3,000만 원짜리 집"),
            ("만 오천 원만 주세요", "1만 5,000원만 주세요"),
            (
                "이천이십육 년 십일 월 이십오 일에 만나요",
                "2026년 11월 25일에 만나요",
            ),
            ("오후 세 시 반에 봬요", "오후 3시 반에 봬요"),
            ("세 시 삼십 분까지 오세요", "3시 30분까지 오세요"),
            ("한 시 반에 시작해요", "1시 반에 시작해요"),
            ("오후 한 시에 점심", "오후 1시에 점심"),
            ("열두 시에 닫아요", "12시에 닫아요"),
            ("열한 시 사십오 분 기차", "11시 45분 기차"),
            ("다섯 명이 와요", "5명이 와요"),
            ("사과 두 개만 주세요", "사과 2개만 주세요"),
            ("세 번이나 말했어요", "3번이나 말했어요"),
            ("스물다섯 살이에요", "25살이에요"),
            ("스물 다섯 살이에요", "25살이에요"),
            ("서른 명 정도 모였어요", "30명 정도 모였어요"),
            ("삼십 퍼센트 할인", "30% 할인"),
            ("이십 프로가 남았어요", "20%가 남았어요"),
            ("삼 점 오 퍼센트 올랐어요", "3.5% 올랐어요"),
            ("영 점 오 킬로 줄었어요", "0.5킬로 줄었어요"),
            ("삼 년 동안 다녔어요", "3년 동안 다녔어요"),
            ("일 년에 한 번", "1년에 한 번"),
            ("삼십이 평 아파트", "32평 아파트"),
            ("백삼 동 천오백이 호", "103동 1502호"),
            ("이십 층에 살아요", "20층에 살아요"),
            ("십오 분 뒤에 출발", "15분 뒤에 출발"),
            ("십 오 분 걸려요", "15분 걸려요"),
            ("이 분 동안 기다려 주세요", "2분 동안 기다려 주세요"),
            ("십일 월 이 일까지", "11월 2일까지"),
            ("3시 이 분에", "3시 2분에"),
            ("이천 이십육 년", "2026년"),
            ("삼백 오십 명", "350명"),
            ("천 원짜리", "1,000원짜리"),
            ("이십 미터 남았어요", "20미터 남았어요"),
            ("이천삼백 밀리라서", "2,300밀리라서"),
            ("구십 년대 노래", "90년대 노래"),
            ("이 호선 타세요", "2호선 타세요"),
            (
                "유월에 시작해서 시월에 끝나요",
                "6월에 시작해서 10월에 끝나요",
            ),
            ("사 개월 걸려요", "4개월 걸려요"),
            ("두 시간 걸렸어요", "2시간 걸렸어요"),
            ("한 시간 동안 했어요", "1시간 동안 했어요"),
            ("오후 네 시에 회의", "오후 4시에 회의"),
            ("열 명이 넘어요", "10명이 넘어요"),
            ("백 명이 왔어요", "100명이 왔어요"),
            ("이십 대 후반", "20대 후반"),
            ("오십 달러예요", "50달러예요"),
            ("세 시경에 갈게요", "3시경에 갈게요"),
            ("삼 일째 비가 와요", "3일째 비가 와요"),
            ("두 배로 늘었어요", "2배로 늘었어요"),
            ("삼십 도가 넘어요", "30도가 넘어요"),
            ("(이십 초)", "(20초)"),
            ("이 주 동안 쉬어요", "2주 동안 쉬어요"),
        ];
        let wrong: Vec<String> = cases
            .iter()
            .filter(|(said, want)| d(said) != *want)
            .map(|(said, want)| format!("{said} -> {} (want {want})", d(said)))
            .collect();
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn leaves_words_that_only_look_like_numbers() {
        let cases = [
            "이번 주에 보내 드릴게요",
            "이분이 담당자세요",
            "일요일에 쉬어요",
            "삼성 제품이에요",
            "그 사이에 끝내요",
            "오늘 회의는 짧아요",
            "구성이 좋네요",
            "이 일은 제가 할게요",
            "한 번 보자",
            "한 번 해 볼게요",
            "이 분야는 처음이에요",
            "이 분 누구세요",
            "사원 교육이 있어요",
            "사번을 알려 주세요",
            "오차가 커요",
            "구분해서 정리해요",
            "십분 활용하세요",
            "일원으로서 책임",
            "육회 먹으러 가요",
            "오 분석 결과가 나왔어요",
            "삼십 평가 기준",
            "삼십 초과는 안 돼요",
            "삼분의 일만 했어요",
            "이삼 일 걸려요",
            "두세 개 주세요",
            "만 원만 주세요",
            "네 시간 괜찮아요",
            "네 명이요",
            "한 명도 없어요",
            "한 달 뒤에 봐요 그리고",
            "열대 지방",
            "세대 차이가 나요",
            "천만에요",
            "이층 침대",
            "일년 내내",
            "사이좋게 지내요",
            "2026년 11월 25일",
            "3,500원",
            "02 123 4567",
            "공일공 일이삼사",
            "하나 둘 셋",
            "meeting at five",
            "이 차 누구 거예요",
            "삼일절",
            "회의 시작해요",
            "다섯 분이 오셨어요",
            "서너 명",
            "오십 차이 나요",
            "이 점은 중요해요",
            "일주일 전까지 보내 주세요",
        ];
        let wrong: Vec<String> = cases
            .iter()
            .filter(|said| d(said) != **said)
            .map(|said| format!("{said} -> {}", d(said)))
            .collect();
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn plain_text_is_returned_as_is() {
        assert_eq!(d(""), "");
        assert_eq!(d("Hello"), "Hello");
        assert_eq!(d("가나다"), "가나다");
    }
}
