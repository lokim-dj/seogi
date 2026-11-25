//! Spoken numbers written as digits: "이십 초" → "20초", "삼천오백 원" →
//! "3,500원", "오후 세 시 반" → "오후 3시 반".
//!
//! A plain function of the text: no model, nothing made up, linear in the
//! length of the text. It is deliberately timid, because a wrong change is
//! worse than none, and Korean number words are ordinary words just as often
//! ("조사", "천장", "세대", "이번", "십분 활용", "페인트 칠"):
//! - a number is only read as whole words of number syllables, starting a
//!   word, followed by a unit that starts the next word ("이십 초"); a unit
//!   written onto the number ("이십초") counts only for numbers of two
//!   syllables or more, and never "도" ("오십도 안 남았어요");
//! - after the unit only the end of the word, or a short particle and the
//!   end of the word, may follow ("20초에", not "2대도시");
//! - number-looking words are refused by a list ("천사", "백일", "일조",
//!   "구조", "오만"), and values a unit cannot take are refused ("이천 시");
//! - a number of one syllable ("두", "삼", "이") needs a unit known to be
//!   safe with it ("두 개", "삼 년") or a reason around it: the number before
//!   it ("11월 이 일"), a time of day ("오후 한 시"), a half or minutes after
//!   it ("한 시 반"), or a length of time ("오 분 뒤"). "이" (this) and "한",
//!   "네" (yes) take almost nothing;
//! - ranges and rough numbers are left whole ("몇 백 명", "두 세 명", "삼 사
//!   일", "2 천 원"), and so are digits, English and numbers without a unit.
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
    value: u64,
    /// The digits after "점", for "삼 점 오".
    fraction: Option<String>,
    kind: Kind,
    /// The number's only syllable, when it is said in one ("이", "한").
    lone: Option<char>,
    /// The first word of the number, for the list of words that only look
    /// like numbers.
    first: String,
}

/// A word that counts or measures, after which a number is a number.
struct Unit {
    word: &'static str,
    /// Said with 한 두 세 … too, not only with 일 이 삼 ….
    native: bool,
    /// Counts and amounts: thousands get a comma (3,500원) and a number may
    /// run to 만 and beyond. Years and numbers that name something do not
    /// (2026년, 1502호).
    amount: bool,
    /// One-syllable Sino numbers that are safe before it with a space.
    lone_sino: &'static str,
    /// One-syllable native numbers that are safe before it with a space.
    lone_native: &'static str,
    /// The largest value it sensibly takes ("이천 시" is no time).
    max: u64,
    /// A measure, which may have a decimal point.
    measure: bool,
}

const ANY: u64 = u64::MAX;
const ALL_SINO: &str = "일삼사오육칠팔구십백천";

const fn unit(
    word: &'static str,
    native: bool,
    amount: bool,
    lone_sino: &'static str,
    lone_native: &'static str,
    max: u64,
    measure: bool,
) -> Unit {
    Unit {
        word,
        native,
        amount,
        lone_sino,
        lone_native,
        max,
        measure,
    }
}

/// Every unit a number is written before. The one-syllable lists leave out
/// what makes a common phrase: 이 (this) everywhere, 칠 (paint) and 팔 (arm)
/// before time and sides, 열 (open) before 시, and 한 and 네 (yes) always.
/// "프로" is left out: after a product name it is part of the name.
const UNITS: &[Unit] = &[
    unit(
        "퍼센트",
        false,
        false,
        "일이삼사오육칠팔구십백",
        "",
        ANY,
        true,
    ),
    unit(
        "개월",
        true,
        false,
        "일이삼사오육칠팔구십",
        "두세",
        1200,
        false,
    ),
    unit("인분", false, true, "일이삼사오육칠팔구십", "", ANY, false),
    unit("주년", false, false, "일삼사오육칠팔구십백", "", ANY, false),
    unit("학년", false, false, "일삼사오육", "", 6, false),
    unit("센티미터", false, true, ALL_SINO, "", ANY, true),
    unit("밀리미터", false, true, ALL_SINO, "", ANY, true),
    unit("킬로미터", false, true, ALL_SINO, "", ANY, true),
    unit("킬로그램", false, true, ALL_SINO, "", ANY, true),
    unit("미터", false, true, ALL_SINO, "", ANY, true),
    unit("센티", false, true, ALL_SINO, "", ANY, true),
    unit("센치", false, true, ALL_SINO, "", ANY, true),
    unit("밀리", false, true, ALL_SINO, "", ANY, true),
    unit("킬로", false, true, ALL_SINO, "", ANY, true),
    unit("키로", false, true, ALL_SINO, "", ANY, true),
    unit("그램", false, true, ALL_SINO, "", ANY, true),
    unit("리터", false, true, ALL_SINO, "", ANY, true),
    unit("페이지", false, false, "", "", ANY, false),
    unit("주일", false, false, "", "", ANY, false),
    unit("호선", false, false, "일삼사오육칠팔구", "", 20, false),
    unit("번지", false, false, "", "", ANY, false),
    unit("번째", false, false, "", "", ANY, false),
    unit("달러", false, true, "십백천", "", ANY, true),
    unit("세기", false, false, "", "", 30, false),
    unit("시간", true, true, "", "두세", ANY, true),
    unit("초", false, false, "", "", ANY, true),
    unit("분", false, false, "", "", ANY, false),
    unit("시", true, false, "", "두세", 24, false),
    unit("년", false, false, ALL_SINO, "", ANY, false),
    unit("월", false, false, "", "", 12, false),
    unit("일", false, false, "", "", 366, false),
    unit("주", true, false, "", "", ANY, false),
    unit("원", false, true, "십백천", "", ANY, false),
    unit("층", false, false, "일삼사오육십", "", 300, false),
    unit("평", false, true, "", "", ANY, true),
    unit("동", false, false, "", "", 9999, false),
    unit("호", false, false, "", "", 9999, false),
    unit("차", false, false, "", "", ANY, false),
    unit("회", false, false, "", "", ANY, false),
    unit("번", true, false, "", "", ANY, false),
    unit("위", false, false, "", "", ANY, false),
    unit("세", false, false, "", "", 150, false),
    unit("점", false, false, "", "", ANY, false),
    unit("도", false, false, "", "", ANY, true),
    unit("쪽", false, false, "", "", ANY, false),
    unit("배", true, false, "", "두세", ANY, true),
    unit("개", true, true, "백천", "두세열", ANY, false),
    unit("명", true, true, "백천", "두세열", ANY, false),
    unit("살", true, false, "", "두세열쉰", 150, false),
    unit("달", true, false, "", "", ANY, false),
    unit("대", true, true, "", "두세", ANY, false),
    unit("장", true, true, "", "두세열", ANY, false),
    unit("권", true, true, "", "두세열", ANY, false),
    unit("잔", true, true, "", "두세열", ANY, false),
    unit("마리", true, true, "", "두세열", ANY, false),
    unit("군데", true, true, "", "두세열", ANY, false),
    unit("곳", true, true, "", "두세열", ANY, false),
    unit("가지", true, true, "", "두세열", ANY, false),
];

/// What may follow a unit in its word, as the whole rest of the word.
const PARTICLES: &[&str] = &[
    "이",
    "가",
    "은",
    "는",
    "을",
    "를",
    "의",
    "에",
    "에서",
    "에는",
    "에도",
    "에서는",
    "에게",
    "도",
    "만",
    "만에",
    "까지",
    "까지는",
    "부터",
    "부터는",
    "씩",
    "쯤",
    "째",
    "짜리",
    "와",
    "과",
    "로",
    "으로",
    "으로는",
    "이나",
    "이면",
    "이고",
    "이라",
    "이라서",
    "이라고",
    "이요",
    "요",
    "입니다",
    "이에요",
    "예요",
    "인데",
    "이죠",
    "죠",
    "이야",
    "이랑",
    "랑",
    "하고",
    "정도",
    "간",
    "경",
    "께",
    "마다",
    "밖에",
    "뿐",
    "어치",
    "라서",
    "이니까",
    "니까",
    "이었어요",
    "였어요",
    "이었는데",
    "였는데",
    "까지요",
    "부터요",
    "이서",
    "에요",
];

/// Endings particular units take before a particle ("90년대에", "3주차").
const UNIT_ENDINGS: &[(&str, &[&str])] = &[
    ("년", &["대", "생", "차", "말", "초", "반", "도"]),
    ("월", &["말", "초", "중순"]),
    ("주", &["차"]),
    ("일", &["차"]),
    ("회", &["차"]),
    ("평", &["형"]),
    ("시", &["반"]),
    ("시간", &["반"]),
    ("개월", &["반"]),
];

/// A unit and what follows it that make another word ("시도", "세로").
const UNIT_WORDS: &[&str] = &[
    "시도", "시가", "세로", "세가", "세대", "도로", "분과", "분만", "차이", "차도", "차로", "회의",
    "회가", "평가", "평이", "주가", "주요", "주의", "주도", "호가", "위도", "점도", "명도", "대가",
    "대로", "장도",
];

/// Words that read as numbers but are words: never a number, or only before
/// the units listed.
const NUMBER_WORDS: &[(&str, &[&str])] = &[
    ("천사", &[]),
    ("백사", &[]),
    ("백일", &[]),
    ("일조", &[]),
    ("이조", &[]),
    ("구조", &[]),
    ("일천", &[]),
    ("오만", &["원"]),
    ("이만", &["원"]),
    ("만", &["원", "명", "개"]),
    ("천만", &["원", "명"]),
    ("백만", &["원", "명"]),
    ("이천", &["원", "년"]),
];

/// Words before a number that make it part of a range or a rough number
/// ("몇 백 명", "두 세 명", "수 백 명").
const RANGE_BEFORE: &[&str] = &[
    "몇",
    "수",
    "한",
    "두",
    "세",
    "네",
    "서너",
    "너덧",
    "너댓",
    "여러",
    "대여섯",
    "예닐곱",
];

/// Times of day, before a number of o'clock ("오후 한 시").
const TIMES_OF_DAY: &[&str] = &["오전", "오후", "새벽", "아침", "저녁", "밤", "낮"];

/// Words after a length of time that say it is one ("오 분 뒤").
const DURATION_AFTER: &[&str] = &[
    "동안",
    "동안에",
    "동안은",
    "뒤",
    "뒤에",
    "뒤에는",
    "후",
    "후에",
    "후에는",
    "전",
    "전에",
    "전에는",
    "만에",
    "이내",
    "이내에",
    "째",
];

/// The units a length of time is counted in.
const DURATION_UNITS: &[&str] = &["시간", "분", "초", "일", "주", "개월", "년", "달"];

/// Words after an hour that say it is one ("한 시 반").
const HALF_AFTER: &[&str] = &[
    "반",
    "반에",
    "반까지",
    "반부터",
    "반쯤",
    "반이에요",
    "반입니다",
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

/// At most this many words make one number ("이억 삼천만"), which keeps the
/// reading linear.
const MAX_NUMBER_WORDS: usize = 4;

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

fn is_place_or_big(c: char) -> bool {
    sino_place(c).is_some() || sino_big(c).is_some()
}

/// The value of Sino number syllables, if they make one number ("삼천오백",
/// "이억삼천만"). Not "이삼" (a range), not "만일" or "조사" (a 만 or 조 with
/// nothing to count).
fn sino_value(words: &[char]) -> Option<u64> {
    if words.is_empty() {
        return None;
    }
    let mut value = 0u64;
    let mut group = 0u64;
    let mut pending: Option<u64> = None;
    let mut last_place = u64::MAX;
    let mut last_big = u64::MAX;
    for (i, &c) in words.iter().enumerate() {
        if let Some(d) = sino_digit(c) {
            if pending.is_some() {
                return None;
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
                // Only "만 오천": a leading 만 followed by a place.
                let more = &words[i + 1..];
                if i != 0 || b != 10_000 || !more.iter().any(|&c| sino_place(c).is_some()) {
                    return None;
                }
                g = 1;
            }
            value = value.checked_add(g.checked_mul(b)?)?;
            group = 0;
            last_place = u64::MAX;
            last_big = b;
        }
    }
    value.checked_add(group + pending.unwrap_or(0))
}

/// A native number at the start of `s`: its value, how many letters it
/// takes and how many syllables. "스물다섯", "열두", "세".
fn native_prefix(s: &[char]) -> Option<(u64, usize)> {
    let starts = |at: usize, w: &str| {
        let w: Vec<char> = w.chars().collect();
        s.len() >= at + w.len() && s[at..at + w.len()] == w[..]
    };
    let mut value = 0;
    let mut at = 0;
    if let Some(&(tens, v)) = TENS.iter().find(|(w, _)| starts(0, w)) {
        value = v;
        at = tens.chars().count();
        if tens == "스무" {
            return Some((value, at));
        }
    }
    if let Some(&(ones, v)) = ONES.iter().find(|(w, _)| starts(at, w)) {
        value += v;
        at += ones.chars().count();
    }
    (at > 0).then_some((value, at))
}

/// A native number that is the whole of `s`.
fn native_whole(s: &[char]) -> Option<u64> {
    native_prefix(s).and_then(|(v, n)| (n == s.len()).then_some(v))
}

/// Whether `s` is a number said in words, of either kind, or digits.
fn is_number_word(s: &[char]) -> bool {
    if s.is_empty() {
        return false;
    }
    let digits = s
        .iter()
        .all(|c| c.is_ascii_digit() || *c == ',' || *c == '.');
    digits || native_whole(s).is_some() || s.iter().all(|&c| is_sino(c) || c == '영' || c == '공')
}

/// One whitespace-separated word of the text.
struct Word {
    /// Index of its first letter after any opening punctuation.
    core: usize,
    /// Index just past it.
    end: usize,
    /// It ends in punctuation ("세,"), which cuts it off from the next word.
    closed: bool,
}

fn split_words(chars: &[char]) -> Vec<Word> {
    let mut words = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let core = (start..i)
            .find(|&k| chars[k].is_alphanumeric())
            .unwrap_or(i);
        let closed = !chars[i - 1].is_alphanumeric();
        words.push(Word {
            core,
            end: i,
            closed,
        });
    }
    words
}

/// The letters of a word without its punctuation before or after.
fn letters<'a>(chars: &'a [char], word: &Word) -> &'a [char] {
    let mut end = word.end;
    while end > word.core && !chars[end - 1].is_alphanumeric() {
        end -= 1;
    }
    &chars[word.core..end]
}

/// The unit that `s` starts with, longest first, and the rest of `s` after
/// it, when the rest is the end of the word or a particle.
fn unit_then_end(s: &[char]) -> Option<(&'static Unit, usize)> {
    let mut best: Option<(&'static Unit, usize)> = None;
    for unit in UNITS {
        let w: Vec<char> = unit.word.chars().collect();
        if s.len() >= w.len()
            && s[..w.len()] == w[..]
            && best.is_none_or(|(b, _)| b.word.chars().count() < w.len())
        {
            best = Some((unit, w.len()));
        }
    }
    let (unit, n) = best?;
    let rest: String = s[n..].iter().collect();
    let whole: String = s.iter().collect();
    if UNIT_WORDS.contains(&whole.as_str()) {
        return None;
    }
    if rest.is_empty() || PARTICLES.contains(&rest.as_str()) {
        return Some((unit, n));
    }
    let endings = UNIT_ENDINGS
        .iter()
        .find(|(u, _)| *u == unit.word)
        .map_or(&[][..], |(_, e)| *e);
    for ending in endings {
        if let Some(after) = rest.strip_prefix(ending) {
            if after.is_empty() || PARTICLES.contains(&after) {
                return Some((unit, n));
            }
        }
    }
    None
}

/// The text with spoken numbers that come before a unit written as digits.
pub fn spoken_to_digits(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let words = split_words(&chars);
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    // The unit of the last number written, and the word it is in.
    let mut last: Option<(usize, &'static str)> = None;
    let mut i = 0;
    while i < words.len() {
        match convert_at(&chars, &words, i, last) {
            Some(found) => {
                edits.push((found.start, found.end, found.text));
                last = Some((found.unit_word, found.unit));
                i = found.unit_word + 1;
            }
            None => i += 1,
        }
    }
    if edits.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (start, end, written) in edits {
        out.extend(&chars[at..start]);
        out.push_str(&written);
        at = end;
    }
    out.extend(&chars[at..]);
    out
}

/// A number and unit found in the text, written with digits.
struct Found {
    start: usize,
    end: usize,
    text: String,
    /// The index of the word the unit is in.
    unit_word: usize,
    unit: &'static str,
}

fn convert_at(
    chars: &[char],
    words: &[Word],
    i: usize,
    last: Option<(usize, &'static str)>,
) -> Option<Found> {
    let word = letters(chars, &words[i]);
    if word.is_empty() || !is_hangul(word[0]) {
        return None;
    }
    // A number right after another number word, "몇" or "수" is part of a
    // range or a rough number: leave the lot.
    let prev = (i > 0 && !words[i - 1].closed).then(|| letters(chars, &words[i - 1]));
    if let Some(prev) = prev {
        let p: String = prev.iter().collect();
        if RANGE_BEFORE.contains(&p.as_str()) || is_number_word(prev) {
            return None;
        }
    }

    // Spoken in words that stand alone, then the unit in the next word.
    if let Some(found) = spaced(chars, words, i, last) {
        return Some(found);
    }
    // A number of two syllables or more with its unit written onto it.
    attached(chars, words, i)
}

/// "이십 초", "이억 삼천만 원", "삼 점 오 퍼센트", "스물 다섯 살".
fn spaced(
    chars: &[char],
    words: &[Word],
    i: usize,
    last: Option<(usize, &'static str)>,
) -> Option<Found> {
    let first = letters(chars, &words[i]);
    let open = |k: usize| k < words.len() && !words[k - 1].closed;

    let mut candidates: Vec<(Spoken, usize)> = Vec::new();
    // Native: one word, or a ten and a one apart ("스물 다섯").
    if let Some(v) = native_whole(first) {
        let mut value = v;
        let mut next = i + 1;
        let is_tens = TENS
            .iter()
            .any(|(w, _)| *w != "스무" && w.chars().eq(first.iter().copied()));
        if is_tens && open(next) {
            let following = letters(chars, &words[next]);
            if let Some(&(_, ones)) = ONES
                .iter()
                .find(|(w, _)| w.chars().eq(following.iter().copied()))
            {
                value += ones;
                next += 1;
            }
        }
        let syllables: usize = (i..next).map(|k| letters(chars, &words[k]).len()).sum();
        candidates.push((
            Spoken {
                value,
                fraction: None,
                kind: Kind::Native,
                lone: (syllables == 1).then(|| first[0]),
                first: first.iter().collect(),
            },
            next,
        ));
    }
    // Sino: words of number syllables, joined where they plainly go on
    // ("이천 이십육", "이억 삼천만").
    if first.iter().all(|&c| is_sino(c)) || first == ['영'] {
        let mut syllables: Vec<char> = first.to_vec();
        let mut next = i + 1;
        while next - i < MAX_NUMBER_WORDS && open(next) && first != ['영'] {
            let following = letters(chars, &words[next]);
            let joins = syllables.last().is_some_and(|&c| is_place_or_big(c))
                && !following.is_empty()
                && following.iter().all(|&c| is_sino(c))
                && following.iter().any(|&c| is_place_or_big(c));
            if !joins {
                break;
            }
            syllables.extend_from_slice(following);
            next += 1;
        }
        let zero = first == ['영'];
        let value = if zero {
            Some(0)
        } else {
            sino_value(&syllables)
        };
        if let Some(value) = value {
            // "삼 점 오": the point and its digits as words of their own.
            let mut fraction = None;
            let mut after = next;
            if open(after + 1) && letters(chars, &words[after]) == ['점'] {
                let digits = letters(chars, &words[after + 1]);
                let read: Option<String> = digits
                    .iter()
                    .map(|&c| match c {
                        '영' | '공' => Some('0'),
                        c => sino_digit(c).and_then(|d| char::from_digit(d as u32, 10)),
                    })
                    .collect();
                if let Some(read) = read.filter(|r| !r.is_empty()) {
                    fraction = Some(read);
                    after += 2;
                }
            }
            if !zero || fraction.is_some() {
                let lone = (syllables.len() == 1 && fraction.is_none()).then(|| syllables[0]);
                candidates.push((
                    Spoken {
                        value,
                        fraction,
                        kind: Kind::Sino,
                        lone,
                        first: first.iter().collect(),
                    },
                    after,
                ));
            }
        }
    }

    for (number, unit_word) in candidates {
        if !open(unit_word) {
            continue;
        }
        let unit_letters = letters(chars, &words[unit_word]);
        let Some((unit, unit_len)) = unit_then_end(unit_letters) else {
            continue;
        };
        if !fits(&number, unit) {
            continue;
        }
        // "삼십 일 차" (thirtieth day, or 31st?) and "오십 대 오십" (a ratio):
        // what follows makes the reading unsure.
        let unit_alone = unit_len == unit_letters.len();
        if unit_alone && open(unit_word + 1) {
            let next = letters(chars, &words[unit_word + 1]);
            if UNITS
                .iter()
                .any(|u| u.word.chars().eq(next.iter().copied()))
            {
                continue;
            }
            if matches!(unit.word, "대" | "점") && is_number_word(next) {
                continue;
            }
        }
        let around = Around::read(chars, words, i, unit_word, unit, last, unit_alone);
        if !allowed(&number, unit, &around) {
            continue;
        }
        let start = words[i].core;
        let end = words[unit_word].core + unit_len;
        return Some(Found {
            start,
            end,
            text: written(&number, unit),
            unit_word,
            unit: unit.word,
        });
    }
    None
}

/// "이십초", "삼천오백원이에요", "다섯시": two syllables or more, then the
/// unit and the end of the word.
fn attached(chars: &[char], words: &[Word], i: usize) -> Option<Found> {
    let word = letters(chars, &words[i]);
    let mut candidates: Vec<(Spoken, usize)> = Vec::new();
    if let Some((value, n)) = native_prefix(word) {
        if n >= 2 && n < word.len() {
            candidates.push((
                Spoken {
                    value,
                    fraction: None,
                    kind: Kind::Native,
                    lone: None,
                    first: word[..n].iter().collect(),
                },
                n,
            ));
        }
    }
    let n = word.iter().take_while(|&&c| is_sino(c)).count();
    if n >= 2 && n < word.len() {
        if let Some(value) = sino_value(&word[..n]) {
            candidates.push((
                Spoken {
                    value,
                    fraction: None,
                    kind: Kind::Sino,
                    lone: None,
                    first: word[..n].iter().collect(),
                },
                n,
            ));
        }
    }
    for (number, n) in candidates {
        let Some((unit, unit_len)) = unit_then_end(&word[n..]) else {
            continue;
        };
        // "오십도 안 남았어요": 도 written on is "even", not degrees.
        if unit.word == "도" || !fits(&number, unit) {
            continue;
        }
        if number.kind == Kind::Native && !unit.native {
            continue;
        }
        let start = words[i].core;
        return Some(Found {
            start,
            end: start + n + unit_len,
            text: written(&number, unit),
            unit_word: i,
            unit: unit.word,
        });
    }
    None
}

/// Whether the unit can take the number at all: the list of words that look
/// like numbers, a value it can have, a decimal only on a measure.
fn fits(number: &Spoken, unit: &Unit) -> bool {
    if let Some((_, units)) = NUMBER_WORDS.iter().find(|(w, _)| *w == number.first) {
        if !units.contains(&unit.word) {
            return false;
        }
    }
    if number.fraction.is_some() && !unit.measure {
        return false;
    }
    if number.value > unit.max {
        return false;
    }
    if number.value >= 10_000 && !unit.amount {
        return false;
    }
    if number.value >= 100_000_000 && !matches!(unit.word, "원" | "달러" | "명") {
        return false;
    }
    true
}

/// Around a number, what says it is one.
struct Around {
    /// The number before it goes with it ("11월 이 일").
    chain: bool,
    /// A time of day just before an hour ("오후 한 시").
    time_of_day: bool,
    /// "반" after an hour or hours ("한 시 반").
    half: bool,
    /// Minutes after an hour ("한 시 삼십 분").
    minutes: bool,
    /// "동안", "뒤", "전" after a length of time ("오 분 뒤").
    duration: bool,
}

impl Around {
    fn read(
        chars: &[char],
        words: &[Word],
        i: usize,
        unit_word: usize,
        unit: &Unit,
        last: Option<(usize, &'static str)>,
        unit_alone: bool,
    ) -> Around {
        let prev = (i > 0 && !words[i - 1].closed).then(|| letters(chars, &words[i - 1]));
        // The unit of the number just before: one written here, or digits
        // already in the text ("3시").
        let prev_unit: Option<String> = match last {
            Some((w, u)) if i > 0 && w == i - 1 && prev.is_some() => Some(u.to_string()),
            _ => prev.and_then(|p| {
                let digits = p.iter().take_while(|c| c.is_ascii_digit()).count();
                (digits > 0 && digits < p.len()).then(|| p[digits..].iter().collect())
            }),
        };
        let chain = prev_unit.is_some_and(|p| {
            CHAINS
                .iter()
                .any(|(from, to)| *from == p && to.contains(&unit.word))
        });
        let prev_word: String = prev.map(|p| p.iter().collect()).unwrap_or_default();
        let next: Option<String> =
            (unit_alone && unit_word + 1 < words.len() && !words[unit_word].closed)
                .then(|| letters(chars, &words[unit_word + 1]).iter().collect());
        let next_is = |list: &[&str]| next.as_deref().is_some_and(|n| list.contains(&n));
        // Minutes after: a Sino number word, then 분 starting the next word.
        let minutes = unit.word == "시"
            && next.as_deref().is_some_and(|n| {
                let n: Vec<char> = n.chars().collect();
                n.iter().all(|&c| is_sino(c)) && sino_value(&n).is_some_and(|v| v < 60)
            })
            && unit_word + 2 < words.len()
            && !words[unit_word + 1].closed
            && unit_then_end(letters(chars, &words[unit_word + 2]))
                .is_some_and(|(u, _)| u.word == "분");
        Around {
            chain,
            time_of_day: unit.word == "시" && TIMES_OF_DAY.contains(&prev_word.as_str()),
            half: matches!(unit.word, "시" | "시간") && next_is(HALF_AFTER),
            minutes,
            duration: DURATION_UNITS.contains(&unit.word) && next_is(DURATION_AFTER),
        }
    }
}

/// Whether a number may be written as digits before this unit.
fn allowed(number: &Spoken, unit: &Unit, around: &Around) -> bool {
    if number.kind == Kind::Native && !unit.native {
        return false;
    }
    let Some(lone) = number.lone else {
        return true; // Two syllables or more: "이십", "열두", "다섯".
    };
    let hour = around.time_of_day || around.half || around.minutes;
    match lone {
        // 한 and 네 are words first ("한 번 보자", "네, 시간 돼요").
        '한' => match unit.word {
            "시" => hour,
            "시간" => around.half || around.duration,
            _ => false,
        },
        '네' => unit.word == "시" && hour,
        // 이 is "this" before almost anything ("이 페이지", "이 시간").
        '이' => around.chain || matches!(unit.word, "퍼센트" | "개월" | "인분"),
        _ => {
            let safe = match number.kind {
                Kind::Sino => unit.lone_sino.contains(lone),
                Kind::Native => unit.lone_native.contains(lone),
            };
            safe || around.chain || (unit.word == "시" && hour) || around.duration
        }
    }
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

/// The number and unit as written: "3,500원", "50만 원", "3.5%".
fn written(number: &Spoken, unit: &Unit) -> String {
    let unit_text = if unit.word == "퍼센트" {
        "%"
    } else {
        unit.word
    };
    if let Some(fraction) = &number.fraction {
        return format!("{}.{}{}", number.value, fraction, unit_text);
    }
    let value = number.value;
    if value < 10_000 {
        return format!("{}{}", grouped(value, unit.amount), unit_text);
    }
    let mut parts = Vec::new();
    for (size, word) in [
        (1_000_000_000_000u64, "조"),
        (100_000_000, "억"),
        (10_000, "만"),
    ] {
        let part = if word == "조" {
            value / size
        } else {
            (value / size) % 10_000
        };
        if part > 0 {
            parts.push(format!("{}{}", grouped(part, true), word));
        }
    }
    let rest = value % 10_000;
    if rest > 0 {
        parts.push(format!("{}{}", grouped(rest, true), unit_text));
        parts.join(" ")
    } else {
        format!("{} {}", parts.join(" "), unit_text)
    }
}

#[cfg(test)]
mod tests {
    use super::spoken_to_digits as d;

    /// Made-up sentences and what they should become.
    pub(super) const CHANGED: &[(&str, &str)] = &[
        (
            "이십 초 동안 녹음을 하는 중입니다",
            "20초 동안 녹음을 하는 중입니다",
        ),
        ("이십초 동안 기다려요", "20초 동안 기다려요"),
        ("삼천오백 원이에요", "3,500원이에요"),
        ("삼천오백원이에요", "3,500원이에요"),
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
        ("오후 네 시에 회의", "오후 4시에 회의"),
        ("열두 시에 닫아요", "12시에 닫아요"),
        ("열한 시 사십오 분 기차", "11시 45분 기차"),
        ("다섯시에 끝나요", "5시에 끝나요"),
        ("다섯 명이 와요", "5명이 와요"),
        ("사과 두 개만 주세요", "사과 2개만 주세요"),
        ("스물다섯 살이에요", "25살이에요"),
        ("스물 다섯 살이에요", "25살이에요"),
        ("서른 명 정도 모였어요", "30명 정도 모였어요"),
        ("삼십 퍼센트 할인", "30% 할인"),
        ("삼 점 오 퍼센트 올랐어요", "3.5% 올랐어요"),
        ("영 점 오 킬로 줄었어요", "0.5킬로 줄었어요"),
        ("삼 년 동안 다녔어요", "3년 동안 다녔어요"),
        ("일 년에 한 번", "1년에 한 번"),
        ("삼십이 평 아파트", "32평 아파트"),
        ("백삼 동 천오백이 호", "103동 1502호"),
        ("이십 층에 살아요", "20층에 살아요"),
        ("십오 분 뒤에 출발", "15분 뒤에 출발"),
        ("오 분 뒤에 출발", "5분 뒤에 출발"),
        ("십일 월 이 일까지", "11월 2일까지"),
        ("3시 이 분에", "3시 2분에"),
        ("이천 원짜리", "2,000원짜리"),
        ("삼백오십 명", "350명"),
        ("삼백 오십 명", "350명"),
        ("천 원짜리", "1,000원짜리"),
        ("이십 미터 남았어요", "20미터 남았어요"),
        ("이천삼백 밀리라서", "2,300밀리라서"),
        ("구십 년대 노래", "90년대 노래"),
        ("사 개월 걸려요", "4개월 걸려요"),
        ("두 시간 걸렸어요", "2시간 걸렸어요"),
        ("한 시간 동안 했어요", "1시간 동안 했어요"),
        ("열 명이 넘어요", "10명이 넘어요"),
        ("백 명이 왔어요", "100명이 왔어요"),
        ("이십 대 후반", "20대 후반"),
        ("오십 달러예요", "50달러예요"),
        ("삼십 도가 넘어요", "30도가 넘어요"),
        ("(이십 초)", "(20초)"),
        ("이십 초.", "20초."),
        ("이십 번째 손님", "20번째 손님"),
        ("다섯 번 넘게", "5번 넘게"),
        ("두 배로 늘었어요", "2배로 늘었어요"),
        ("삼 학년이에요", "3학년이에요"),
    ];

    /// Made-up sentences that must stay as they are.
    pub(super) const KEPT: &[&str] = &[
        // Words that only look like numbers.
        "이번 주에 보내 드릴게요",
        "이분이 담당자세요",
        "일요일에 쉬어요",
        "삼촌 댁이에요",
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
        "만 원만 주세요",
        "네 시간 괜찮아요",
        "네 명이요",
        "한 명도 없어요",
        "한 달 뒤에 봐요",
        "열대 지방",
        "세대 차이가 나요",
        "천만에요",
        "이층 침대",
        "일년 내내",
        "일주일 전까지 보내 주세요",
        "이 차 누구 거예요",
        "삼일절",
        "회의 시작해요",
        "다섯 분이 오셨어요",
        "서너 명",
        "오십 차이 나요",
        "이 점은 중요해요",
        "조사도 필요합니다",
        "조사 시간은 얼마나 걸려요",
        "구조도 바꿔야 해요",
        "일조권 문제",
        "만일 시간이 되시면",
        "백사장에서 놀아요",
        "백일장에 나가요",
        "천사도 있어요",
        "이천 시에 갔어요",
        "오만 가지 생각",
        "가로 삼십 세로 사십",
        "오십도 안 남았어요",
        "오십 대 오십",
        "아이폰 십오 프로",
        "총 세대 수는",
        "세대 간 갈등",
        "최대 오차는",
        "최대 구동 시간",
        "사원 이상",
        "사장 뒤에 서세요",
        "천장 뒤에 배선",
        "사회 후에 식사",
        "구분 후에 정리",
        "그 일도 전에 끝냈어요",
        "매일 오시는 손님",
        "약 사시는 분",
        "세배 전에 인사",
        "이 페이지를 보시면",
        "이 시간 동안",
        "이 달 동안",
        "이동 동안에는",
        "몇 백 명",
        "몇 천 원",
        "몇 십 년",
        "수 백 명",
        "두 세 명",
        "한 두 번",
        "네 다섯 명",
        "삼 사 일",
        "이 박 삼 일",
        "2 천 원",
        "삼십 일 차",
        "두 시도 모두",
        "두 대도시",
        "페인트 칠 주의",
        "칠 일은 내일",
        "팔 쪽이 아파요",
        "창문 열 시 주의",
        "십 분 활용하세요",
        "어서 오십시요",
        "구십 점 오 명",
        "영 점 영 일 초",
        "이삼 일 걸려요",
        "두세 개 주세요",
        "첫 번째 두 번째",
        // Digits, English, no unit.
        "2026년 11월 25일",
        "3,500원",
        "02 123 4567",
        "공일공 일이삼사",
        "하나 둘 셋",
        "meeting at five",
    ];

    #[test]
    fn writes_numbers_before_units_as_digits() {
        let wrong: Vec<String> = CHANGED
            .iter()
            .filter(|(said, want)| d(said) != *want)
            .map(|(said, want)| format!("{said} -> {} (want {want})", d(said)))
            .collect();
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn leaves_words_that_only_look_like_numbers() {
        let wrong: Vec<String> = KEPT
            .iter()
            .filter(|said| d(said) != **said)
            .map(|said| format!("{said} -> {}", d(said)))
            .collect();
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn plain_text_is_returned_as_is() {
        assert_eq!(d(""), "");
        assert_eq!(d("   "), "   ");
        assert_eq!(d("Hello"), "Hello");
        assert_eq!(d("가나다"), "가나다");
    }

    #[test]
    fn long_runs_of_number_words_stay_quick() {
        let long = "천 ".repeat(200_000);
        let started = std::time::Instant::now();
        assert_eq!(d(&long), long);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}
