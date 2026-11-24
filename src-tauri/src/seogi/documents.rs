//! Words from a meeting's documents: a quote, a drawing, a photo of a
//! sample board. The proper names in them (people, companies, buildings,
//! products, materials) are what a speech model gets wrong, so they are
//! offered as candidates for the meeting's own words. The user only ticks;
//! nothing is typed.
//!
//! Text is read here (PDF and plain text). Pictures are shown to the model
//! that writes minutes when it is a signed-in tool; a model on this computer
//! is not given them. A word taken from a text must appear in that text:
//! the model picks, it does not invent.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;

use super::llm::{self, Provider, Request, Secrets};

/// The most of one document's text that is read.
const TEXT_PER_FILE: usize = 20_000;
/// The most text read from all the documents together.
const TEXT_IN_ALL: usize = 60_000;
/// The most words offered at once.
const MOST_TERMS: usize = 60;

/// What the documents gave.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TermsFound {
    pub terms: Vec<String>,
    /// Documents that gave nothing, and why.
    pub skipped: Vec<Skipped>,
}

/// A document that could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Skipped {
    /// Its file name.
    pub file: String,
    /// `kind` (not a kind read here), `unreadable`, `no_text` (a scanned
    /// PDF), or `pictures_local` (pictures are not shown to a model on this
    /// computer or one behind a key).
    pub reason: String,
}

enum Kind {
    Pdf,
    Text,
    Picture,
}

fn kind_of(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => Some(Kind::Pdf),
        "txt" | "md" | "csv" | "tsv" => Some(Kind::Text),
        "png" | "jpg" | "jpeg" | "gif" | "webp" => Some(Kind::Picture),
        _ => None,
    }
}

/// Whether a file is a document words can be taken from (and not, say, a
/// recording).
pub fn is_document(path: &Path) -> bool {
    kind_of(path).is_some()
}

fn read_pdf(path: &Path) -> Result<String> {
    let path = path.to_path_buf();
    // The PDF reader panics on some damaged files rather than failing.
    std::panic::catch_unwind(move || pdf_extract::extract_text(&path))
        .map_err(|_| anyhow!("the PDF could not be read"))?
        .map_err(|e| anyhow!("the PDF could not be read: {e}"))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// The text with spaces and line breaks taken out, for finding a word in a
/// text that breaks lines in odd places.
fn squeezed(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Whether `term` is in a text squeezed and lowercased by the caller.
fn present(term: &str, haystack: &str) -> bool {
    haystack.contains(&squeezed(term).to_lowercase())
}

const INSTRUCTIONS: &str = "회의 자료(견적서, 도면, 사진 등)에서 회의를 받아 적을 때 틀리기 쉬운 고유명사와 전문 용어를 뽑습니다.\n\
뽑을 것: 사람 이름, 회사·업체 이름, 아파트 단지·건물 이름, 제품·브랜드·모델명, 자재 이름, 그 분야의 전문 용어.\n\
빼는 것: 누구나 아는 말(견적, 합계, 공사, 부가세, 단가, 수량 같은 말), 숫자만 있는 것, 날짜, 금액, 주소의 번지와 동·호수 숫자.\n\
자료에 적힌 표기 그대로 적고, 자료에 없는 말은 만들지 않습니다. 중복 없이 많아야 40개.";

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "terms": { "type": "array", "items": { "type": "string" } }
        },
        "required": ["terms"],
        // Codex holds the answer to the schema strictly, which needs this.
        "additionalProperties": false
    })
}

fn terms_in(answer: &Value) -> Vec<String> {
    answer
        .get("terms")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|term| term.trim().to_string())
        .filter(|term| {
            let letters = term.chars().filter(|c| c.is_alphabetic()).count();
            letters >= 2 && term.chars().count() <= 40
        })
        .collect()
}

/// Candidate words from the documents at `paths`, without the ones in
/// `known` (already the meeting's, or in the vocabulary).
pub fn terms_from(
    paths: &[PathBuf],
    provider: &Provider,
    secrets: Secrets<'_>,
    known: &[String],
) -> Result<TermsFound> {
    let mut found = TermsFound::default();
    let mut text = String::new();
    let mut pictures: Vec<PathBuf> = Vec::new();
    for path in paths {
        let skip = |reason: &str| Skipped {
            file: file_name(path),
            reason: reason.to_string(),
        };
        let read = match kind_of(path) {
            None => {
                found.skipped.push(skip("kind"));
                continue;
            }
            Some(Kind::Picture) => {
                pictures.push(path.clone());
                continue;
            }
            Some(Kind::Pdf) => read_pdf(path),
            Some(Kind::Text) => std::fs::read(path)
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                .map_err(|e| anyhow!("{e}")),
        };
        match read {
            Ok(body) if body.trim().is_empty() => found.skipped.push(skip("no_text")),
            Ok(body) => {
                let room = TEXT_IN_ALL.saturating_sub(text.chars().count());
                let body: String = body.chars().take(TEXT_PER_FILE.min(room)).collect();
                text.push_str(&format!("\n--- {} ---\n{body}\n", file_name(path)));
            }
            Err(e) => {
                log::warn!("Could not read a document: {e:#}");
                found.skipped.push(skip("unreadable"));
            }
        }
    }

    let schema = schema();
    let mut terms: Vec<String> = Vec::new();
    if !text.trim().is_empty() {
        let request = Request {
            system: INSTRUCTIONS,
            user: &format!("자료:\n{text}"),
            schema: &schema,
        };
        let answer = llm::complete(provider, &request, secrets)?;
        // A word the text does not have was made up.
        let haystack = squeezed(&text).to_lowercase();
        terms.extend(
            terms_in(&answer)
                .into_iter()
                .filter(|term| present(term, &haystack)),
        );
    }
    if !pictures.is_empty() {
        let request = Request {
            system: INSTRUCTIONS,
            user: "자료는 함께 보낸 그림입니다. 그림에 보이는 글자에서만 뽑습니다.",
            schema: &schema,
        };
        match llm::complete_with_images(provider, &request, &pictures) {
            Ok(answer) => terms.extend(terms_in(&answer)),
            Err(e) if format!("{e:#}").contains(llm::NO_IMAGES) => {
                found.skipped.extend(pictures.iter().map(|path| Skipped {
                    file: file_name(path),
                    reason: "pictures_local".to_string(),
                }));
            }
            Err(e) => return Err(e),
        }
    }

    let known: Vec<String> = known
        .iter()
        .map(|word| squeezed(word).to_lowercase())
        .collect();
    for term in terms {
        let key = squeezed(&term).to_lowercase();
        if known.contains(&key)
            || found
                .terms
                .iter()
                .any(|kept| squeezed(kept).to_lowercase() == key)
        {
            continue;
        }
        found.terms.push(term);
        if found.terms.len() >= MOST_TERMS {
            break;
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_documents_are_taken_and_answers_are_tidied() {
        assert!(is_document(Path::new("견적서.PDF")));
        assert!(is_document(Path::new("도면.jpg")));
        assert!(is_document(Path::new("메모.txt")));
        assert!(!is_document(Path::new("회의.m4a")));
        assert!(!is_document(Path::new("README")));

        let answer = json!({ "terms": [" 새봄마루 ", "1", "A", "포세린 타일", 3] });
        assert_eq!(terms_in(&answer), ["새봄마루", "포세린 타일"]);
        assert_eq!(squeezed("포세린\n 타일"), "포세린타일");
    }

    #[test]
    fn a_word_the_documents_do_not_have_is_dropped() {
        let haystack = squeezed("원목마루 새봄\n마루 15mm\n담당 박도현").to_lowercase();
        assert!(present("새봄마루", &haystack));
        assert!(present("박도현", &haystack));
        assert!(!present("한빛마을", &haystack));
    }

    #[test]
    fn what_is_not_a_document_is_skipped_and_a_missing_model_is_said() {
        let dir = tempfile::tempdir().unwrap();
        let quote = dir.path().join("견적.txt");
        std::fs::write(&quote, "원목마루 새봄\n마루 15mm\n담당 박도현").unwrap();
        let other = dir.path().join("녹음.m4a");
        std::fs::write(&other, b"not a document").unwrap();
        // Ollama is not reached in tests: a closed port answers at once.
        let provider = Provider::Ollama {
            base_url: "http://127.0.0.1:9".into(),
            model: "none".into(),
        };
        let no_secrets = |_: &str| None;
        let result = terms_from(&[other.clone()], &provider, &no_secrets, &[]).unwrap();
        assert_eq!(result.terms, Vec::<String>::new());
        assert_eq!(result.skipped[0].reason, "kind");
        // With text to read the model is asked, and an unreachable one is an
        // error the screen shows.
        assert!(terms_from(&[quote], &provider, &no_secrets, &[]).is_err());
    }

    /// Asks a real model; run by hand with `SEOGI_DOCS=<folder>` and
    /// `SEOGI_LLM=claude:<model>` (or `ollama:<model>`).
    #[test]
    #[ignore]
    fn words_from_real_documents() {
        let dir = PathBuf::from(std::env::var("SEOGI_DOCS").unwrap());
        let provider = Provider::parse(&std::env::var("SEOGI_LLM").unwrap()).unwrap();
        let paths: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| is_document(path))
            .collect();
        let no_secrets = |_: &str| None;
        let found = terms_from(&paths, &provider, &no_secrets, &[]).unwrap();
        eprintln!("{found:#?}");
        assert!(!found.terms.is_empty() || !found.skipped.is_empty());
    }
}
