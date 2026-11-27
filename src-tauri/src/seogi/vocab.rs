//! The user's own vocabulary: names and terms a speech model gets wrong.
//!
//! It lives in a file or folder the user chooses, never in the app or this
//! repository. Two things come out of it: hint terms given to the speech
//! model with every chunk, and corrections applied to the text it returns.
//!
//! Accepted forms:
//! - a text file: one term per line (or several separated by commas), `#`
//!   starts a comment, and `틀린 말 => 바른 말` is a correction (a way the
//!   speech model mishears the word). A correction added from a meeting
//!   carries when and where in its comment,
//!   `틀린 말 => 바른 말  # 2026-11-27 「회의 제목」`: only a comment, so
//!   every version of the app reads the file the same;
//! - a JSON file: a list of terms, or of entries like
//!   `{"term": "걸레받이", "category": "용어", "aliases": [...],
//!   "common_mishearings": [...], "freq": 12}`, bare or under an `entries` /
//!   `terms` / `vocabulary` key;
//! - a folder holding `hints_core.txt` (the terms to hint first, in the text
//!   form) and `vocabulary.json`. Nothing else in the folder is read.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

/// What a new user's vocabulary file starts as: generic interior terms and a
/// sample correction, so the format explains itself.
pub const EXAMPLE_FILE: &str = "\
# 서기 용어집
# 한 줄에 하나씩 적습니다. 자주 틀리는 이름·현장 이름·전문 용어를 넣어 두면
# 받아쓸 때 모든 구간에 힌트로 쓰입니다. 위에 있을수록 먼저 쓰입니다.
# '#' 로 시작하는 줄은 설명입니다.
#
# \"틀린 말 => 바른 말\" 로 적으면 받아쓴 글에서 바꿔 줍니다.
# 예) 걸래받이 => 걸레받이

걸레받이
몰딩
중문
도배
마루
타일
줄눈
실리콘
샷시
붙박이장
상부장
하부장
아트월
우물천장
간접등
SMC
이노솔

# 받아쓰기 모델이 자주 잘못 듣는 말
인오솔 => 이노솔
";

/// Categories hinted only for a recording whose own information (its
/// calendar event, title, attendees) mentions the entry. A long list of
/// customer names would crowd the real terms out of the model's small
/// prompt, and a short name can be an ordinary word.
const MENTION_ONLY: &[&str] = &["고객", "customer"];
/// Categories hinted ahead of the rest: the people who are in most meetings.
const PEOPLE_FIRST: &[&str] = &["직원", "staff"];

/// A categorised entry of the vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub term: String,
    /// The user's own grouping (`용어`, `자재`, `직원`, `고객`, ...); empty
    /// when the vocabulary has none.
    pub category: String,
    /// Other right ways to write it.
    pub aliases: Vec<String>,
}

/// A change the vocabulary made to transcribed text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Correction {
    pub from: String,
    pub to: String,
}

/// One place a correction applies: `text[start..end]` is `from`, to be
/// read as `to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub from: String,
    pub to: String,
}

/// What can follow a noun and still leave it the same word: particles and
/// the commonest endings of "이다". A short wrong form is corrected only
/// when it stands alone or is followed by these, because two letters are
/// too often the start of a different, real word.
const PARTICLES: &[&str] = &[
    "으로",
    "에서",
    "에게",
    "한테",
    "께서",
    "까지",
    "부터",
    "마다",
    "처럼",
    "보다",
    "하고",
    "이랑",
    "이나",
    "이든",
    "이라",
    "라고",
    "이고",
    "이며",
    "인데",
    "이면",
    "이요",
    "이죠",
    "이네요",
    "입니다",
    "입니까",
    "이에요",
    "예요",
    "였",
    "이었",
    "습니다",
    "는데",
    "은",
    "는",
    "이",
    "가",
    "을",
    "를",
    "의",
    "에",
    "께",
    "로",
    "와",
    "과",
    "랑",
    "도",
    "만",
    "나",
    "든",
    "고",
    "며",
    "면",
    "요",
    "죠",
    "야",
    "님",
    "씨",
    "들",
];

/// Whether `rest` (what follows a word, up to the next space or mark) is
/// nothing but particles.
fn only_particles(rest: &str) -> bool {
    fn walk(rest: &str, depth: u32) -> bool {
        if rest.is_empty() {
            return true;
        }
        depth < 4
            && PARTICLES.iter().any(|particle| {
                rest.strip_prefix(particle)
                    .is_some_and(|more| walk(more, depth + 1))
            })
    }
    walk(rest, 0)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Vocabulary {
    /// The curated terms, hinted first, most important first.
    pub core: Vec<String>,
    /// Categorised entries, most frequent first.
    pub entries: Vec<Entry>,
    /// `(wrong, right)` pairs applied to transcribed text, longest wrong form
    /// first.
    pub corrections: Vec<(String, String)>,
    /// Every right form: the core terms, the entries and their aliases.
    known: HashSet<String>,
}

impl Vocabulary {
    /// Reads the vocabulary at `path`, a file or a folder. A path that does
    /// not exist is an empty vocabulary: the user may not have made one yet.
    pub fn load(path: &Path) -> Result<Self> {
        let mut vocabulary = Vocabulary::default();
        if path.is_dir() {
            let core = path.join("hints_core.txt");
            if core.is_file() {
                vocabulary.add_text(&read(&core)?);
            }
            let json = path.join("vocabulary.json");
            if json.is_file() {
                vocabulary.add_json(&read(&json)?)?;
            }
        } else if path.is_file() {
            let text = read(path)?;
            if path.extension().is_some_and(|ext| ext == "json") {
                vocabulary.add_json(&text)?;
            } else {
                vocabulary.add_text(&text);
            }
        }
        vocabulary.finish();
        Ok(vocabulary)
    }

    /// Whether `term` is in the vocabulary already, as a term or an alias.
    pub fn knows(&self, term: &str) -> bool {
        self.known.contains(term.trim())
    }

    /// A vocabulary of plain terms, all of them core.
    pub fn from_terms(terms: impl IntoIterator<Item = String>) -> Self {
        let mut vocabulary = Vocabulary {
            core: terms.into_iter().collect(),
            ..Default::default()
        };
        vocabulary.finish();
        vocabulary
    }

    /// Adds plain terms after the core ones.
    pub fn add_terms(&mut self, terms: impl IntoIterator<Item = String>) {
        self.core.extend(terms);
        self.finish();
    }

    fn add_text(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if let Some((wrong, right)) = line.split_once("=>") {
                let (wrong, right) = (wrong.trim(), right.trim());
                if !wrong.is_empty() && !right.is_empty() && wrong != right {
                    self.corrections
                        .push((wrong.to_string(), right.to_string()));
                    self.core.push(right.to_string());
                }
                continue;
            }
            self.core.extend(
                line.split([',', '，', '、'])
                    .map(str::trim)
                    .filter(|term| !term.is_empty())
                    .map(str::to_string),
            );
        }
    }

    fn add_json(&mut self, text: &str) -> Result<()> {
        let value: Value =
            serde_json::from_str(text).context("the vocabulary JSON is not valid")?;
        let entries = match &value {
            Value::Array(entries) => entries.as_slice(),
            Value::Object(map) => ["entries", "terms", "vocabulary", "items"]
                .iter()
                .find_map(|key| map.get(*key).and_then(Value::as_array))
                .map_or(&[][..], Vec::as_slice),
            _ => &[],
        };

        // Entries that say how often they come up go first.
        let mut ranked: Vec<(f64, usize, &Value)> = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let freq = entry
                    .get("freq")
                    .or_else(|| entry.get("frequency"))
                    .or_else(|| entry.get("count"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                (freq, index, entry)
            })
            .collect();
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));

        for (_, _, entry) in ranked {
            let term = match entry {
                Value::String(term) => term.trim(),
                Value::Object(map) => map
                    .get("term")
                    .or_else(|| map.get("word"))
                    .or_else(|| map.get("name"))
                    .and_then(Value::as_str)
                    .map_or("", str::trim),
                _ => "",
            };
            if term.is_empty() {
                continue;
            }
            for wrong in strings(entry, &["common_mishearings", "mishearings", "wrong"]) {
                if wrong != term {
                    self.corrections.push((wrong, term.to_string()));
                }
            }
            self.entries.push(Entry {
                term: term.to_string(),
                category: entry
                    .get("category")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                aliases: strings(entry, &["aliases", "alias"]),
            });
        }
        Ok(())
    }

    /// Drops repeats (keeping the first, so priority holds) and orders
    /// corrections so a longer wrong form is replaced before a shorter one
    /// inside it.
    fn finish(&mut self) {
        let mut seen = HashSet::new();
        self.core.retain(|term| seen.insert(term.clone()));
        let mut seen = HashSet::new();
        self.entries.retain(|entry| seen.insert(entry.term.clone()));
        self.known = self
            .core
            .iter()
            .cloned()
            .chain(self.entries.iter().flat_map(|entry| {
                std::iter::once(entry.term.clone()).chain(entry.aliases.iter().cloned())
            }))
            .collect();
        let mut seen = HashSet::new();
        let known = &self.known;
        // A wrong form of one character would rewrite half the transcript,
        // and one that is itself a right form somewhere is not wrong.
        self.corrections.retain(|pair| {
            pair.0.chars().count() >= 2 && !known.contains(&pair.0) && seen.insert(pair.clone())
        });
        self.corrections
            .sort_by_key(|(wrong, _)| std::cmp::Reverse(wrong.chars().count()));
    }

    /// The hint terms for one recording, at most `limit`, in the order the
    /// speech model should keep them (it drops from the end when its prompt
    /// is full):
    ///
    /// 1. names the recording's own information gives (`mentions`: its title,
    ///    attendees, calendar event), and vocabulary entries those mention;
    /// 2. the core list;
    /// 3. people who are in most meetings (staff);
    /// 4. the other entries by frequency, except the kinds hinted only when
    ///    mentioned (customers).
    pub fn hints(&self, mentions: &[String], limit: usize) -> Vec<String> {
        let mention_text = mentions.join(" ");
        let mentioned = |entry: &Entry| {
            std::iter::once(&entry.term)
                .chain(entry.aliases.iter())
                .any(|form| form.chars().count() >= 2 && mention_text.contains(form.as_str()))
        };
        let in_kinds = |entry: &Entry, kinds: &[&str]| kinds.contains(&entry.category.as_str());

        let from_mentions = mentions
            .iter()
            // A name is a term; a whole title is not. Titles reach the hints
            // through the entries they mention.
            .filter(|mention| {
                mention.chars().count() <= 12
                    && !mention.contains(['[', ']', '/', ':', ',', '(', ')'])
            })
            .map(String::as_str);
        let by_mention = self
            .entries
            .iter()
            .filter(|entry| mentioned(entry))
            .map(|entry| entry.term.as_str());
        let core = self.core.iter().map(String::as_str);
        let people = self
            .entries
            .iter()
            .filter(|entry| in_kinds(entry, PEOPLE_FIRST))
            .map(|entry| entry.term.as_str());
        let rest = self
            .entries
            .iter()
            .filter(|entry| !in_kinds(entry, MENTION_ONLY) && !in_kinds(entry, PEOPLE_FIRST))
            .map(|entry| entry.term.as_str());

        let mut seen = HashSet::new();
        from_mentions
            .chain(by_mention)
            .chain(core)
            .chain(people)
            .chain(rest)
            .map(str::trim)
            .filter(|term| !term.is_empty() && seen.insert(term.to_string()))
            .take(limit)
            .map(str::to_string)
            .collect()
    }

    /// Applies the corrections to transcribed text, and says what it changed,
    /// once for every place.
    pub fn correct(&self, text: &str) -> (String, Vec<Correction>) {
        let spans = self.spans(text);
        let mut out = String::with_capacity(text.len());
        let mut from = 0;
        for span in &spans {
            out.push_str(&text[from..span.start]);
            out.push_str(&span.to);
            from = span.end;
        }
        out.push_str(&text[from..]);
        let made = spans
            .into_iter()
            .map(|span| Correction {
                from: span.from,
                to: span.to,
            })
            .collect();
        (out, made)
    }

    /// Where the corrections apply in `text`, in order and never
    /// overlapping, as byte ranges of `text` itself: a longer wrong form is
    /// placed before a shorter one inside it, and what is put in is never
    /// looked at again.
    ///
    /// A correction must never turn a real word into a wrong one, so a wrong
    /// form is replaced only:
    /// - where a word starts (Korean attaches particles to the end of a word,
    ///   never to its start, so `걸레바지는` is corrected while the same
    ///   letters in the middle of another word are left alone);
    /// - when the word it starts is not a longer term the vocabulary knows;
    /// - and, for a wrong form of two letters, when the word ends there or
    ///   goes on with particles only.
    pub fn spans(&self, text: &str) -> Vec<Span> {
        let mut spans: Vec<Span> = Vec::new();
        for (wrong, right) in &self.corrections {
            let short = wrong.chars().count() <= 2;
            for (at, _) in text.match_indices(wrong.as_str()) {
                let end = at + wrong.len();
                if spans.iter().any(|span| at < span.end && span.start < end) {
                    continue;
                }
                let before = text[..at].chars().next_back();
                let after = &text[end..];
                // The rest of the word the wrong form starts.
                let tail: &str = after
                    .find(|c: char| !c.is_alphanumeric())
                    .map_or(after, |stop| &after[..stop]);
                let at_word_start = before.is_none_or(|c| !c.is_alphanumeric());
                let is_known_word = tail
                    .char_indices()
                    .any(|(index, c)| self.known.contains(&text[at..end + index + c.len_utf8()]));
                if at_word_start && !is_known_word && (!short || only_particles(tail)) {
                    spans.push(Span {
                        start: at,
                        end,
                        from: wrong.clone(),
                        to: right.clone(),
                    });
                }
            }
        }
        spans.sort_by_key(|span| span.start);
        spans
    }

    /// A vocabulary that corrects only `wrong` to `right`, knowing the right
    /// forms `known` knows (so a longer word of the user's is left alone).
    /// Empty of corrections when `wrong` is too short or is itself a right
    /// form, the same as in the user's vocabulary.
    pub fn one_correction(wrong: &str, right: &str, known: &Vocabulary) -> Self {
        let mut vocabulary = Vocabulary {
            core: vec![right.trim().to_string()],
            corrections: vec![(wrong.trim().to_string(), right.trim().to_string())],
            ..Default::default()
        };
        vocabulary.finish();
        vocabulary.known.extend(known.known.iter().cloned());
        let known = &vocabulary.known;
        vocabulary
            .corrections
            .retain(|(wrong, right)| wrong != right && !known.contains(wrong));
        vocabulary
    }

    pub fn is_empty(&self) -> bool {
        self.core.is_empty() && self.entries.is_empty() && self.corrections.is_empty()
    }

    /// Every right form the vocabulary knows, for checking names against.
    pub fn known_terms(&self) -> impl Iterator<Item = &str> {
        self.core
            .iter()
            .map(String::as_str)
            .chain(self.entries.iter().flat_map(|entry| {
                std::iter::once(entry.term.as_str()).chain(entry.aliases.iter().map(String::as_str))
            }))
    }
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))
}

/// The strings under the first of `keys` that `entry` has, as a list.
fn strings(entry: &Value, keys: &[&str]) -> Vec<String> {
    let Some(value) = keys.iter().find_map(|key| entry.get(*key)) else {
        return Vec::new();
    };
    let items: Vec<&Value> = match value {
        Value::Array(items) => items.iter().collect(),
        other => vec![other],
    };
    items
        .into_iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .collect()
}

/// Adds `terms` to the user's vocabulary at `path` (a text file, or a
/// folder whose `hints_core.txt` is the text file), after what is there.
/// Terms it has already are left out. Returns how many were added. A JSON
/// vocabulary is the user's own format and is not written to.
pub fn append_terms(path: &Path, terms: &[String], heading: &str) -> Result<usize> {
    let file = if path.is_dir() {
        path.join("hints_core.txt")
    } else {
        path.to_path_buf()
    };
    if file.extension().is_some_and(|ext| ext == "json") {
        anyhow::bail!("the vocabulary is a JSON file, which is not written to");
    }
    let _writing = writing();
    let vocabulary = Vocabulary::load(path)?;
    let mut new: Vec<&str> = Vec::new();
    for term in terms.iter().map(|term| term.trim()) {
        if !term.is_empty() && !vocabulary.knows(term) && !new.contains(&term) {
            new.push(term);
        }
    }
    if new.is_empty() {
        return Ok(0);
    }
    let mut text = if file.is_file() {
        read(&file)?
    } else if path.is_dir() {
        String::new()
    } else {
        EXAMPLE_FILE.to_string()
    };
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("\n# {heading}\n"));
    for term in &new {
        text.push_str(term);
        text.push('\n');
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    super::store::write_atomically(&file, text.as_bytes())?;
    Ok(new.len())
}

/// A way the speech model mishears a word of the vocabulary, and where the
/// user found it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Misheard {
    /// What the speech model writes for the word.
    pub form: String,
    /// The day it was added (`2026-11-27`); empty when the file does not say.
    pub added_on: String,
    /// The title of the meeting it was fixed in; empty when it was written
    /// in by hand.
    pub meeting: String,
}

/// A word of the user's vocabulary, as the settings list it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Word {
    pub term: String,
    /// "이렇게 잘못 들려요": what the speech model writes instead, corrected
    /// in every meeting transcribed from now on.
    pub misheard: Vec<Misheard>,
    /// It is in the text file, so it can be changed here. A word only in a
    /// JSON vocabulary is the user's to change in that file.
    pub editable: bool,
}

/// The user's vocabulary as one list, newest additions first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct WordList {
    pub words: Vec<Word>,
    /// Words can be added and changed here: the vocabulary is a text file
    /// or a folder, not a single JSON file.
    pub writable: bool,
}

/// One line of the vocabulary's text file, as written and as read.
#[derive(Debug, Clone)]
struct Line {
    raw: String,
    kind: LineKind,
}

#[derive(Debug, Clone)]
enum LineKind {
    /// A blank line, a comment, or something not understood: kept as it is.
    Other,
    /// Terms, one or several separated by commas, and the comment after them.
    Terms { terms: Vec<String>, note: String },
    /// `틀린 말 => 바른 말`, and the comment after it (`# 2026-11-27 「회의」`).
    Misheard {
        wrong: String,
        right: String,
        note: String,
    },
}

impl Line {
    fn parse(raw: &str) -> Line {
        let (body, note) = match raw.find('#') {
            Some(at) => (&raw[..at], raw[at..].trim().to_string()),
            None => (raw, String::new()),
        };
        let body = body.trim();
        let kind = if body.is_empty() {
            LineKind::Other
        } else if let Some((wrong, right)) = body.split_once("=>") {
            let (wrong, right) = (wrong.trim(), right.trim());
            if wrong.is_empty() || right.is_empty() || wrong == right {
                LineKind::Other
            } else {
                LineKind::Misheard {
                    wrong: wrong.to_string(),
                    right: right.to_string(),
                    note,
                }
            }
        } else {
            let terms: Vec<String> = body
                .split([',', '，', '、'])
                .map(str::trim)
                .filter(|term| !term.is_empty())
                .map(str::to_string)
                .collect();
            if terms.is_empty() {
                LineKind::Other
            } else {
                LineKind::Terms { terms, note }
            }
        };
        Line {
            raw: raw.to_string(),
            kind,
        }
    }

    /// The line written again after its parts changed.
    fn rewrite(&mut self) {
        let with_note = |body: String, note: &str| {
            if note.is_empty() {
                body
            } else {
                format!("{body}  {note}")
            }
        };
        self.raw = match &self.kind {
            LineKind::Other => return,
            LineKind::Terms { terms, note } => with_note(terms.join(", "), note),
            LineKind::Misheard { wrong, right, note } => {
                with_note(format!("{wrong} => {right}"), note)
            }
        };
    }

    fn misheard(wrong: &str, right: &str, added_on: &str, meeting: &str) -> Line {
        let meeting: String = meeting
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let meeting = meeting.trim();
        let note = match (added_on.is_empty(), meeting.is_empty()) {
            (true, true) => String::new(),
            (false, true) => format!("# {added_on}"),
            (true, false) => format!("# 「{meeting}」"),
            (false, false) => format!("# {added_on} 「{meeting}」"),
        };
        let mut line = Line {
            raw: String::new(),
            kind: LineKind::Misheard {
                wrong: wrong.to_string(),
                right: right.to_string(),
                note,
            },
        };
        line.rewrite();
        line
    }
}

/// When and from which meeting a misheard form was added, from the comment
/// after it: `# 2026-11-27 「주간 회의」`. Anything else in the comment is
/// the user's own and says neither.
fn provenance(note: &str) -> (String, String) {
    let note = note.trim_start_matches('#').trim();
    let is_day = |text: &str| {
        text.len() == 10
            && text.char_indices().all(|(at, c)| {
                if at == 4 || at == 7 {
                    c == '-'
                } else {
                    c.is_ascii_digit()
                }
            })
    };
    let (day, rest) = match note.get(..10) {
        Some(day) if is_day(day) => (day.to_string(), note[10..].trim()),
        _ => (String::new(), note),
    };
    let meeting = rest
        .strip_prefix('「')
        .and_then(|rest| rest.strip_suffix('」'))
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    (day, meeting)
}

/// The vocabulary's text file: the file itself, or a folder's
/// `hints_core.txt`. `None` for a JSON vocabulary, which is not written to.
fn text_file(path: &Path) -> Option<std::path::PathBuf> {
    if path.is_dir() {
        Some(path.join("hints_core.txt"))
    } else if path.extension().is_some_and(|ext| ext == "json") {
        None
    } else {
        Some(path.to_path_buf())
    }
}

/// The vocabulary's text file read line by line, to change some lines and
/// keep every other line (the user's comments, blank lines, order) as it is.
struct TextFile {
    file: std::path::PathBuf,
    lines: Vec<Line>,
    /// The file's lines end in CR LF (written on Windows): kept so.
    crlf: bool,
    /// The file ends with a line ending: kept so, and kept without.
    final_newline: bool,
}

/// One change of the vocabulary's file at a time: a fix in one window and
/// a word saved in another would otherwise read the same file, and the
/// later write would drop the earlier change (or both would race for the
/// same temporary file).
static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Holds the vocabulary's file for one read, change and write.
fn writing() -> std::sync::MutexGuard<'static, ()> {
    WRITING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl TextFile {
    fn open(path: &Path) -> Result<Self> {
        let file = text_file(path).ok_or_else(|| {
            anyhow::anyhow!("the vocabulary is a JSON file, which is not written to")
        })?;
        let text = if file.is_file() {
            read(&file)?
        } else if path.is_dir() {
            String::new()
        } else {
            // A vocabulary not made yet starts as the example, as when
            // words are added from documents.
            EXAMPLE_FILE.to_string()
        };
        Ok(TextFile {
            file,
            lines: text.lines().map(Line::parse).collect(),
            crlf: text.contains("\r\n"),
            final_newline: text.is_empty() || text.ends_with('\n'),
        })
    }

    fn save(&self) -> Result<()> {
        let ending = if self.crlf { "\r\n" } else { "\n" };
        let mut text: String = self
            .lines
            .iter()
            .map(|line| line.raw.as_str())
            .collect::<Vec<_>>()
            .join(ending);
        if self.final_newline {
            text.push_str(ending);
        }
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        super::store::write_atomically(&self.file, text.as_bytes())
    }

    /// Whether `term` is a term of the file, on its own or as the right
    /// form of a misheard one.
    fn has(&self, term: &str) -> bool {
        self.lines.iter().any(|line| match &line.kind {
            LineKind::Terms { terms, .. } => terms.iter().any(|t| t == term),
            LineKind::Misheard { right, .. } => right == term,
            LineKind::Other => false,
        })
    }

    fn push(&mut self, line: Line) {
        self.lines.push(line);
    }
}

/// A word or misheard form as it may be written in the text file: one
/// line's worth, without the marks the file's lines are read by.
fn clean_word(word: &str) -> Result<String> {
    let word = word.trim();
    if word.is_empty() {
        anyhow::bail!("a word is needed");
    }
    if word.contains(['#', ',', '，', '、', '\n', '\r']) || word.contains("=>") {
        anyhow::bail!("a word cannot have # , or =>");
    }
    Ok(word.to_string())
}

/// The user's vocabulary as one list: each word with the ways it is
/// misheard, the ones added most recently first, the rest in the file's
/// order. Words of a JSON vocabulary are listed too, not to be changed here.
pub fn list(path: &Path) -> Result<WordList> {
    let mut words: Vec<Word> = Vec::new();
    fn word<'a>(words: &'a mut Vec<Word>, term: &str, editable: bool) -> &'a mut Word {
        let at = match words.iter().position(|word| word.term == term) {
            Some(at) => at,
            None => {
                words.push(Word {
                    term: term.to_string(),
                    misheard: Vec::new(),
                    editable,
                });
                words.len() - 1
            }
        };
        let word = &mut words[at];
        word.editable |= editable;
        word
    }
    let writable = text_file(path).is_some();
    if writable {
        // A vocabulary not made yet lists what it starts as.
        for line in &TextFile::open(path)?.lines {
            match &line.kind {
                LineKind::Terms { terms, .. } => {
                    for term in terms {
                        word(&mut words, term, true);
                    }
                }
                LineKind::Misheard { wrong, right, note } => {
                    let (added_on, meeting) = provenance(note);
                    let entry = word(&mut words, right, true);
                    if !entry.misheard.iter().any(|m| &m.form == wrong) {
                        entry.misheard.push(Misheard {
                            form: wrong.clone(),
                            added_on,
                            meeting,
                        });
                    }
                }
                LineKind::Other => {}
            }
        }
    }
    let json = if path.is_dir() {
        Some(path.join("vocabulary.json"))
    } else if !writable {
        Some(path.to_path_buf())
    } else {
        None
    };
    if let Some(json) = json.filter(|json| json.is_file()) {
        let mut vocabulary = Vocabulary::default();
        vocabulary.add_json(&read(&json)?)?;
        for entry in &vocabulary.entries {
            word(&mut words, &entry.term, false);
        }
        for (wrong, right) in &vocabulary.corrections {
            let entry = word(&mut words, right, false);
            if !entry.misheard.iter().any(|m| &m.form == wrong) {
                entry.misheard.push(Misheard {
                    form: wrong.clone(),
                    added_on: String::new(),
                    meeting: String::new(),
                });
            }
        }
    }
    let newest = |word: &Word| {
        word.misheard
            .iter()
            .map(|m| m.added_on.clone())
            .max()
            .unwrap_or_default()
    };
    words.sort_by_key(|word| std::cmp::Reverse(newest(word)));
    Ok(WordList { words, writable })
}

/// Writes a word of the user's vocabulary: `term` with exactly the misheard
/// forms `misheard`, in place of `old` when it is being renamed. A form new
/// to the word is written with when it was added (`today` when `added_on`
/// is empty) and the meeting it came from. Lines about other words, and the
/// user's comments, stay as they are.
pub fn save_word(
    path: &Path,
    old: Option<&str>,
    term: &str,
    misheard: &[Misheard],
    today: &str,
) -> Result<()> {
    let term = clean_word(term)?;
    let _writing = writing();
    let mut forms: Vec<Misheard> = Vec::new();
    for item in misheard {
        let form = clean_word(&item.form)?;
        if form.chars().count() < 2 {
            anyhow::bail!("a misheard form needs two letters or more");
        }
        if form != term && !forms.iter().any(|f| f.form == form) {
            forms.push(Misheard {
                form,
                ..item.clone()
            });
        }
    }
    let mut file = TextFile::open(path)?;

    if let Some(old) = old.map(str::trim).filter(|old| *old != term) {
        for line in &mut file.lines {
            let changed = match &mut line.kind {
                LineKind::Terms { terms, .. } if terms.iter().any(|t| t == old) => {
                    let mut seen = HashSet::new();
                    *terms = terms
                        .iter()
                        .map(|t| if t == old { term.clone() } else { t.clone() })
                        .filter(|t| seen.insert(t.clone()))
                        .collect();
                    true
                }
                LineKind::Misheard { right, .. } if right == old => {
                    *right = term.clone();
                    true
                }
                _ => false,
            };
            if changed {
                line.rewrite();
            }
        }
    }

    // A form is the word's alone: the same form for another word would
    // leave which one it becomes to the file's order.
    file.lines.retain(|line| match &line.kind {
        LineKind::Misheard { wrong, right, .. } => {
            let mine = *right == term;
            let wanted = forms.iter().any(|f| &f.form == wrong);
            if mine {
                wanted
            } else {
                !wanted && *wrong != term
            }
        }
        _ => true,
    });
    for form in &forms {
        let there = file.lines.iter().any(|line| {
            matches!(&line.kind, LineKind::Misheard { wrong, right, .. }
                if *wrong == form.form && *right == term)
        });
        if !there {
            let day = if form.added_on.is_empty() {
                today
            } else {
                form.added_on.as_str()
            };
            file.push(Line::misheard(&form.form, &term, day, &form.meeting));
        }
    }
    if !file.has(&term) {
        file.push(Line::parse(&term));
    }
    file.save()
}

/// Takes `term` and the ways it is misheard out of the user's vocabulary.
pub fn remove_word(path: &Path, term: &str) -> Result<()> {
    let _writing = writing();
    let term = term.trim();
    let mut file = TextFile::open(path)?;
    let before = file.lines.len();
    let mut changed = false;
    file.lines.retain_mut(|line| match &mut line.kind {
        LineKind::Terms { terms, note } if terms.iter().any(|t| t == term) => {
            terms.retain(|t| t != term);
            changed = true;
            if terms.is_empty() {
                if note.is_empty() {
                    return false;
                }
                // The user's comment stays, on its own.
                line.raw = note.clone();
                line.kind = LineKind::Other;
            } else {
                line.rewrite();
            }
            true
        }
        LineKind::Misheard { right, .. } => *right != term,
        _ => true,
    });
    if !changed && file.lines.len() == before {
        anyhow::bail!("the word is not in the vocabulary's text file");
    }
    file.save()
}

/// Adds one way `term` is misheard to the user's vocabulary, from a fix in
/// a meeting. Returns whether it was added: not when the vocabulary already
/// corrects it so, when it could not be used (one letter, or a word the
/// vocabulary knows as right), or when the vocabulary corrects the same
/// words to another word (that is the user's to change in the settings;
/// taking it over here could not be undone).
pub fn add_misheard(
    path: &Path,
    term: &str,
    form: &str,
    added_on: &str,
    meeting: &str,
) -> Result<bool> {
    let (term, form) = (clean_word(term)?, clean_word(form)?);
    if form == term || form.chars().count() < 2 {
        return Ok(false);
    }
    let _writing = writing();
    let vocabulary = Vocabulary::load(path)?;
    if vocabulary.knows(&form)
        || vocabulary
            .corrections
            .iter()
            .any(|(wrong, _)| *wrong == form)
    {
        return Ok(false);
    }
    let mut file = TextFile::open(path)?;
    if file
        .lines
        .iter()
        .any(|line| matches!(&line.kind, LineKind::Misheard { wrong, .. } if *wrong == form))
    {
        return Ok(false);
    }
    file.push(Line::misheard(&form, &term, added_on, meeting));
    file.save()?;
    Ok(true)
}

/// Takes one misheard form of `term` out of the user's vocabulary (a fix
/// undone right after it was made).
pub fn remove_misheard(path: &Path, term: &str, form: &str) -> Result<()> {
    let _writing = writing();
    let (term, form) = (term.trim(), form.trim());
    let mut file = TextFile::open(path)?;
    file.lines.retain(|line| {
        !matches!(&line.kind, LineKind::Misheard { wrong, right, .. }
            if wrong == form && right == term)
    });
    file.save()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picked_words_are_added_to_the_vocabulary_once() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hints_core.txt"), "김서윤\n").unwrap();
        let added = append_terms(
            dir.path(),
            &[
                "김서윤".into(),
                "새봄마루".into(),
                "새봄마루".into(),
                " ".into(),
            ],
            "자료에서 추가",
        )
        .unwrap();
        assert_eq!(added, 1);
        let text = std::fs::read_to_string(dir.path().join("hints_core.txt")).unwrap();
        assert!(text.starts_with("김서윤\n"));
        assert!(text.ends_with("# 자료에서 추가\n새봄마루\n"));
        assert!(Vocabulary::load(dir.path()).unwrap().knows("새봄마루"));
        // Nothing new: the file is left as it is.
        assert_eq!(
            append_terms(dir.path(), &["새봄마루".into()], "x").unwrap(),
            0
        );
        // A JSON vocabulary is not written to.
        let json = dir.path().join("v.json");
        std::fs::write(&json, "[]").unwrap();
        assert!(append_terms(&json, &["새봄마루".into()], "x").is_err());
    }

    fn from_text(text: &str) -> Vocabulary {
        let mut vocabulary = Vocabulary::default();
        vocabulary.add_text(text);
        vocabulary.finish();
        vocabulary
    }

    fn from_json(json: &str) -> Vocabulary {
        let mut vocabulary = Vocabulary::default();
        vocabulary.add_json(json).unwrap();
        vocabulary.finish();
        vocabulary
    }

    #[test]
    fn text_form_reads_terms_comments_and_corrections() {
        let vocabulary = from_text(
            "# 설명\n걸레받이\n몰딩, 중문 # 한 줄에 여러 개\n\n걸래받이 => 걸레받이\n한빗아파트=>한빛아파트\n",
        );
        assert_eq!(
            vocabulary.core,
            vec!["걸레받이", "몰딩", "중문", "한빛아파트"]
        );
        assert_eq!(
            vocabulary.corrections,
            vec![
                ("한빗아파트".to_string(), "한빛아파트".to_string()),
                ("걸래받이".to_string(), "걸레받이".to_string()),
            ]
        );
    }

    #[test]
    fn the_example_file_parses_to_its_terms() {
        let vocabulary = from_text(EXAMPLE_FILE);
        assert_eq!(
            vocabulary.core.first().map(String::as_str),
            Some("걸레받이")
        );
        assert_eq!(vocabulary.core.len(), 17);
        assert_eq!(vocabulary.corrections.len(), 1);
        // What Qwen3 ASR wrote for a ceiling material on a real meeting.
        let (text, made) = vocabulary.correct("인오솔은 천이라 물 한 방울도");
        assert_eq!(text, "이노솔은 천이라 물 한 방울도");
        assert_eq!(made.len(), 1);
        // Its other wrong forms ("S M" for SMC, "이노소") are hints only: as
        // corrections they would rewrite real words ("S M L", "이노소프트").
        for text in [
            "SMC 천장과 이노솔",
            "사이즈는 S M L 중에",
            "이노소프트 제품",
        ] {
            let (same, made) = vocabulary.correct(text);
            assert_eq!(same, text);
            assert!(made.is_empty());
        }
    }

    #[test]
    fn json_form_orders_by_frequency_and_reads_mishearings() {
        let vocabulary = from_json(
            r#"{"version": 1, "entries": [
                {"term": "몰딩", "category": "자재", "freq": 3},
                {"term": "걸레받이", "category": "용어", "freq": 40, "aliases": ["굽도리"], "common_mishearings": ["걸래받이", "걸레바지"]},
                "중문"
            ]}"#,
        );
        let terms: Vec<&str> = vocabulary.entries.iter().map(|e| e.term.as_str()).collect();
        assert_eq!(terms, vec!["걸레받이", "몰딩", "중문"]);
        assert_eq!(vocabulary.entries[0].aliases, vec!["굽도리"]);
        let (text, made) = vocabulary.correct("걸레바지는 흰색으로");
        assert_eq!(text, "걸레받이는 흰색으로");
        assert_eq!(
            made,
            vec![Correction {
                from: "걸레바지".into(),
                to: "걸레받이".into()
            }]
        );
        assert!(Vocabulary::default().add_json("not json").is_err());
    }

    #[test]
    fn customers_are_hinted_only_when_the_recording_mentions_them() {
        let vocabulary = from_json(
            r#"[
                {"term": "이서진", "category": "고객", "freq": 99},
                {"term": "정다은", "category": "직원", "freq": 5},
                {"term": "한빛아파트", "category": "현장", "freq": 9, "aliases": ["한빛"]},
                {"term": "줄눈", "category": "용어", "freq": 50},
                {"term": "최도윤", "category": "고객", "freq": 80}
            ]"#,
        );
        // Without a mention: staff first, then the rest by frequency, no customers.
        assert_eq!(
            vocabulary.hints(&[], 10),
            vec!["정다은", "줄눈", "한빛아파트"]
        );
        // An event title that names a customer and a site brings both to the front.
        let mentions = vec![
            "[회의실A] 이서진 / 한빛 상담".to_string(),
            "박도현".to_string(),
        ];
        assert_eq!(
            vocabulary.hints(&mentions, 10),
            vec!["박도현", "이서진", "한빛아파트", "정다은", "줄눈"]
        );
        // The limit keeps the front of the order.
        assert_eq!(vocabulary.hints(&mentions, 2), vec!["박도현", "이서진"]);
    }

    #[test]
    fn core_terms_come_before_categorised_ones() {
        let mut vocabulary = from_json(r#"[{"term": "줄눈", "category": "용어", "freq": 50}]"#);
        vocabulary.add_terms(["아트월".to_string(), "줄눈".to_string()]);
        assert_eq!(vocabulary.hints(&[], 10), vec!["아트월", "줄눈"]);
    }

    #[test]
    fn a_correction_applies_only_where_a_word_starts() {
        let vocabulary =
            from_text("한빗 => 한빛\n한빗아파트 => 한빛 아파트\n바지 => 받이\n가 => 나\n");
        // The longer form wins, and a one-character rule is ignored.
        assert_eq!(
            vocabulary.correct("한빗아파트 103동").0,
            "한빛 아파트 103동"
        );
        // Every place is reported.
        let (text, made) = vocabulary.correct("그 한빗, 한빗은 한빗에서도");
        assert_eq!(text, "그 한빛, 한빛은 한빛에서도");
        assert_eq!(made.len(), 3);
        // "바지" inside "걸레바지" is the middle of a word: left alone.
        let (text, made) = vocabulary.correct("걸레바지 색과 바지 색");
        assert_eq!(text, "걸레바지 색과 받이 색");
        assert_eq!(made.len(), 1);
        assert_eq!(vocabulary.correct("가나다").1, Vec::new());
    }

    #[test]
    fn a_correction_never_rewrites_a_real_word() {
        let vocabulary =
            from_text("한빗 => 한빛\n실리곤 => 실리콘\n몰딩 => 몰딩재\n한빗마을\n몰딩\n");
        // Two letters that start a different word are left alone ...
        assert_eq!(vocabulary.correct("한빗나래 단지").0, "한빗나래 단지");
        // ... and so is a word the vocabulary knows as right.
        assert_eq!(
            vocabulary.correct("한빗마을에 갑니다").0,
            "한빗마을에 갑니다"
        );
        // A longer wrong form is corrected whatever follows it.
        assert_eq!(vocabulary.correct("실리곤건으로").0, "실리콘건으로");
        // A wrong form that is a right form elsewhere is not a correction.
        assert_eq!(vocabulary.correct("몰딩은 흰색").0, "몰딩은 흰색");
        assert!(only_particles("에서는") && only_particles("") && !only_particles("가람"));
    }

    #[test]
    fn a_folder_reads_only_the_core_list_and_the_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hints_core.txt"), "김서윤\n박도현\n").unwrap();
        std::fs::write(
            dir.path().join("vocabulary.json"),
            r#"{"entries": [{"term": "아트월", "category": "용어", "freq": 5}]}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("notes.txt"), "읽지 않는 파일\n").unwrap();
        std::fs::write(dir.path().join("curated.list"), "읽지 않는 파일\n").unwrap();
        let vocabulary = Vocabulary::load(dir.path()).unwrap();
        assert_eq!(vocabulary.core, vec!["김서윤", "박도현"]);
        assert_eq!(
            vocabulary.hints(&[], 10),
            vec!["김서윤", "박도현", "아트월"]
        );
        assert!(Vocabulary::load(&dir.path().join("없는파일.txt"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn spans_point_into_the_text_as_transcribed() {
        let vocabulary = from_text("한빗아파트 => 한빛 아파트\n한빗 => 한빛\n");
        let text = "그 한빗아파트, 한빗은";
        let spans = vocabulary.spans(text);
        assert_eq!(spans.len(), 2);
        assert_eq!(&text[spans[0].start..spans[0].end], "한빗아파트");
        assert_eq!(spans[0].to, "한빛 아파트");
        assert_eq!(&text[spans[1].start..spans[1].end], "한빗");
        // What a correction puts in is never corrected again.
        let vocabulary = from_text("우물천 => 우물천 장\n");
        assert_eq!(vocabulary.correct("우물천 위").0, "우물천 장 위");
    }

    #[test]
    fn a_word_s_misheard_forms_are_listed_added_changed_and_taken_out() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("용어집.txt");
        std::fs::write(
            &file,
            "# 내 용어집\n몰딩, 중문 # 문\n걸래받이 => 걸레받이\n",
        )
        .unwrap();
        // Learnt in a meeting: kept with the day and the meeting.
        assert!(add_misheard(&file, "우물천장", "우물천장탄", "2026-11-27", "주간 회의").unwrap());
        // Known already, too short, or the same: not added.
        assert!(!add_misheard(&file, "우물천장", "우물천장탄", "2026-11-28", "").unwrap());
        assert!(!add_misheard(&file, "우물천장", "탄", "2026-11-28", "").unwrap());
        assert!(!add_misheard(&file, "몰딩", "중문", "2026-11-28", "").unwrap());
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.ends_with("우물천장탄 => 우물천장  # 2026-11-27 「주간 회의」\n"));
        // An older app reads it as a plain correction.
        let vocabulary = Vocabulary::load(&file).unwrap();
        assert_eq!(vocabulary.correct("우물천장탄을").0, "우물천장을");

        let words = list(&file).unwrap();
        assert!(words.writable);
        let terms: Vec<&str> = words.words.iter().map(|w| w.term.as_str()).collect();
        assert_eq!(terms, vec!["우물천장", "몰딩", "중문", "걸레받이"]);
        assert_eq!(
            words.words[0].misheard,
            vec![Misheard {
                form: "우물천장탄".into(),
                added_on: "2026-11-27".into(),
                meeting: "주간 회의".into(),
            }]
        );

        // Renamed, with one form kept (its note too) and one new.
        let kept = words.words[0].misheard[0].clone();
        save_word(
            &file,
            Some("우물천장"),
            "우물 천장",
            &[
                kept,
                Misheard {
                    form: "우물 천정".into(),
                    added_on: String::new(),
                    meeting: String::new(),
                },
            ],
            "2026-11-28",
        )
        .unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("우물천장탄 => 우물 천장  # 2026-11-27 「주간 회의」\n"));
        assert!(text.contains("우물 천정 => 우물 천장  # 2026-11-28\n"));
        assert!(text.starts_with("# 내 용어집\n몰딩, 중문 # 문\n"));
        // A form taken off the list goes; a word in a line of several is
        // renamed in its place.
        save_word(&file, Some("중문"), "현관 중문", &[], "2026-11-28").unwrap();
        save_word(&file, None, "걸레받이", &[], "2026-11-28").unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("몰딩, 현관 중문  # 문\n"));
        assert!(!text.contains("걸래받이"));
        // The word stays a word without its misheard forms.
        assert!(Vocabulary::load(&file).unwrap().knows("걸레받이"));
        // A new word goes at the end.
        save_word(&file, None, "아트월", &[], "2026-11-28").unwrap();
        assert!(std::fs::read_to_string(&file)
            .unwrap()
            .ends_with("아트월\n"));
        // Taken out: its line, or its place in a line of several.
        remove_word(&file, "몰딩").unwrap();
        remove_word(&file, "우물 천장").unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("\n현관 중문  # 문\n"));
        assert!(!text.contains("우물"));
        assert!(remove_word(&file, "없는 말").is_err());
        // A form undone right after it was learnt.
        add_misheard(&file, "아트월", "아트얼", "2026-11-28", "").unwrap();
        remove_misheard(&file, "아트월", "아트얼").unwrap();
        assert!(!std::fs::read_to_string(&file).unwrap().contains("아트얼"));
        // Marks the file is read by cannot be part of a word.
        assert!(save_word(&file, None, "가, 나", &[], "").is_err());
        assert!(save_word(&file, None, "가#", &[], "").is_err());
    }

    #[test]
    fn a_new_vocabulary_starts_as_the_example_and_a_json_one_is_only_read() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("vocabulary.txt");
        let words = list(&file).unwrap();
        assert_eq!(words.words.len(), 17);
        assert!(words
            .words
            .iter()
            .any(|w| w.term == "이노솔" && w.misheard.len() == 1));
        assert!(add_misheard(&file, "걸레받이", "걸레바지", "2026-11-27", "").unwrap());
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with(EXAMPLE_FILE));

        let json = dir.path().join("v.json");
        std::fs::write(
            &json,
            r#"[{"term": "걸레받이", "common_mishearings": ["걸래받이"]}]"#,
        )
        .unwrap();
        let words = list(&json).unwrap();
        assert!(!words.writable);
        assert!(!words.words[0].editable);
        assert_eq!(words.words[0].misheard[0].form, "걸래받이");
        assert!(save_word(&json, None, "몰딩", &[], "").is_err());
        assert!(add_misheard(&json, "몰딩", "몰땅", "", "").is_err());

        // A folder: the text file is written, its JSON is listed.
        std::fs::write(dir.path().join("vocabulary.json"), r#"["줄눈"]"#).unwrap();
        std::fs::write(dir.path().join("hints_core.txt"), "타일\n").unwrap();
        save_word(dir.path(), None, "마루", &[], "").unwrap();
        let words = list(dir.path()).unwrap();
        let terms: Vec<(&str, bool)> = words
            .words
            .iter()
            .map(|w| (w.term.as_str(), w.editable))
            .collect();
        assert_eq!(terms, vec![("타일", true), ("마루", true), ("줄눈", false)]);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("hints_core.txt")).unwrap(),
            "타일\n마루\n"
        );
    }

    #[test]
    fn a_comment_says_when_and_where_or_nothing() {
        assert_eq!(
            provenance("# 2026-11-27 「주간 회의」"),
            ("2026-11-27".to_string(), "주간 회의".to_string())
        );
        assert_eq!(
            provenance("# 2026-11-27"),
            ("2026-11-27".into(), String::new())
        );
        assert_eq!(provenance("# 문"), (String::new(), String::new()));
        assert_eq!(provenance(""), (String::new(), String::new()));
    }

    #[test]
    fn a_file_keeps_its_line_endings_and_its_last_line() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("v.txt");
        std::fs::write(&file, "# 용어집\r\n몰딩\r\n중문").unwrap();
        save_word(&file, None, "아트월", &[], "2026-11-27").unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "# 용어집\r\n몰딩\r\n중문\r\n아트월"
        );
        std::fs::write(&file, "몰딩\n중문\n").unwrap();
        remove_word(&file, "중문").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "몰딩\n");
    }

    #[test]
    fn a_form_another_word_has_is_not_taken_over_from_a_meeting() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("v.txt");
        let before = "우물 천정 => 우물천정\n";
        std::fs::write(&file, before).unwrap();
        assert!(!add_misheard(&file, "우물천장", "우물 천정", "2026-11-27", "").unwrap());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
    }

    #[test]
    fn changes_from_two_places_at_once_are_all_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("v.txt");
        std::fs::write(&file, "몰딩\n").unwrap();
        let writers: Vec<_> = (0..8)
            .map(|n| {
                let file = file.clone();
                std::thread::spawn(move || {
                    save_word(&file, None, &format!("낱말{n}"), &[], "").unwrap();
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }
        let vocabulary = Vocabulary::load(&file).unwrap();
        assert!((0..8).all(|n| vocabulary.knows(&format!("낱말{n}"))));
    }
}
