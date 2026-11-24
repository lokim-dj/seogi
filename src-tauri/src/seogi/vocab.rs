//! The user's own vocabulary: names and terms a speech model gets wrong.
//!
//! It lives in a file or folder the user chooses, never in the app or this
//! repository. Two things come out of it: hint terms given to the speech
//! model with every chunk, and corrections applied to the text it returns.
//!
//! Accepted forms:
//! - a text file: one term per line (or several separated by commas), `#`
//!   starts a comment, and `틀린 말 => 바른 말` is a correction;
//! - a JSON file: a list of terms, or of entries like
//!   `{"term": "걸레받이", "category": "용어", "aliases": [...],
//!   "common_mishearings": [...], "freq": 12}`, bare or under an `entries` /
//!   `terms` / `vocabulary` key;
//! - a folder holding `hints_core.txt` (the terms to hint first, in the text
//!   form) and `vocabulary.json`. Nothing else in the folder is read.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

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
    ///
    /// A correction must never turn a real word into a wrong one, so a wrong
    /// form is replaced only:
    /// - where a word starts (Korean attaches particles to the end of a word,
    ///   never to its start, so `걸레바지는` is corrected while the same
    ///   letters in the middle of another word are left alone);
    /// - when the word it starts is not a longer term the vocabulary knows;
    /// - and, for a wrong form of two letters, when the word ends there or
    ///   goes on with particles only.
    pub fn correct(&self, text: &str) -> (String, Vec<Correction>) {
        let mut corrected = text.to_string();
        let mut made: Vec<Correction> = Vec::new();
        for (wrong, right) in &self.corrections {
            if !corrected.contains(wrong.as_str()) {
                continue;
            }
            let short = wrong.chars().count() <= 2;
            let mut out = String::with_capacity(corrected.len());
            let mut rest = corrected.as_str();
            let mut changed = false;
            while let Some(at) = rest.find(wrong.as_str()) {
                let before = out.chars().chain(rest[..at].chars()).next_back();
                let after = &rest[at + wrong.len()..];
                // The rest of the word the wrong form starts.
                let tail: &str = after
                    .find(|c: char| !c.is_alphanumeric())
                    .map_or(after, |end| &after[..end]);
                let at_word_start = before.is_none_or(|c| !c.is_alphanumeric());
                let is_known_word = tail.char_indices().any(|(index, c)| {
                    let end = at + wrong.len() + index + c.len_utf8();
                    self.known.contains(&rest[at..end])
                });
                out.push_str(&rest[..at]);
                if at_word_start && !is_known_word && (!short || only_particles(tail)) {
                    out.push_str(right);
                    changed = true;
                    made.push(Correction {
                        from: wrong.clone(),
                        to: right.clone(),
                    });
                } else {
                    out.push_str(wrong);
                }
                rest = after;
            }
            out.push_str(rest);
            if changed {
                corrected = out;
            }
        }
        (corrected, made)
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
        assert_eq!(vocabulary.core.len(), 15);
        assert!(vocabulary.corrections.is_empty());
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
}
