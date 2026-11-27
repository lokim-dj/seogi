//! Numbers in the minutes, read as values so they can be compared with the
//! words they rest on: "9평" is what "아홉 평" said, "1억 5천" is 150,000,000
//! won like "1억 5,000만 원", and "2억 천" is "2억 1천만".
//!
//! Also what a careful reader would doubt in a written amount: a big unit
//! said twice ("1억 5천억"), more than four digits before 만 or 억
//! ("80,900만원"), 억 with thousands but no 만 before 원 ("2억 천원"); the
//! same rate said with two values ("자당 20" and "자당 25만"); and a change
//! that does not add up ("1억 5천에서 2억 1천으로, 5,100만 원 늘었다").
//!
//! And the numbers of a whole meeting checked against each other, from its
//! lines rather than from the minutes: the same thing priced two ways, a
//! price times a count that is not the total said, parts that do not add up
//! to the total said, a change that does not match the amounts it went
//! between.
//!
//! A plain function of the text. It only says what to doubt, never changes
//! what was written.

use std::sync::LazyLock;

use regex::Regex;

/// How close two values must be to count as the same.
fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0)
}

/// One amount: a chain of digits and big units, as written.
#[derive(Debug, Clone, PartialEq)]
pub struct Amount {
    pub value: f64,
    /// Where it was in the text, in chars, for telling what is next to it.
    pub start: usize,
    pub end: usize,
    /// A big unit came twice or out of order, or more than four digits came
    /// before 만 or 억, or 억 with thousands but no 만 came before 원.
    pub odd: bool,
}

/// A number with an optional sub-unit (천 백 십) and big unit (만 억 조).
static PART: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:(\d[\d,]*(?:\.\d+)?)\s?([천백십])?|([천백]))\s?([만억조])?")
        .expect("a valid pattern")
});

fn big(unit: &str) -> f64 {
    match unit {
        "만" => 1e4,
        "억" => 1e8,
        "조" => 1e12,
        _ => 1.0,
    }
}

fn sub(unit: &str) -> f64 {
    match unit {
        "천" => 1e3,
        "백" => 1e2,
        "십" => 1e1,
        _ => 1.0,
    }
}

/// Whether what follows a 조 lets it be the unit: no word goes on, or 원
/// does ("3조원"), or a particle that ends the word ("3조가 넘는", "3조에").
/// A word that goes on ("조금", "조정") makes it no unit.
fn trillion_follows(rest: &str) -> bool {
    const PARTICLES: &str = "이가을를은는에의쯤";
    let mut chars = rest.chars();
    let hangul = |c: char| ('가'..='힣').contains(&c);
    match chars.next() {
        Some(c) if hangul(c) => {
            c == '원' || (PARTICLES.contains(c) && !chars.next().is_some_and(hangul))
        }
        _ => true,
    }
}

/// The amounts written with digits in `text`, big units joined: "1억 5,000만
/// 원" is one amount of 150,000,000.
pub fn amounts(text: &str) -> Vec<Amount> {
    let chars = |byte: usize| text[..byte].chars().count();
    let mut found: Vec<Amount> = Vec::new();
    let mut at = 0;
    while let Some(digit) = text[at..].find(|c: char| c.is_ascii_digit()) {
        let start = at + digit;
        let (value, end, odd) = chain(text, start);
        found.push(Amount {
            value,
            start: chars(start),
            end: chars(end),
            odd,
        });
        at = end.max(start + 1);
        while !text.is_char_boundary(at) {
            at += 1;
        }
    }
    found
}

/// Reads one chain of parts from `start`: its value, where it ends (bytes),
/// and whether it is written oddly.
fn chain(text: &str, start: usize) -> (f64, usize, bool) {
    let mut value = 0.0;
    let mut group = 0.0;
    let mut bigs: Vec<f64> = Vec::new();
    let mut odd = false;
    let mut end = start;
    let mut pos = start;
    let mut first = true;
    while let Some(caps) = PART.captures_at(text, pos) {
        let whole = caps.get(0).expect("the whole match");
        if whole.start() != pos || whole.as_str().trim().is_empty() {
            break;
        }
        let bare = caps.get(3).is_some();
        let sub_unit = caps.get(2).or(caps.get(3)).map_or("", |m| m.as_str());
        let mut big_unit = caps.get(4).map_or("", |m| m.as_str());
        let mut whole_text = whole.as_str();
        // "100 조금" is a hundred and a little, not a hundred trillion;
        // "3조원", "3조가 넘는" are trillions.
        if big_unit == "조" && !trillion_follows(&text[whole.end()..]) {
            big_unit = "";
            let unit_at = whole_text.rfind('조').unwrap_or(whole_text.len());
            whole_text = &whole_text[..unit_at];
        }
        // "2만 3만 원" is a range, two amounts: the same big unit again with
        // no thousands before it ends the chain ("1억 5천억" does not).
        if !first
            && !big_unit.is_empty()
            && sub_unit.is_empty()
            && bigs.last().is_some_and(|&b| same(b, big(big_unit)))
        {
            break;
        }
        // A bare 천 or 백 goes on a chain, never starts one, and is no
        // number when a word goes on after it ("2억, 천장 포함"); a plain
        // number after a chain is a number of its own ("3m에 48만").
        if (first && bare) || (!first && sub_unit.is_empty() && big_unit.is_empty()) {
            break;
        }
        if bare && big_unit.is_empty() {
            const AFTER: &str = "원만이을은에으로정쯤가까짜대선";
            let next = text[pos + whole_text.trim_end().len()..].chars().next();
            if next.is_some_and(|c| ('가'..='힣').contains(&c) && !AFTER.contains(c)) {
                break;
            }
        }
        let written = caps.get(1).map_or("1", |m| m.as_str()).replace(',', "");
        let Ok(number) = written.parse::<f64>() else {
            break;
        };
        let whole_digits = written.split('.').next().unwrap_or("").len();
        if !big_unit.is_empty() && sub_unit.is_empty() && whole_digits > 4 {
            odd = true;
        }
        group += number * sub(sub_unit);
        end = pos + whole_text.trim_end().len();
        first = false;
        if big_unit.is_empty() {
            break;
        }
        let unit = big(big_unit);
        if bigs.iter().any(|&b| b <= unit) {
            odd = true;
        }
        bigs.push(unit);
        value += group * unit;
        group = 0.0;
        // Another part may follow after one space at most.
        let rest = &text[end..];
        let skip = usize::from(rest.starts_with(' '));
        if !rest[skip..].starts_with(|c: char| c.is_ascii_digit() || c == '천' || c == '백') {
            break;
        }
        pos = end + skip;
    }
    if group > 0.0 {
        // "1억 5천" is said for 1억 5천만: thousands after 억 with no 만.
        if bigs.last().is_some_and(|&b| same(b, 1e8)) && group < 1e4 {
            if text[end..].trim_start().starts_with('원') {
                odd = true;
            }
            value += group * 1e4;
        } else {
            value += group;
        }
    }
    (value, end, odd)
}

/// Whether `text` writes an amount a careful reader would doubt.
pub fn odd_units(text: &str) -> bool {
    amounts(text).iter().any(|amount| amount.odd)
}

/// Native Korean numbers, for "아홉 평", "두 개".
const NATIVE: &[(&str, f64)] = &[
    ("하나", 1.0),
    ("한", 1.0),
    ("두", 2.0),
    ("둘", 2.0),
    ("세", 3.0),
    ("셋", 3.0),
    ("네", 4.0),
    ("넷", 4.0),
    ("다섯", 5.0),
    ("여섯", 6.0),
    ("일곱", 7.0),
    ("여덟", 8.0),
    ("아홉", 9.0),
    ("열", 10.0),
    ("스무", 20.0),
    ("스물", 20.0),
    ("서른", 30.0),
    ("마흔", 40.0),
    ("쉰", 50.0),
];

/// A word number before a counting word: "아홉 평", "한 개소", "열 자".
static WORD_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:^|[^가-힣])((?:스물|서른|마흔|쉰|열)?(?:하나|다섯|여섯|일곱|여덟|아홉|한|두|둘|세|셋|네|넷)?|열|스무|스물|서른|마흔|쉰|[일이삼사오육칠팔구십백천]{1,6})\s?(평|개소|개|명|층|곳|군데|자|번|대|장|미터|분|시간|일|주|달)",
    )
    .expect("a valid pattern")
});

fn native_value(word: &str) -> Option<f64> {
    let mut value = 0.0;
    let mut rest = word;
    for tens in ["스물", "서른", "마흔", "쉰", "열"] {
        if let Some(after) = rest.strip_prefix(tens) {
            value += NATIVE.iter().find(|(w, _)| *w == tens)?.1;
            rest = after;
            break;
        }
    }
    if !rest.is_empty() {
        value += NATIVE.iter().find(|(w, _)| *w == rest)?.1;
    }
    (value > 0.0).then_some(value)
}

fn sino_value(word: &str) -> Option<f64> {
    let digit = |c: char| "일이삼사오육칠팔구".find(c).map(|i| (i / 3 + 1) as f64);
    let mut value = 0.0;
    let mut pending: Option<f64> = None;
    for c in word.chars() {
        let place = match c {
            '십' => 10.0,
            '백' => 100.0,
            '천' => 1000.0,
            _ => {
                pending = Some(digit(c)?);
                continue;
            }
        };
        value += pending.take().unwrap_or(1.0) * place;
    }
    value += pending.unwrap_or(0.0);
    (value > 0.0).then_some(value)
}

/// Every value the words of `text` give, written with digits or said in
/// words: what a stated number may be checked against. Generous on purpose:
/// it only keeps a number that was said from being doubted.
pub fn values_said(text: &str) -> Vec<f64> {
    let mut values: Vec<f64> = Vec::new();
    let spoken = super::numbers::spoken_to_digits(text);
    for source in [text, spoken.as_str()] {
        for amount in amounts(source) {
            values.push(amount.value);
            // "5천" may be said for 5천만, and "1억 5천억" meant 1억 5천만.
            if amount.value < 1e4 {
                values.push(amount.value * 1e4);
            }
        }
        for caps in WORD_NUMBER.captures_iter(source) {
            let word = &caps[1];
            if let Some(value) = native_value(word).or_else(|| sino_value(word)) {
                values.push(value);
            }
        }
    }
    values
}

/// Whether every number `stated` gives was said in `evidence`: as the same
/// digits, or as the same value however written.
pub fn backed(stated: &str, evidence: &str) -> bool {
    let runs =
        digit_runs(evidence) + " " + &digit_runs(&super::numbers::spoken_to_digits(evidence));
    let said = values_said(evidence);
    let mut covered: Vec<(usize, usize)> = Vec::new();
    for amount in amounts(stated) {
        let chars: String = stated
            .chars()
            .skip(amount.start)
            .take(amount.end - amount.start)
            .collect();
        let all_runs_said = digit_list(&chars).iter().all(|run| has_run(&runs, run));
        if all_runs_said || said.iter().any(|&v| same(v, amount.value)) {
            covered.push((amount.start, amount.end));
        } else {
            return false;
        }
    }
    // Digits outside any amount (a time, a model number) must be said as such.
    let outside: String = stated
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if covered.iter().any(|&(s, e)| i >= s && i < e) {
                ' '
            } else {
                c
            }
        })
        .collect();
    digit_list(&outside).iter().all(|run| has_run(&runs, run))
}

fn digit_list(text: &str) -> Vec<String> {
    let mut runs = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_digit() {
            current.push(c);
        } else if c == ','
            && !current.is_empty()
            && chars.get(i + 1).is_some_and(char::is_ascii_digit)
        {
        } else if !current.is_empty() {
            runs.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        runs.push(current);
    }
    runs
}

fn digit_runs(text: &str) -> String {
    format!(" {} ", digit_list(text).join(" "))
}

fn has_run(runs: &str, run: &str) -> bool {
    runs.contains(&format!(" {run} "))
}

/// A price per something: "자당 25만", "평당 300만", or a length and its
/// price, "3m에 48만" (per metre).
static PER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:(?P<what0>[가-힣A-Za-z]+)\s+)?(?:(?P<what>[가-힣A-Za-z]+)\s+)?(?P<per>[가-힣a-zA-Z]{1,3})당\s?(?P<n>\d[\d,]*(?:\.\d+)?)\s?(?P<scale>만|천)?(?P<won>\s?원)?",
    )
    .expect("a valid pattern")
});
/// A length in metres and its price; "1200mm" and "120cm" are no metres.
static LENGTH_PRICE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:(?P<what>[가-힣A-Za-z]+)\s+)?(?P<len>\d+(?:\.\d+)?)\s?(?:m(?:[^a-zA-Z]|$)|미터)\s?(?:에|씩|짜리|당)?[^\d.]{0,12}?(?P<price>\d[\d,]*)\s?만",
    )
    .expect("a valid pattern")
});

/// Words before a price that point rather than name ("여기도 자당 25만",
/// "각각 1m씩"): the price may be of the same thing as another.
const POINTING: &[&str] = &[
    "여기", "거기", "저기", "이것", "그것", "이쪽", "그쪽", "각각", "각", "다", "모두", "전부",
    "이", "그", "저", "요",
];

/// Pointing words that tie a price to the one just said: "각각", "모두"
/// (each of them), or any pointing word with 도 ("여기도", "이것도").
const EACH: &[&str] = &["각각", "각", "다", "모두", "전부"];

/// Nouns that stand for a thing without naming it ("같은 경우는", "쪽은",
/// "하는 거는", "깔 때는", "부분은", "기준으로"), and pointing forms that
/// may point at something else ("그건 평당 18만" after another was asked).
const BOUND_NOUNS: &[&str] = &[
    "경우",
    "쪽",
    "거",
    "것",
    "때",
    "데",
    "부분",
    "기준",
    "정도",
    "이상",
    "이하",
    "종류",
    "식",
    "방식",
    "형태",
    "타입",
    "위치",
    "자리",
    "곳",
    "편",
    "중",
    "건",
    "게",
    "이건",
    "그건",
    "저건",
    "이거",
    "그거",
    "저거",
    "이게",
    "그게",
    "저게",
    "하나",
    "둘",
    "번째",
    "나머지",
    "다른",
    "제품",
    "모델",
    "전자",
    "후자",
    "이번",
    "저번",
    "쪽거",
    "상품",
    "샘플",
];

/// Endings that make a word a verb or a joint, not a thing: "그랬는데",
/// "하니까", "기준으로", "해서", "하고".
const NOT_A_NOUN_END: &[&str] = &[
    "는데", "니까", "으로", "해서", "하고", "지만", "다가", "려고", "면서", "인데", "라서", "어서",
    "아서", "고요", "구요", "세요", "네요", "하면", "이면", "으면", "러면", "이고", "라고", "다고",
    "했고", "했는", "하는", "같은",
];

/// The last letters of a word that make it a verb, never a noun's: "할",
/// "된", "던" before a noun, "까", "죠", "요" at a sentence's end. Letters
/// that real nouns also end in (벽지, 베란다, 화면) are not here.
const NOT_A_NOUN_LAST: &[char] = &['까', '며', '요', '죠', '할', '된', '던'];

/// Whether a word (without its particle) passes as the name of a thing: two
/// letters or more, not a bound noun, a filler or a verb. A positive test:
/// what does not pass names nothing.
fn is_name(word: &str) -> bool {
    word.chars().count() >= 2
        && word
            .chars()
            .all(|c| ('가'..='힣').contains(&c) || c.is_ascii_alphabetic())
        && !BOUND_NOUNS.contains(&word)
        && !FILLER.contains(&word)
        && !NOT_A_NOUN_END.iter().any(|end| word.ends_with(end))
        && !word.ends_with(NOT_A_NOUN_LAST)
}

/// Words before a price that say how or when, not what of: "보통 평당
/// 12만", "지금 평당 8만".
const FILLER: &[&str] = &[
    // Money and who speaks name no thing: "강마루 단가는 평당 12만" is
    // 강마루's price (the word before is taken), "저희는 평당 12만" no one's.
    "단가",
    "가격",
    "자재",
    "자재비",
    "시공비",
    "인건비",
    "견적",
    "금액",
    "비용",
    "저희",
    "우리",
    "그러면",
    "제가",
    "보통",
    "지금",
    "원래",
    "대략",
    "거의",
    "그냥",
    "일단",
    "요즘",
    "대충",
    "약",
    "한",
    "그럼",
    "그래서",
    "근데",
    "이제",
    "기본",
    "기본적으로",
    "최소",
    "최대",
    "평균",
    "대개",
    "주로",
    "통상",
    "보통은",
    "정도",
    "그리고",
    "또",
    "다시",
    "현재",
    "요새",
    "실제로",
    "사실",
    "대체로",
    "전체",
    "전체적으로",
];

/// What a price is of, as the words around it say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// A word names it: "강마루 평당 12만", "평당 12만 원짜리 강마루".
    Named(String),
    /// A word points rather than names ("여기도", "각각"): it may be the
    /// same thing as another price.
    Pointing,
    /// No word says what it is of.
    Unsaid,
}

/// A word that names what a price is of, after it: "평당 12만 원짜리
/// 강마루", "평당 12만 원의 강마루". Only these joins: a plain word after a
/// price is as often "기준으로" or "정도" as a thing.
static NAMED_AFTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s?원?\s?(?:짜리|의)\s+(?P<what>[가-힣A-Za-z]+)").expect("a valid pattern")
});

/// Words after "원의" that name no thing but the money itself ("평당 12만
/// 원의 차이", "…원의 견적"): no subject.
const NOT_A_THING: &[&str] = &[
    "차이",
    "차액",
    "견적",
    "비용",
    "금액",
    "가격",
    "정도",
    "수준",
    "예산",
    "공사비",
    "범위",
];

/// What a price is of, from a word without its particle.
fn priced(word: Option<regex::Match<'_>>) -> Subject {
    let Some(word) = word else {
        return Subject::Unsaid;
    };
    let bare = strip_particle(word.as_str());
    if POINTING.contains(&bare.as_str()) {
        Subject::Pointing
    } else if is_name(&bare) {
        Subject::Named(bare)
    } else {
        // "보통 평당 12만", "그러면 평당 15만", "같은 경우는 평당 12만": no
        // word names a thing.
        Subject::Unsaid
    }
}

/// A word without the particle at its end: "강마루는" is "강마루".
fn strip_particle(word: &str) -> String {
    let mut bare = word.to_string();
    for particle in [
        "에서는",
        "에는",
        "은",
        "는",
        "도",
        "이",
        "가",
        "을",
        "를",
        "의",
        "에",
        "만",
    ] {
        if bare.chars().count() > particle.chars().count() && bare.ends_with(particle) {
            bare.truncate(bare.len() - particle.len());
            break;
        }
    }
    bare
}

/// The pointing words the entry-level check (`rates_differ`) has always
/// known.
const POINTING_LOOSE: &[&str] = &[
    "여기", "거기", "저기", "이것", "그것", "이쪽", "그쪽", "각각", "각", "다", "모두", "전부",
];

/// What a price is of for the entry-level check: any word before it names
/// it, unless it points. That check reads a few lines only, and a word that
/// is no name by `is_name` still sets two prices apart there ("벽지는",
/// "장판은").
fn subject_loose(before: Option<regex::Match<'_>>, rest: &str) -> Subject {
    match before {
        Some(word) => {
            let bare = strip_particle(word.as_str());
            if POINTING_LOOSE.contains(&bare.as_str()) {
                Subject::Pointing
            } else {
                Subject::Named(bare)
            }
        }
        None => subject(None, rest),
    }
}

/// What a price is of: the word before it, or when none, the word it is
/// joined to after it (`rest`, the text after the price).
fn subject(before: Option<regex::Match<'_>>, rest: &str) -> Subject {
    match priced(before) {
        Subject::Unsaid => match priced(NAMED_AFTER.captures(rest).and_then(|c| c.name("what"))) {
            Subject::Named(word) if NOT_A_THING.iter().any(|w| word.starts_with(w)) => {
                Subject::Unsaid
            }
            after => after,
        },
        said => said,
    }
}

/// One price per something a text gives.
#[derive(Debug, Clone, PartialEq)]
pub struct Rate {
    /// What it is per: "자", "평", "m".
    pub per: String,
    /// The price for one.
    pub value: f64,
    /// What it is the price of, as the words say.
    pub what: Subject,
}

/// The rates `text` gives.
pub fn rates(text: &str) -> Vec<Rate> {
    placed_rates(text).into_iter().map(|p| p.rate).collect()
}

/// Whether a rate `stated` gives is said with another value nearby in
/// `around`: the same thing priced two ways. Prices of things named
/// differently ("강마루 평당 12만", "원목마루 평당 18만", or "평당 12만 원짜리
/// 강마루") are two prices. A price that no word says the thing of is
/// compared with a named one ("강마루 평당 12만", then "평당 15만 원이요"),
/// but two such prices are not: missing a doubt is better than raising a
/// wrong one.
pub fn rates_differ(stated: &str, around: &str) -> bool {
    let loose = |text: &str| -> Vec<Rate> {
        placed_rates(text)
            .into_iter()
            .map(|placed| Rate {
                what: placed.loose,
                ..placed.rate
            })
            .collect()
    };
    let near = loose(around);
    loose(stated).iter().any(|mine| {
        near.iter().any(|other| {
            let same_thing = match (&mine.what, &other.what) {
                (Subject::Unsaid, Subject::Unsaid) => false,
                (Subject::Named(a), Subject::Named(b)) => a == b,
                _ => true,
            };
            other.per == mine.per && same_thing && !same(other.value, mine.value)
        })
    })
}

/// A change of an amount: from and to, or by how much, as one text says.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Change {
    pub from: Option<f64>,
    pub to: Option<f64>,
    pub by: Option<f64>,
}

static FROM_TO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"에서\s*(?:약\s*)?$").expect("a valid pattern"));
/// What follows the second amount of "A에서 B" when it is a range.
static RANGE_END: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s?(?:원)?\s?(?:사이|까지|선|범위|정도|쯤|안쪽|생각|예상)")
        .expect("a valid pattern")
});
static BY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:원)?\s*(?:정도|가량|쯤)?\s*(?:이|가)?\s*(?:더\s*)?(늘|증가|올라|올랐|상승|많아|줄(?:었|어|여|일|인)|감소|내려|낮아|빠)")
        .expect("a valid pattern")
});

/// The changes `text` states: "A에서 B로", "C 늘었다".
pub fn changes(text: &str) -> Vec<Change> {
    let found = amounts(text);
    let chars: Vec<char> = text.chars().collect();
    let slice = |s: usize, e: usize| -> String {
        chars[s.min(chars.len())..e.min(chars.len())]
            .iter()
            .collect()
    };
    let mut out = Vec::new();
    for (i, amount) in found.iter().enumerate() {
        if amount.value < 1e4 {
            continue;
        }
        let after = slice(amount.end, amount.end + 16);
        if let Some(next) = found.get(i + 1) {
            let between = slice(amount.end, next.start);
            // "3천만 원에서 5천만 원 사이" is a range, not a move.
            let after_next = slice(next.end, next.end + 8);
            if FROM_TO.is_match(&between) && next.value >= 1e4 && !RANGE_END.is_match(&after_next) {
                out.push(Change {
                    from: Some(amount.value),
                    to: Some(next.value),
                    by: None,
                });
            }
        }
        if BY.is_match(&after) {
            out.push(Change {
                from: None,
                to: None,
                by: Some(amount.value),
            });
        }
    }
    out
}

/// Of `texts` (the entries of the minutes), those whose changes do not add
/// up with another's: one says the amount went from A to B, another (or the
/// same) that it went up by C, where C is near B − A but not it. Positions
/// in `texts`.
pub fn sums_disagree(texts: &[&str]) -> Vec<usize> {
    let all: Vec<Vec<Change>> = texts.iter().map(|text| changes(text)).collect();
    let mut odd: Vec<usize> = Vec::new();
    for (i, own) in all.iter().enumerate() {
        for by in own.iter().filter_map(|c| c.by) {
            // Of the moves said anywhere, the ones this change may be about.
            let moves: Vec<(usize, f64)> = all
                .iter()
                .enumerate()
                .flat_map(|(j, changes)| {
                    changes.iter().filter_map(move |c| match (c.from, c.to) {
                        (Some(from), Some(to)) => Some((j, (to - from).abs())),
                        _ => None,
                    })
                })
                .collect();
            if moves.iter().any(|&(_, size)| same(size, by)) {
                continue;
            }
            for (j, size) in moves {
                if size > 0.0 && by / size >= 0.5 && by / size <= 2.0 {
                    odd.push(i);
                    odd.push(j);
                }
            }
        }
    }
    odd.sort_unstable();
    odd.dedup();
    odd
}

// ---------------------------------------------------------------------------
// The numbers of one meeting, checked against each other
// ---------------------------------------------------------------------------

/// What does not agree among the numbers said in a meeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisagreementKind {
    /// The same thing priced per unit with two values ("강마루 평당 12만",
    /// later "강마루 평당 15만"; "3m에 48만" beside "각각 1m씩 32만").
    Rate,
    /// A price per unit times a count is not the total said for it ("개당 5만
    /// 원이고 아홉 개니까 50만 원").
    Product,
    /// Parts said with a total that is not their sum ("300만, 450만, 합쳐서
    /// 800만").
    Sum,
    /// A change from one amount to another and how much it changed by do not
    /// agree ("1억 5천에서 2억 1천으로, 5,100만 원 늘었다").
    Change,
}

/// Numbers said in a meeting that disagree with each other.
#[derive(Debug, Clone, PartialEq)]
pub struct Disagreement {
    pub kind: DisagreementKind,
    /// The lines the numbers are in (positions in the lines given); the same
    /// twice when one line holds them all.
    pub first: usize,
    pub second: usize,
    /// The values that disagree, for finding what else states them.
    pub values: Vec<f64>,
    /// What does not agree, in plain words: "m당 16만 원과 m당 32만 원".
    pub note: String,
    /// For prices of a thing named the same, its name ("강마루"); empty
    /// otherwise.
    pub name: String,
}

/// How many lines apart a price that does not name its thing ("여기도",
/// "각각", or no word) is still compared with a named one.
const NEAR_LINES: usize = 4;
/// How many lines apart a change and a move are taken to be about one
/// amount.
const CHANGE_LINES: usize = 6;

/// An amount as people say it: "2억 1,000만 원", "45만 원", "16.5만 원".
pub fn won(value: f64) -> String {
    let grouped = |n: u64| -> String {
        let digits = n.to_string();
        let mut out = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        out
    };
    let value = value.max(0.0);
    if value >= 1e8 {
        let eok = (value / 1e8).floor();
        let man = ((value - eok * 1e8) / 1e4).round() as u64;
        if man == 0 {
            format!("{}억 원", eok as u64)
        } else {
            format!("{}억 {}만 원", eok as u64, grouped(man))
        }
    } else if value >= 1e4 {
        let man = value / 1e4;
        if same(man, man.round()) {
            format!("{}만 원", grouped(man.round() as u64))
        } else {
            let one = format!("{:.1}", man);
            format!("{}만 원", one.trim_end_matches('0').trim_end_matches('.'))
        }
    } else {
        format!("{}원", grouped(value.round() as u64))
    }
}

/// Words that say an amount is rough: "한 500 정도", "약 2억".
static ROUGH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"정도|쯤|가량|대략|약\s?\d|한\s\d|좀|조금|넘|안 되|미만|이상|남짓|내외|언저리")
        .expect("a valid pattern")
});

/// Whether `said` is `expected`, allowing the roughness of speech when the
/// words around it say it is rough. A bare small number may be said for
/// that many 만 ("한 500 정도" for 500만).
fn agrees(said: f64, expected: f64, rough: bool) -> bool {
    let close = |value: f64| {
        let tolerance = if rough { 0.1 } else { 0.01 };
        (value - expected).abs() <= tolerance * expected.abs().max(1.0)
    };
    close(said) || (said < 1e4 && close(said * 1e4))
}

/// The value of `said` nearest `expected`: itself, or that many 만.
fn as_meant(said: f64, expected: f64) -> f64 {
    if said < 1e4 && (said * 1e4 - expected).abs() < (said - expected).abs() {
        said * 1e4
    } else {
        said
    }
}

/// Whether two values are far enough apart to be about different things
/// rather than one said wrong: more than twice, or less than half.
fn unrelated(said: f64, expected: f64) -> bool {
    expected <= 0.0 || said / expected > 2.0 || said / expected < 0.5
}

/// A rate with where it is in its line, in chars.
struct Placed {
    rate: Rate,
    start: usize,
    end: usize,
    /// Said with "당" ("자당", "평당"), not a length and its price.
    per_word: bool,
    /// The start of a range ("평당 12만에서 15만 사이"): no single price.
    range: bool,
    /// The price as it was said: "평당 12만", "3m에 48만".
    said: String,
    /// What it is of for the entry-level check (`subject_loose`).
    loose: Subject,
    /// Its name was said with a noun particle ("강마루는", "벽지가"): only
    /// such a name is matched beyond a few lines. Endings cannot tell a noun
    /// from a verb ("벽지" and "깔지", "화면" and "깔면"); the particle can.
    marked: bool,
    /// The word before it says "this too" ("여기도", "이것도"): the same
    /// thing as the price just before.
    also: bool,
}

/// Whether a word ends in a noun particle (은 는 이 가) after a noun:
/// "강마루는", "벽지가", "단가는".
fn topic(word: &str) -> bool {
    let bare = strip_particle(word);
    // Only 은 는 이 가 mark a noun here: 도 is as often a verb's "even if"
    // or an adverb ("그래도", "아마도"), and "this too" is read by `ties`;
    // "이번에는" is a time.
    let particle = ["은", "는", "이", "가"]
        .iter()
        .any(|particle| word.ends_with(particle));
    // "들어가는", "나오는", "하는", "되는": a verb before a noun.
    let verb_before_noun = (word.ends_with('는') || word.ends_with('은'))
        && bare.ends_with(['가', '오', '하', '되', '시', '내', '보', '주', '우', '꾸'])
        && !FILLER.contains(&bare.as_str());
    particle
        && !verb_before_noun
        && bare != word
        && !word.ends_with("에는")
        && !word.ends_with("에서는")
        && bare.chars().count() >= 1
}

/// Words before a noun that pick one of several ("이 제품은", "첫 번째는",
/// "다른 거는"): the price is of whichever was shown, not of a named thing.
const DETERMINERS: &[&str] = &[
    "이", "그", "저", "첫", "두", "세", "다른", "어떤", "어느", "요",
];

/// Whether a pointing word ties a price to another: "여기도", "각각".
fn ties(word: &str) -> bool {
    word.ends_with('도') || EACH.iter().any(|each| word.starts_with(each))
}

/// What follows a price that makes it the start of a range.
static RANGE_AFTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s?(?:원)?\s?(?:에서|부터|~|-|내지)\s?\d").expect("a valid pattern")
});

/// The rates `text` gives, with where each is.
fn placed_rates(text: &str) -> Vec<Placed> {
    let chars = |byte: usize| text[..byte].chars().count();
    let mut found = Vec::new();
    for caps in PER.captures_iter(text) {
        let Ok(number) = caps["n"].replace(',', "").parse::<f64>() else {
            continue;
        };
        // "자당 20" and "자당 25만" are compared as said: a bare number is
        // the same scale as the others are. A number followed by 원 alone
        // is won ("개당 3000원").
        let scale = match (
            caps.name("scale").map(|m| m.as_str()),
            caps.name("won").is_some(),
        ) {
            (Some("천"), _) => 1e3,
            (Some(_), _) => 1e4,
            (None, true) => 1.0,
            (None, false) => 1e4,
        };
        let end = caps.get(0).map_or(text.len(), |m| m.end());
        let start = caps.name("n").map_or(end, |m| m.start());
        let per_start = caps.name("per").map_or(start, |m| m.start());
        // A word that says how or when ("여기도 이제 자당 25만") gives way
        // to the word before it.
        // A word that names nothing ("저희는", "쪽은", "이제") gives way to
        // the word before it, when that one names or points ("거실 쪽은",
        // "여기도 이제").
        // A word with no noun particle ("아마", "깔면") gives way to a
        // particle-marked noun before it ("강마루는 아마 평당 12만").
        let word = match (caps.name("what0"), caps.name("what")) {
            (Some(first), Some(second))
                if priced(Some(second)) == Subject::Unsaid
                    && priced(Some(first)) != Subject::Unsaid =>
            {
                Some(first)
            }
            (Some(first), Some(second)) if !topic(second.as_str()) && topic(first.as_str()) => {
                Some(first)
            }
            (first, None) => first,
            (_, second) => second,
        };
        let picked = caps
            .name("what0")
            .is_some_and(|w| DETERMINERS.contains(&w.as_str()));
        let what = if picked {
            Subject::Unsaid
        } else {
            subject(word, &text[end..])
        };
        // Said as a topic ("강마루는", "강마루 단가는"): a name that may be
        // matched across the whole meeting.
        let marked = matches!(what, Subject::Named(_))
            && (word.is_some_and(|w| topic(w.as_str()))
                || caps.name("what").is_some_and(|w| topic(w.as_str())));
        let also = what == Subject::Pointing && word.is_some_and(|w| ties(w.as_str()));
        found.push(Placed {
            marked,
            loose: subject_loose(caps.name("what").or(caps.name("what0")), &text[end..]),
            also,
            range: RANGE_AFTER.is_match(&text[end..]),
            said: text[per_start..end].trim().to_string(),
            rate: Rate {
                per: caps["per"].to_string(),
                value: number * scale,
                what,
            },
            start: chars(start),
            end: chars(end),
            per_word: true,
        });
    }
    for caps in LENGTH_PRICE.captures_iter(text) {
        let (Ok(length), Ok(price)) = (
            caps["len"].parse::<f64>(),
            caps["price"].replace(',', "").parse::<f64>(),
        ) else {
            continue;
        };
        if length > 0.0 {
            let end = caps.get(0).map_or(text.len(), |m| m.end());
            let start = caps.name("len").map_or(end, |m| m.start());
            let what = subject(caps.name("what"), &text[end..]);
            // "각각 1m씩 32만": each of the lengths at that price.
            let also = what == Subject::Pointing
                && (caps.name("what").is_some_and(|w| ties(w.as_str()))
                    || text[start..end].contains('씩'));
            found.push(Placed {
                marked: false,
                loose: subject_loose(caps.name("what"), &text[end..]),
                also,
                range: RANGE_AFTER.is_match(&text[end..]),
                said: text[start..end].trim().to_string(),
                rate: Rate {
                    per: "m".to_string(),
                    value: price * 1e4 / length,
                    what,
                },
                start: chars(start),
                end: chars(end),
                per_word: false,
            });
        }
    }
    found
}

/// A count of something: "9개", "10평", "12자", "2m".
static COUNT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d+(?:\.\d+)?)\s?(개소|세트|미터|자|평|개|장|곳|짝|m)").expect("a valid pattern")
});

/// Whether a count's unit ends where the word does: "3개월" is no count of
/// 개, "자리" no 자, "3mm" no metres.
fn count_ends(unit: &str, next: Option<char>) -> bool {
    let Some(next) = next else {
        return true;
    };
    let goes_on = match unit {
        "개" => "월".contains(next),
        "자" => "리재세체동기".contains(next),
        "장" => "판식소님".contains(next),
        "평" => "균가생".contains(next),
        "m" => next.is_ascii_alphabetic(),
        _ => false,
    };
    !goes_on
}

/// Words that make the amount after them something other than a plain
/// total: a discount, what is included or left out, a payment in parts.
static NOT_A_TOTAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"포함|깎|할인|잔금|중도금|계약금|부가세|빼|별도|추가|네고|에누리|제외")
        .expect("a valid pattern")
});

/// The unit a count is in, as a rate says it.
fn count_unit(unit: &str) -> &str {
    match unit {
        "미터" => "m",
        "개소" => "곳",
        other => other,
    }
}

/// Words between a count and its total that join them: "아홉 개니까 50만",
/// "10평이라 80만", "12자 총 300만".
static TOTAL_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"니까|이라|라서|이면|하면|해서|면|총|합계|전체|모두|전부|=|이니")
        .expect("a valid pattern")
});

/// Words that give the total of the amounts before them.
static SUM_WORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"합쳐서|합치면|합하면|더하면|다 해서|전부 해서|모두 해서|다 합해서|총|합계|토탈|해서",
    )
    .expect("a valid pattern")
});

fn char_slice(chars: &[char], start: usize, end: usize) -> String {
    chars[start.min(chars.len())..end.min(chars.len())]
        .iter()
        .collect()
}

/// The same thing priced per unit with two values, across the lines. Two
/// prices are compared when a word names the same thing in both, anywhere
/// in the meeting; or when one is named and the other only points or names
/// nothing ("여기도 자당 25만"), a few lines apart. Each thing is told once.
fn rate_disagreements(lines: &[&str], speakers: &[u32]) -> Vec<Disagreement> {
    let all: Vec<(usize, Placed)> = lines
        .iter()
        .enumerate()
        .flat_map(|(at, line)| placed_rates(line).into_iter().map(move |p| (at, p)))
        .collect();
    // A price that says "this too" is tied to one a few lines before only
    // when the same person goes on and no one asks about another thing in
    // between ("타일은 얼마예요?").
    let tied = |first: usize, second: usize| -> bool {
        second - first <= NEAR_LINES
            && speakers[first] == speakers[second]
            && lines[(first + 1).min(second)..second].iter().all(|line| {
                !line.contains('?') && !line.contains("얼마") && !line.trim_end().ends_with("요?")
            })
    };
    let mut found: Vec<Disagreement> = Vec::new();
    let mut told: Vec<(String, String)> = Vec::new();
    for (i, (line_a, a)) in all.iter().enumerate() {
        for (line_b, b) in &all[i + 1..] {
            // Two values far apart are two things, and a range holds both.
            if a.rate.per != b.rate.per
                || same(a.rate.value, b.rate.value)
                || unrelated(a.rate.value, b.rate.value)
                || a.range
                || b.range
            {
                continue;
            }
            let apart = line_b - line_a;
            let name = match (&a.rate.what, &b.rate.what) {
                // A length and its price is only read near another: the
                // words before a length are as often "여기가" as a thing.
                (Subject::Named(x), Subject::Named(y))
                    if x == y
                        && if a.per_word && b.per_word {
                            // Prices per unit are matched by a name said as
                            // a topic, near or far: "강마루로 가면 평당 12만",
                            // "원목으로 가면 평당 18만" are two options.
                            a.marked && b.marked
                        } else {
                            apart <= NEAR_LINES
                        } =>
                {
                    x.clone()
                }
                // A price that only points ("여기도", "각각") near a named
                // one. A price that names nothing may answer a question
                // about something else ("타일은요?" "평당 8만 원이요"): not
                // compared.
                (Subject::Named(x), Subject::Pointing) if b.also && tied(*line_a, *line_b) => {
                    x.clone()
                }
                (Subject::Pointing, Subject::Named(x)) if a.also && tied(*line_a, *line_b) => {
                    x.clone()
                }
                // Two lengths priced, each only pointed at ("이 3m 48만",
                // "각각 1m씩 32만"), a few lines apart: a wall's price per
                // metre said two ways.
                (Subject::Pointing, Subject::Pointing)
                    if !a.per_word && !b.per_word && b.also && apart <= NEAR_LINES =>
                {
                    String::new()
                }
                // "자당 20 기준으로" then, a line on, "여기도 자당 25만":
                // "this too" is the price just before.
                (Subject::Unsaid, Subject::Pointing)
                    if b.also && apart <= 1 && speakers[*line_a] == speakers[*line_b] =>
                {
                    String::new()
                }
                _ => continue,
            };
            let key = (a.rate.per.clone(), name.clone());
            if told.contains(&key) {
                continue;
            }
            told.push(key);
            found.push(Disagreement {
                kind: DisagreementKind::Rate,
                first: *line_a,
                second: *line_b,
                // The prices per unit, and the numbers as said ("1m씩 32만"
                // gives 1 and 32만), so an entry that copies only a part
                // ("각각 1m씩이다") is found too.
                values: [a.rate.value, b.rate.value]
                    .into_iter()
                    .chain(amounts(&a.said).into_iter().map(|x| x.value))
                    .chain(amounts(&b.said).into_iter().map(|x| x.value))
                    .collect(),
                name: name.clone(),
                note: format!(
                    "{}단가가 앞뒤로 다르게 나옴: {} / {}",
                    if name.is_empty() {
                        String::new()
                    } else {
                        format!("{name} ")
                    },
                    a.said,
                    b.said
                ),
            });
        }
    }
    found
}

/// A price per unit, a count in that unit, and a total after them, in one
/// line: the total should be the price times the count.
fn product_disagreements(lines: &[&str]) -> Vec<Disagreement> {
    let mut found = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let text = super::numbers::spoken_to_digits(line);
        let chars: Vec<char> = text.chars().collect();
        let byte_to_char = |byte: usize| text[..byte].chars().count();
        let found_amounts = amounts(&text);
        for placed in placed_rates(&text)
            .iter()
            .filter(|p| p.per_word && !p.range)
        {
            let rate = &placed.rate;
            for caps in COUNT.captures_iter(&text) {
                let whole = caps.get(0).expect("the whole match");
                let (start, end) = (byte_to_char(whole.start()), byte_to_char(whole.end()));
                if count_unit(&caps[2]) != rate.per
                    || (start < placed.end && end > placed.start)
                    || chars.get(end) == Some(&'당')
                    || !count_ends(&caps[2], chars.get(end).copied())
                {
                    continue;
                }
                let Ok(count) = caps[1].parse::<f64>() else {
                    continue;
                };
                let after = end.max(placed.end);
                // The first amount after both, joined to them by a word
                // that makes it their total.
                let Some(total) = found_amounts.iter().find(|a| a.start >= after) else {
                    continue;
                };
                let between = char_slice(&chars, after, total.start);
                if between.chars().count() > 12
                    || !TOTAL_LINK.is_match(&between)
                    || NOT_A_TOTAL.is_match(&char_slice(&chars, placed.start, total.end))
                {
                    continue;
                }
                let expected = count * rate.value;
                let around = char_slice(&chars, total.start.saturating_sub(4), total.end + 6);
                let said = as_meant(total.value, expected);
                if agrees(total.value, expected, ROUGH.is_match(&around))
                    || unrelated(said, expected)
                {
                    continue;
                }
                found.push(Disagreement {
                    kind: DisagreementKind::Product,
                    name: String::new(),
                    first: at,
                    second: at,
                    values: vec![rate.value, said],
                    note: format!(
                        "{per}당 {} × {count}{per}이면 {}인데 {}으로 말함",
                        won(rate.value),
                        won(expected),
                        won(said),
                        per = rate.per
                    ),
                });
            }
        }
    }
    found
}

/// Amounts said one after another and then their total, in one line: the
/// total should be their sum.
fn sum_disagreements(lines: &[&str]) -> Vec<Disagreement> {
    let mut found = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let byte_to_char = |byte: usize| line[..byte].chars().count();
        let all = amounts(line);
        let rate_spans: Vec<(usize, usize)> = placed_rates(line)
            .iter()
            .map(|p| (p.start, p.end))
            .collect();
        let mut from = 0;
        for word in SUM_WORD.find_iter(line) {
            let (word_start, word_end) = (byte_to_char(word.start()), byte_to_char(word.end()));
            let Some(total) = all
                .iter()
                .find(|a| a.start >= word_end && a.start <= word_end + 8)
            else {
                continue;
            };
            // The parts: money said before the word since the last total,
            // each joined closely to the next, none a price per unit.
            let parts: Vec<&Amount> = all
                .iter()
                .filter(|a| a.start >= from && a.end <= word_start && a.value >= 1e4)
                .filter(|a| !rate_spans.iter().any(|&(s, e)| a.start < e && a.end > s))
                // The total of something else ("아홉 개니까 50만 원") is no part.
                .filter(|a| {
                    !TOTAL_LINK.is_match(&char_slice(&chars, a.start.saturating_sub(5), a.start))
                })
                .collect();
            // An amount said between the parts that is not taken as one (a
            // bare number, a total of something else): the sum cannot be
            // read, so the line is left alone.
            let left_out_between = parts.len() >= 2
                && all.iter().any(|a| {
                    a.start > parts[0].start
                        && a.end <= word_start
                        && !parts.iter().any(|p| p.start == a.start)
                        && !rate_spans.iter().any(|&(s, e)| a.start < e && a.end > s)
                });
            from = total.end;
            if parts.len() < 2 || left_out_between {
                continue;
            }
            let joined = parts.windows(2).all(|pair| {
                let between = char_slice(&chars, pair[0].end, pair[1].start);
                between.chars().count() <= 16 && !between.contains("에서")
            }) && char_slice(&chars, parts[parts.len() - 1].end, word_start)
                .chars()
                .count()
                <= 8;
            if !joined || NOT_A_TOTAL.is_match(&char_slice(&chars, parts[0].start, total.end + 2)) {
                continue;
            }
            let sum: f64 = parts.iter().map(|a| a.value).sum();
            let around = char_slice(&chars, total.start.saturating_sub(4), total.end + 6);
            let said = as_meant(total.value, sum);
            if agrees(total.value, sum, ROUGH.is_match(&around)) || unrelated(said, sum) {
                continue;
            }
            let listed: Vec<String> = parts.iter().map(|a| won(a.value)).collect();
            found.push(Disagreement {
                kind: DisagreementKind::Sum,
                name: String::new(),
                first: at,
                second: at,
                values: parts.iter().map(|a| a.value).chain([said]).collect(),
                note: format!(
                    "{}을 더하면 {}인데 {}으로 말함",
                    listed.join(" + "),
                    won(sum),
                    won(said)
                ),
            });
        }
    }
    found
}

/// A change said as how much ("5,100만 원 늘었다") beside a move from one
/// amount to another a few lines away whose difference is near it but not
/// it. When some move nearby is exactly that much, nothing disagrees.
fn change_disagreements(lines: &[&str]) -> Vec<Disagreement> {
    let all: Vec<Vec<Change>> = lines.iter().map(|line| changes(line)).collect();
    let mut found = Vec::new();
    for (i, own) in all.iter().enumerate() {
        for by in own.iter().filter_map(|c| c.by) {
            let near = i.saturating_sub(CHANGE_LINES)..=(i + CHANGE_LINES).min(lines.len() - 1);
            let moves: Vec<(usize, f64, f64)> = near
                .flat_map(|j| {
                    all[j].iter().filter_map(move |c| match (c.from, c.to) {
                        (Some(from), Some(to)) => Some((j, from, to)),
                        _ => None,
                    })
                })
                .collect();
            if moves
                .iter()
                .any(|&(_, from, to)| same((to - from).abs(), by))
            {
                continue;
            }
            if let Some(&(j, from, to)) = moves
                .iter()
                .filter(|&&(_, from, to)| !unrelated(by, (to - from).abs()))
                .min_by_key(|&&(j, _, _)| j.abs_diff(i))
            {
                found.push(Disagreement {
                    kind: DisagreementKind::Change,
                    name: String::new(),
                    first: i.min(j),
                    second: i.max(j),
                    values: vec![from, to, by],
                    note: format!(
                        "{}에서 {}으로 바뀌면 {} 차이인데 {} 바뀌었다고 말함",
                        won(from),
                        won(to),
                        won((to - from).abs()),
                        won(by)
                    ),
                });
            }
        }
    }
    found
}

/// The numbers of one meeting that disagree with each other, from its lines
/// in order: the same price said two ways, a price times a count that is not
/// the total said, parts that do not add up to their total, a change that
/// does not match the amounts it went between. A plain function of the
/// text: it only says what to doubt and never says which number is right.
/// What only looks alike is left alone: prices of differently named things,
/// two prices neither of which names its thing, amounts far apart (more
/// than twice), and a rough total ("한 500 정도") within a tenth.
pub fn disagreements(lines: &[&str]) -> Vec<Disagreement> {
    disagreements_by(lines, &vec![0; lines.len()])
}

/// `disagreements`, knowing who said each line (`speakers`, one per line):
/// a price that only says "this too" ("여기도 자당 25만") is read with an
/// unnamed price just before only when the same person said both.
pub fn disagreements_by(lines: &[&str], speakers: &[u32]) -> Vec<Disagreement> {
    if lines.is_empty() || speakers.len() != lines.len() {
        return Vec::new();
    }
    let mut found = product_disagreements(lines);
    found.extend(sum_disagreements(lines));
    found.extend(change_disagreements(lines));
    found.extend(rate_disagreements(lines, speakers));
    let mut seen: Vec<(DisagreementKind, usize, usize)> = Vec::new();
    found.retain(|d| {
        let key = (d.kind, d.first, d.second);
        let new = !seen.contains(&key);
        seen.push(key);
        new
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(text: &str) -> Vec<f64> {
        amounts(text).iter().map(|a| a.value).collect()
    }

    #[test]
    fn amounts_join_their_big_units() {
        assert_eq!(values("1억 5,000만 원"), [150_000_000.0]);
        assert_eq!(values("5100만 원 정도"), [51_000_000.0]);
        assert_eq!(
            values("1억 5천에서 2억 1천으로"),
            [150_000_000.0, 210_000_000.0]
        );
        assert_eq!(values("2억 천 나왔다"), [210_000_000.0]);
        assert_eq!(values("3m에 48만 원"), [3.0, 480_000.0]);
        assert_eq!(values("814만 원이 늘고"), [8_140_000.0]);
        // A ceiling is not a thousand.
        assert!(values("천장은 새로").is_empty());
        assert_eq!(values("견적 2억 천장 포함"), [200_000_000.0]);
        assert_eq!(values("2억 천이 나왔다"), [210_000_000.0]);
        // Rough words: a range, and "a little".
        assert_eq!(values("2만 3만 원"), [20_000.0, 30_000.0]);
        assert!(!odd_units("2만 3만 원 정도"));
        assert_eq!(values("100 조금 넘게"), [100.0]);
        // 조 before 원 or a particle is a trillion.
        assert_eq!(values("3조원"), [3e12]);
        assert_eq!(values("3조 원"), [3e12]);
        assert_eq!(values("매출이 3조가 넘는"), [3e12]);
        assert_eq!(values("2조 5천억 원"), [2.5e12]);
        assert!(!odd_units("3조원"));
        assert_eq!(values("100 조정 필요"), [100.0]);
    }

    #[test]
    fn odd_amounts_are_found() {
        assert!(odd_units("전체 금액이 1억 5천억이라고"));
        assert!(odd_units("80,900만원 정도 더"));
        assert!(odd_units("1억 5천원에서 2억 천원으로"));
        assert!(!odd_units("1억 5천만 원에서 2억 1천만 원으로"));
        assert!(!odd_units("1,500만 원"));
        assert!(!odd_units("1억 5천에서 2억 1천으로"));
    }

    #[test]
    fn a_number_said_in_words_backs_the_digits() {
        assert!(backed("천장 9평", "천장은 아홉 평 정도 새로 합니다"));
        assert!(backed("1개소 추가", "욕실 한 개소 추가"));
        assert!(backed("총 1억 5천만 원", "러프 견적이 1억 5천이었죠"));
        assert!(backed("2억 1천만 원", "다시 뽑으니 2억 천이 나왔습니다"));
        assert!(backed("22곳 중 12군데", "22곳 중 12군데가"));
        assert!(!backed("천장 8평", "천장은 아홉 평 정도 새로 합니다"));
        assert!(!backed("각각 2m씩 32만 원", "각각 1m씩 32만 원입니다"));
    }

    #[test]
    fn a_rate_said_two_ways_is_found() {
        assert!(rates_differ(
            "복도 끝 수납장은 자당 20",
            "복도 끝은 자당 20 기준으로, 서재도 여기도 자당 25만 원입니다"
        ));
        assert!(rates_differ(
            "문 두 곳을 막는 벽은 각각 1m씩 32만 원",
            "벽체는 3m에 48만 원이고 문 두 곳을 막는 벽은 각각 1m씩 32만 원입니다"
        ));
        assert!(!rates_differ(
            "벽체는 3m에 48만 원",
            "벽체는 3m에 48만 원, 6m면 96만 원"
        ));
        // Two things priced differently are two prices.
        assert!(!rates_differ(
            "강마루 평당 12만",
            "강마루 평당 12만, 원목마루 평당 18만"
        ));
        // What a price is of may come after it.
        assert!(!rates_differ(
            "평당 12만 원짜리 강마루",
            "평당 12만 원짜리 강마루, 평당 18만 원짜리 원목마루"
        ));
        assert!(rates_differ(
            "평당 12만 원짜리 강마루",
            "강마루는 평당 15만이라고 다시 말씀드렸어요"
        ));
        // A price no word says the thing of is not compared.
        assert!(!rates_differ("평당 12만 원", "평당 12만 원, 평당 18만 원"));
        assert_eq!(rates("평당 12만 원 기준으로")[0].what, Subject::Unsaid);
        // One side named, the other not: compared.
        assert!(rates_differ("강마루 평당 12만", "평당 15만 원이요"));
        // Money itself is no thing a price is of.
        assert_eq!(rates("평당 12만 원의 차이가 나요")[0].what, Subject::Unsaid);
        assert_eq!(rates("평당 12만 원의 견적")[0].what, Subject::Unsaid);
        assert_eq!(
            rates("평당 12만 원의 강마루")[0].what,
            Subject::Named("강마루".into())
        );
        // Millimetres are no metres.
        assert!(rates("1200mm 상판 80만").is_empty());
        assert!(rates("120cm 선반 30만").is_empty());
    }

    #[test]
    fn a_change_that_does_not_add_up_is_found() {
        let texts = [
            "견적이 1억 5천에서 2억 1천으로 올랐다",
            "5,100만 원 정도 늘었다",
            "원목마루는 814만 원이 늘어난다",
        ];
        assert_eq!(sums_disagree(&texts), [0, 1]);
        let right = ["견적이 1억 5천에서 2억 1천으로 올랐다", "6,000만 원 늘었다"];
        assert!(sums_disagree(&right).is_empty());
        let one = ["1억 5천에서 2억 1천으로 5100만 원 정도 늘었다"];
        assert_eq!(sums_disagree(&one), [0]);
    }

    fn kinds(lines: &[&str]) -> Vec<(DisagreementKind, usize, usize)> {
        disagreements(lines)
            .iter()
            .map(|d| (d.kind, d.first, d.second))
            .collect()
    }

    #[test]
    fn amounts_read_as_people_say_them() {
        assert_eq!(won(210_000_000.0), "2억 1,000만 원");
        assert_eq!(won(200_000_000.0), "2억 원");
        assert_eq!(won(450_000.0), "45만 원");
        assert_eq!(won(60_000_000.0), "6,000만 원");
        assert_eq!(won(165_000.0), "16.5만 원");
        assert_eq!(won(9_000.0), "9,000원");
    }

    #[test]
    fn a_price_times_a_count_that_is_not_its_total_is_found() {
        let found = disagreements(&["서랍은 개당 5만 원이고 아홉 개니까 50만 원입니다"]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, DisagreementKind::Product);
        assert_eq!(
            found[0].note,
            "개당 5만 원 × 9개이면 45만 원인데 50만 원으로 말함"
        );
        // Right, or rough within a tenth, or said with digits: nothing.
        assert!(disagreements(&["타일은 평당 8만 원이고 10평이라 80만 원 들어갑니다"]).is_empty());
        assert!(disagreements(&["수납장은 자당 25만, 12자니까 300만 원입니다"]).is_empty());
        assert!(disagreements(&["자당 25만이고 12자면 한 290 정도"]).is_empty());
        // A total far from it is something else.
        assert!(disagreements(&["개당 5만 원이고 9개니까 전체 공사는 2천만 원이에요"]).is_empty());
        // A count in another unit is not a count of what is priced.
        assert!(disagreements(&["평당 8만 원이고 3개니까 30만 원"]).is_empty());
    }

    #[test]
    fn parts_that_do_not_add_up_to_their_total_are_found() {
        let found = disagreements(&["철거 300만 원, 목공 450만 원, 합쳐서 800만 원입니다"]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, DisagreementKind::Sum);
        assert_eq!(
            found[0].note,
            "300만 원 + 450만 원을 더하면 750만 원인데 800만 원으로 말함"
        );
        assert!(disagreements(&["철거 300만 원, 목공 450만 원, 합쳐서 750만 원입니다"]).is_empty());
        // "한 500 정도" is a rough total of 510.
        assert!(
            disagreements(&["도배 280만 원 하고 장판 230만 원 해서 한 500 정도 됩니다"]).is_empty()
        );
        // A move from one amount to another is no sum.
        assert!(disagreements(&["1억 5천에서 2억 1천으로 해서 총 2억 1천"]).is_empty());
    }

    #[test]
    fn a_change_that_does_not_match_its_amounts_is_found_lines_apart() {
        let lines = [
            "지난번 러프 견적이 1억 5천이었죠",
            "이번에는 1억 5천에서 2억 1천으로 올라갔습니다",
            "네",
            "그러니까 5100만 원 정도 늘어난 겁니다",
        ];
        assert_eq!(kinds(&lines), [(DisagreementKind::Change, 1, 3)]);
        let right = [
            "1억 5천에서 2억 1천으로 올라갔습니다",
            "6,000만 원 늘었습니다",
        ];
        assert!(disagreements(&right).is_empty());
        // Another change of another size is about something else.
        let other = [
            "1억 5천에서 2억 1천으로 올라갔습니다",
            "원목마루는 814만 원이 늘어납니다",
        ];
        assert!(disagreements(&other).is_empty());
        // Too far apart to be about one amount.
        let mut far = vec!["1억 5천에서 2억 1천으로 올라갔습니다"];
        far.extend(["네"; 8]);
        far.push("5100만 원 늘었습니다");
        assert!(disagreements(&far).is_empty());
    }

    #[test]
    fn one_thing_priced_two_ways_in_a_meeting_is_found_once() {
        let mut lines = vec!["바닥 강마루는 평당 12만 원으로 잡았습니다"];
        lines.extend(["네"; 20]);
        lines.push("강마루는 평당 15만 원 기준이고요");
        lines.push("강마루는 평당 16만 원이면 좋겠네요");
        assert_eq!(kinds(&lines), [(DisagreementKind::Rate, 0, 21)]);
        assert_eq!(
            disagreements(&lines)[0].note,
            "강마루 단가가 앞뒤로 다르게 나옴: 평당 12만 원 / 평당 15만 원"
        );
        // A price in won is quoted as said, never read as 만.
        let won_price = [
            "손잡이는 개당 3000원입니다",
            "손잡이는 개당 3500원으로 올랐어요",
        ];
        assert_eq!(
            disagreements(&won_price)[0].note,
            "손잡이 단가가 앞뒤로 다르게 나옴: 개당 3000원 / 개당 3500원"
        );
        assert_eq!(rates("개당 3000원")[0].value, 3000.0);
        assert_eq!(rates("평당 120,000원")[0].value, 120_000.0);
        // Two lengths priced, both only pointed at, near each other.
        let walls = [
            "이 3m 48만원에 대한 비용은 벽을 만드는 거고요",
            "네",
            "문을 막는 건 각각 1m씩 해가지고 32만원이 들어가 있습니다",
        ];
        assert_eq!(kinds(&walls), [(DisagreementKind::Rate, 0, 2)]);
        // A length and its price beside a pointing one: per metre.
        let wall = ["벽체는 3m에 48만 원이고요, 문 두 곳을 막는 벽은 각각 1m씩 32만 원입니다"];
        assert_eq!(kinds(&wall), [(DisagreementKind::Rate, 0, 0)]);
        // "자당 20" beside "여기도 자당 25만".
        let shelf = [
            "복도 끝 수납장은 자당 20 기준으로 잡았고요",
            "서재 쪽도 여기도 자당 25만 원입니다",
        ];
        assert_eq!(kinds(&shelf), [(DisagreementKind::Rate, 0, 1)]);
        // A price no word names, then "여기도" with a filler word between.
        let also = [
            "자당 20 기준으로 견적을 넣었습니다",
            "그래서 여기도 이제 자당 25만 원 기준으로 안내드렸어요",
        ];
        assert_eq!(
            disagreements(&also)[0].note,
            "단가가 앞뒤로 다르게 나옴: 자당 20 / 자당 25만 원"
        );
    }

    #[test]
    fn what_only_looks_like_a_disagreement_is_left_alone() {
        // A word before the price that says how or when, not what of.
        let mut usual = vec!["바닥은 보통 평당 12만 원 정도 합니다"];
        usual.extend(["네"; 30]);
        usual.push("전체 공사는 보통 평당 300만 원");
        assert!(disagreements(&usual).is_empty());
        assert!(disagreements(&["지금 평당 12만 원이고요", "지금 평당 8만 원이에요"]).is_empty());
        // An answer that leaves its subject out.
        assert!(disagreements(&[
            "강마루는 평당 12만 원입니다",
            "타일은요?",
            "평당 8만 원이요"
        ])
        .is_empty());
        // A range, then a choice inside it.
        assert!(disagreements(&[
            "강마루는 평당 12만에서 15만 사이예요",
            "강마루는 평당 15만 원으로 하죠"
        ])
        .is_empty());
        // A total that includes something else, a discount, a payment.
        assert!(
            disagreements(&["타일은 평당 8만이고 30평이면 시공비 포함해서 300만 원입니다"])
                .is_empty()
        );
        assert!(
            disagreements(&["철거 300만 원 목공 450만 원 해서 깎아서 700만 원에 해드릴게요"])
                .is_empty()
        );
        assert!(
            disagreements(&["계약금 300만 원, 중도금 450만 원 해서 잔금은 800만 원"]).is_empty()
        );
        // Months are no count of pieces; millimetres no metres.
        assert!(disagreements(&["개당 5만 원이고 3개월이면 20만 원"]).is_empty());
        // A range of a budget is no move, and "줄 수 있어요" no change.
        assert!(disagreements(&[
            "예산은 3천만 원에서 5천만 원 사이로 생각해요",
            "네",
            "자재가 1500만 원 정도 올랐어요"
        ])
        .is_empty());
        assert!(changes("800만 원 줄 수 있어요").is_empty());
        assert!(disagreements(&[
            "예산은 3천만 원에서 5천만 원 정도로 생각하고 있어요",
            "네",
            "자재가 1500만 원 정도 올랐어요"
        ])
        .is_empty());
        // A part said after a link word is still a part: not summed wrong.
        assert!(
            disagreements(&["철거 300만, 목공이면 200만, 전기 300만 합쳐서 800만 원입니다"])
                .is_empty()
        );
        // Words that point, or name only money or who speaks, are no thing.
        for (a, b) in [
            ("이건 평당 12만 원이고요", "이건 평당 15만 원입니다"),
            ("그거는 평당 12만 원 정도", "그거는 평당 9만 원이에요"),
            ("강마루 단가는 평당 12만 원", "타일 단가는 평당 9만 원"),
            ("강마루 자재는 평당 12만 원", "타일 자재는 평당 9만 원"),
            ("저희는 평당 12만 원 받아요", "저희는 평당 20만 원에 했어요"),
        ] {
            let mut far = vec![a];
            far.extend(["네"; 30]);
            far.push(b);
            assert!(disagreements(&far).is_empty(), "{a} / {b}");
        }
        // Bound nouns and joints name nothing; the word before may.
        for (a, b) in [
            (
                "강마루 같은 경우는 평당 12만 원",
                "타일 같은 경우는 평당 9만 원",
            ),
            ("거실 쪽은 평당 12만 원", "주방 쪽은 평당 9만 원"),
            (
                "강마루 하는 거는 평당 12만 원",
                "타일 하는 거는 평당 9만 원",
            ),
            ("강마루 깔 때는 평당 12만 원", "타일 깔 때는 평당 9만 원"),
            ("이쪽 부분은 평당 12만 원", "저쪽 부분은 평당 9만 원"),
            ("강마루 기준으로 평당 12만 원", "타일 기준으로 평당 9만 원"),
            (
                "그랬는데 저희는 평당 12만 원",
                "그랬는데 저희는 평당 20만 원",
            ),
        ] {
            let mut far = vec![a];
            far.extend(["네"; 30]);
            far.push(b);
            assert!(disagreements(&far).is_empty(), "{a} / {b}");
        }
        // "그건", "이건" near a named price may point at another thing.
        assert!(disagreements(&[
            "강마루는 평당 12만 원입니다",
            "원목마루는요?",
            "그건 평당 18만 원입니다"
        ])
        .is_empty());
        assert!(
            disagreements(&["강마루는 평당 12만 원이고요", "이건 평당 9만 원이에요"]).is_empty()
        );
        // Two pieces each pointed at, neither "each": two things.
        assert!(disagreements(&[
            "이 2m 30만 원짜리 장이고요",
            "네",
            "저 3m 60만 원짜리는 붙박이입니다"
        ])
        .is_empty());
        // "여기도" after an unnamed price someone else said: not read together.
        assert!(disagreements_by(
            &["평당 8만 원이요", "여기도 평당 12만 원 들어가요"],
            &[2, 1]
        )
        .is_empty());
        assert_eq!(
            disagreements_by(
                &["평당 8만 원이요", "여기도 평당 12만 원 들어가요"],
                &[1, 1]
            )
            .len(),
            1
        );
        // Verbs, joints and adverbs before the price, far apart: no name.
        for (a, b) in [
            ("강마루로 깔면 평당 12만 원", "타일로 깔면 평당 9만 원"),
            (
                "강마루로 하시면 보통 평당 12만 원",
                "타일로 하시면 보통 평당 9만 원",
            ),
            ("그렇게 되면 평당 12만 원", "그렇게 되면 평당 9만 원"),
            ("평수로 치면 평당 12만 원", "평수로 치면 평당 9만 원"),
            (
                "강마루가 들어가고 평당 12만 원",
                "타일이 들어가고 평당 9만 원",
            ),
            ("다 합쳐서 평당 12만 원", "다 합쳐서 평당 9만 원"),
            (
                "강마루는 해가지고 평당 12만 원",
                "타일은 해가지고 평당 9만 원",
            ),
            ("그런데 평당 12만 원", "그런데 평당 9만 원"),
            ("그것 때문에 평당 12만 원", "그것 때문에 평당 9만 원"),
            ("강마루는 아마 평당 12만 원", "타일은 아마 평당 9만 원"),
            (
                "강마루는 싸게 해서 평당 12만 원",
                "타일은 싸게 해서 평당 9만 원",
            ),
            (
                "강마루는 대략 잡아도 평당 12만 원",
                "타일은 대략 잡아도 평당 9만 원",
            ),
        ] {
            let mut far = vec![a];
            far.extend(["네"; 30]);
            far.push(b);
            assert!(disagreements(&far).is_empty(), "{a} / {b}");
        }
        // Options priced one after another, each after a verb.
        assert!(disagreements(&[
            "강마루로 가면 평당 12만 원이고요",
            "원목으로 가면 평당 18만 원이에요"
        ])
        .is_empty());
        assert!(disagreements(&[
            "강마루로 바꾸면 평당 12만 원",
            "네",
            "타일로 바꾸면 평당 9만 원"
        ])
        .is_empty());
        assert!(disagreements(&[
            "그런데 평당 12만 원이에요",
            "네",
            "그런데 평당 9만 원이에요"
        ])
        .is_empty());
        // An adverb with 도, a verb before a noun, a time with 에는.
        for (a, b) in [
            ("강마루는 아마도 평당 12만 원", "타일은 아마도 평당 9만 원"),
            ("강마루는 그래도 평당 12만 원", "타일은 그래도 평당 9만 원"),
            ("강마루 들어가는 평당 12만 원", "타일 들어가는 평당 9만 원"),
            ("이번에는 평당 200만 원", "이번에는 평당 250만 원"),
        ] {
            let mut far = vec![a];
            far.extend(["네"; 30]);
            far.push(b);
            assert!(disagreements(&far).is_empty(), "{a} / {b}");
        }
        // Samples compared side by side by counting or picking words.
        for (a, b) in [
            ("하나는 평당 12만 원이고요", "하나는 평당 15만 원이에요"),
            (
                "첫 번째는 평당 12만 원이고요",
                "두 번째는 평당 15만 원이에요",
            ),
            ("이 제품은 평당 12만 원", "저 제품은 평당 15만 원"),
            ("다른 거는 평당 12만 원", "다른 거는 평당 15만 원"),
            ("나머지는 평당 12만 원", "나머지는 평당 15만 원"),
        ] {
            assert!(disagreements(&[a, "네", b]).is_empty(), "{a} / {b}");
        }
        // Nouns ending like verbs still name: 벽지 priced two ways is found,
        // and 벽지 beside 장판 is two things to the entry-level check too.
        let mut paper = vec!["벽지는 평당 2만 원입니다"];
        paper.extend(["네"; 10]);
        paper.push("벽지는 평당 3만 원으로 올랐어요");
        assert_eq!(disagreements(&paper).len(), 1);
        assert!(!rates_differ(
            "벽지는 평당 2만 원",
            "벽지는 평당 2만 원이고 장판은 평당 3만 원입니다"
        ));
        assert!(!rates_differ(
            "베란다는 평당 20만 원",
            "거실은 평당 12만 원"
        ));
        // "거기도" after a question about another thing is not tied.
        assert!(disagreements(&[
            "강마루는 평당 12만 원입니다",
            "타일은 얼마예요",
            "거기도 평당 9만 원 정도예요"
        ])
        .is_empty());
        // The thing before "단가" is the name.
        let mut same = vec!["강마루 단가는 평당 12만 원"];
        same.extend(["네"; 30]);
        same.push("강마루 단가는 평당 15만 원");
        assert_eq!(disagreements(&same).len(), 1);
        assert_eq!(changes("800만 원 줄었어요").len(), 1);
        // Two different disagreements in one line are both told.
        let both = disagreements(&[
            "서랍은 개당 5만 원이고 아홉 개니까 50만 원, 철거 300만 원, 목공 450만 원, 합쳐서 800만 원입니다",
        ]);
        assert_eq!(
            both.iter().map(|d| d.kind).collect::<Vec<_>>(),
            [DisagreementKind::Product, DisagreementKind::Sum]
        );
    }

    #[test]
    fn prices_that_only_look_alike_are_left_alone() {
        // Differently named things.
        assert!(disagreements(&["강마루 평당 12만, 원목마루 평당 18만"]).is_empty());
        // A pointing price far from the named one.
        let mut far = vec!["강마루는 평당 12만 원"];
        far.extend(["네"; 10]);
        far.push("여기도 평당 15만 원");
        assert!(disagreements(&far).is_empty());
        // Two prices that name nothing.
        assert!(disagreements(&["평당 12만 원", "평당 18만 원"]).is_empty());
        // The same price twice, and a length priced the same way again.
        assert!(disagreements(&["강마루 평당 12만", "강마루는 평당 12만 원"]).is_empty());
        assert!(disagreements(&["벽체는 3m에 48만 원, 6m면 96만 원"]).is_empty());
        // Two figures for two things: material, and material with labour.
        assert!(disagreements(&[
            "원목마루는 자재만 814만 원 늘고, 시공비까지 하면 900만 원 정도 더 올라갑니다"
        ])
        .is_empty());
    }
}
