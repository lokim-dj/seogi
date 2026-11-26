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
        let big_unit = caps.get(4).map_or("", |m| m.as_str());
        // A bare 천 or 백 goes on a chain, never starts one, and is no
        // number when a word goes on after it ("2억, 천장 포함"); a plain
        // number after a chain is a number of its own ("3m에 48만").
        if (first && bare) || (!first && sub_unit.is_empty() && big_unit.is_empty()) {
            break;
        }
        if bare && big_unit.is_empty() {
            const AFTER: &str = "원만이을은에으로정쯤가까짜대선";
            let next = text[pos + whole.as_str().trim_end().len()..].chars().next();
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
        end = pos + whole.as_str().trim_end().len();
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
    Regex::new(r"([가-힣a-zA-Z]{1,3})당\s?(\d[\d,]*(?:\.\d+)?)\s?(만|천)?")
        .expect("a valid pattern")
});
static LENGTH_PRICE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d+(?:\.\d+)?)\s?(?:m|미터)\s?(?:에|씩|짜리|당)?[^\d.]{0,12}?(\d[\d,]*)\s?만")
        .expect("a valid pattern")
});

/// The rates `text` gives: (what it is per, value per one).
pub fn rates(text: &str) -> Vec<(String, f64)> {
    let mut found = Vec::new();
    for caps in PER.captures_iter(text) {
        let Ok(number) = caps[2].replace(',', "").parse::<f64>() else {
            continue;
        };
        // "자당 20" and "자당 25만" are compared as said: a bare number is
        // the same scale as the others are.
        let scale = match caps.get(3).map(|m| m.as_str()) {
            Some("만") => 1e4,
            Some("천") => 1e3,
            _ => 0.0,
        };
        found.push((
            caps[1].to_string(),
            if scale > 0.0 {
                number * scale
            } else {
                number * 1e4
            },
        ));
    }
    for caps in LENGTH_PRICE.captures_iter(text) {
        let (Ok(length), Ok(price)) = (
            caps[1].parse::<f64>(),
            caps[2].replace(',', "").parse::<f64>(),
        ) else {
            continue;
        };
        if length > 0.0 {
            found.push(("m".to_string(), price * 1e4 / length));
        }
    }
    found
}

/// Whether a rate `stated` gives is said with another value nearby in
/// `around`: the same thing priced two ways.
pub fn rates_differ(stated: &str, around: &str) -> bool {
    let near = rates(around);
    rates(stated).iter().any(|(per, value)| {
        near.iter()
            .any(|(other_per, other)| other_per == per && !same(*other, *value))
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
static BY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:원)?\s*(?:정도|가량|쯤)?\s*(?:이|가)?\s*(?:더\s*)?(늘|증가|올라|올랐|상승|많아|줄|감소|내려|낮아|빠)")
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
            if FROM_TO.is_match(&between) && next.value >= 1e4 {
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
}
