//! Asking a recording a question.
//!
//! The answer is written by the summary model from the transcript alone,
//! and every fact in it must come with the words it rests on. Those words
//! are then looked up in the transcript: a quote that is not there is
//! dropped, and an answer left with none is marked as unsupported, so the
//! screen can say so instead of presenting a guess as an answer.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;

use super::llm::{self, Provider, Secrets};
use super::minutes::{self, Source};
use super::transcript::{clock, Transcript};

/// The most transcript sent with a question. A longer recording is searched
/// for the parts that bear on the question first. A model on this computer
/// reads less at once than one elsewhere.
const BUDGET_CHARS: usize = 40_000;
const LOCAL_BUDGET_CHARS: usize = 14_000;
/// Lines kept on each side of a line that bears on the question.
const AROUND: usize = 2;
/// How closely a quote must match the transcript to count as found.
const FOUND: f32 = 0.6;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Answer {
    pub text: String,
    /// Whether the recording has the answer at all.
    pub found: bool,
    /// The words the answer rests on, and when they were said.
    pub sources: Vec<Source>,
    /// The model answered but none of the words it gave are in the
    /// transcript: not to be trusted as it stands.
    pub unsupported: bool,
}

/// Answers `question` from `transcript`.
pub fn answer(
    transcript: &Transcript,
    question: &str,
    provider: &Provider,
    secrets: Secrets<'_>,
) -> Result<Answer> {
    let question = question.trim();
    if question.is_empty() {
        return Err(anyhow!("the question is empty"));
    }
    if transcript.segments.is_empty() {
        return Err(anyhow!("the recording has no transcript to ask"));
    }
    let system = "당신은 녹음을 받아 적은 글만 보고 질문에 답하는 서기입니다.\n\
        - 받아쓴 글에 있는 내용만으로 답합니다. 글에 없으면 found를 false로 하고 answer에 \"녹음에서 찾지 못했습니다.\"라고 씁니다.\n\
        - 답은 두세 문장으로 짧게 씁니다. 짐작이나 일반 상식을 보태지 않습니다.\n\
        - 답에 쓴 사실마다 근거가 된 말을 받아쓴 글에서 그대로(8~40자) quotes에 옮깁니다. 한 글자도 바꾸지 않습니다.\n\
        - 숫자, 날짜, 금액, 이름은 글에 나온 그대로 씁니다.";
    let user = format!(
        "받아쓴 글:\n{}\n\n질문: {question}",
        lines_for(
            transcript,
            question,
            if provider.is_local() {
                LOCAL_BUDGET_CHARS
            } else {
                BUDGET_CHARS
            }
        )
    );
    let schema = json!({
        "type": "object",
        "properties": {
            "answer": { "type": "string" },
            "found": { "type": "boolean" },
            "quotes": { "type": "array", "items": { "type": "string" } }
        },
        "required": ["answer", "found", "quotes"],
        "additionalProperties": false
    });
    let reply = llm::complete(
        provider,
        &llm::Request {
            system,
            user: &user,
            schema: &schema,
        },
        secrets,
    )?;
    Ok(checked(transcript, &reply))
}

/// The model's reply with its quotes looked up in the transcript.
fn checked(transcript: &Transcript, reply: &Value) -> Answer {
    let text = reply
        .get("answer")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let found = reply.get("found").and_then(Value::as_bool).unwrap_or(false) && !text.is_empty();
    let mut sources: Vec<Source> = Vec::new();
    for quote in reply
        .get("quotes")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter_map(Value::as_str)
    {
        if let Some((ms, score)) = minutes::locate(transcript, quote) {
            if score >= FOUND && !sources.iter().any(|source| source.ms == ms) {
                sources.push(Source {
                    ms,
                    quote: quote.trim().to_string(),
                });
            }
        }
    }
    sources.sort_by_key(|source| source.ms);
    Answer {
        unsupported: found && sources.is_empty(),
        text,
        found,
        sources,
    }
}

/// The transcript as lines for the model: all of it when it is short, else
/// the lines that share words with the question and those around them.
fn lines_for(transcript: &Transcript, question: &str, budget: usize) -> String {
    let lines: Vec<String> = transcript
        .paragraphs()
        .iter()
        .map(|paragraph| {
            format!(
                "[{}] {}: {}",
                clock(paragraph.start_ms),
                transcript.speaker_name(paragraph.speaker),
                paragraph.text
            )
        })
        .collect();
    let total: usize = lines.iter().map(|line| line.chars().count() + 1).sum();
    if total <= budget {
        return lines.join("\n");
    }

    // Two letters at a time is how Korean words are best compared without
    // knowing where they end.
    let pieces = |text: &str| -> std::collections::HashSet<(char, char)> {
        let letters: Vec<char> = text
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        letters.windows(2).map(|pair| (pair[0], pair[1])).collect()
    };
    let wanted = pieces(question);
    let mut ranked: Vec<(usize, usize)> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| (pieces(line).intersection(&wanted).count(), index))
        .filter(|(shared, _)| *shared > 0)
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    let mut keep = vec![false; lines.len()];
    let mut used = 0;
    for (_, index) in ranked {
        let from = index.saturating_sub(AROUND);
        let to = (index + AROUND).min(lines.len() - 1);
        let cost: usize = (from..=to)
            .filter(|&i| !keep[i])
            .map(|i| lines[i].chars().count() + 1)
            .sum();
        if used + cost > budget {
            break;
        }
        used += cost;
        keep[from..=to].iter_mut().for_each(|kept| *kept = true);
    }
    let mut out = String::new();
    let mut gap = false;
    for (line, kept) in lines.iter().zip(&keep) {
        if *kept {
            // Whatever was left out, before, between or after, is marked.
            if gap {
                out.push_str("…\n");
            }
            out.push_str(line);
            out.push('\n');
            gap = false;
        } else {
            gap = true;
        }
    }
    if gap {
        out.push_str("…\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seogi::transcript::{Segment, Speaker};

    fn transcript(lines: &[(u64, &str)]) -> Transcript {
        Transcript {
            language: "ko".into(),
            model: "test".into(),
            segments: lines
                .iter()
                .map(|(ms, text)| Segment {
                    start_ms: *ms,
                    end_ms: ms + 4_000,
                    speaker: 1,
                    text: (*text).to_string(),
                    confidence: None,
                    unsure: false,
                    overlap: false,
                    edited: None,
                    fixes: Vec::new(),
                })
                .collect(),
            speakers: vec![Speaker {
                id: 1,
                name: "김서윤 실장".into(),
            }],
            corrections: Vec::new(),
        }
    }

    #[test]
    fn a_quote_that_is_in_the_recording_becomes_a_source_with_its_time() {
        let transcript = transcript(&[
            (1_000, "예산은 4500만 원 안쪽으로 맞추고 싶어요."),
            (9_000, "착공은 12월 8일로 생각하고 있습니다."),
        ]);
        let reply = json!({
            "answer": "착공은 12월 8일로 생각하고 있다.",
            "found": true,
            "quotes": ["착공은 12월 8일로 생각하고", "입주는 2월 말입니다"]
        });
        let answer = checked(&transcript, &reply);
        assert!(answer.found && !answer.unsupported);
        // The quote that was never said is dropped.
        assert_eq!(answer.sources.len(), 1);
        assert_eq!(answer.sources[0].ms, 9_000);
    }

    #[test]
    fn an_answer_with_no_quote_in_the_recording_is_marked_unsupported() {
        let transcript = transcript(&[(1_000, "예산은 4500만 원 안쪽으로 맞추고 싶어요.")]);
        let made_up = json!({
            "answer": "계약금은 500만 원이다.",
            "found": true,
            "quotes": ["계약금은 500만 원으로 하겠습니다"]
        });
        let answer = checked(&transcript, &made_up);
        assert!(answer.unsupported && answer.sources.is_empty());

        let not_there =
            json!({ "answer": "녹음에서 찾지 못했습니다.", "found": false, "quotes": [] });
        let answer = checked(&transcript, &not_there);
        assert!(!answer.found && !answer.unsupported);
    }

    #[test]
    fn a_long_recording_is_cut_down_to_what_bears_on_the_question() {
        let filler = "날씨 이야기를 한참 동안 나누었습니다. ".repeat(12);
        let mut lines: Vec<(u64, String)> =
            (0..120u64).map(|i| (i * 10_000, filler.clone())).collect();
        lines[77].1 = "욕실 타일은 무광 600각으로 정했습니다.".to_string();
        let borrowed: Vec<(u64, &str)> = lines
            .iter()
            .map(|(ms, text)| (*ms, text.as_str()))
            .collect();
        // Each line a turn of its own: far enough apart not to be joined.
        let mut transcript = self::transcript(&borrowed);
        for (index, segment) in transcript.segments.iter_mut().enumerate() {
            segment.speaker = (index % 2) as u32 + 1;
        }
        transcript.speakers.push(Speaker {
            id: 2,
            name: "박도현 소장".into(),
        });

        let sent = lines_for(
            &transcript,
            "욕실 타일은 무엇으로 정했나요?",
            LOCAL_BUDGET_CHARS,
        );
        assert!(sent.chars().count() <= LOCAL_BUDGET_CHARS + 400);
        assert!(sent.contains("무광 600각"));
        // Parts left out are shown as left out.
        assert!(sent.contains("…\n"));

        // A short recording is sent whole.
        let whole = lines_for(
            &self::transcript(&[(0, "짧은 녹음입니다.")]),
            "무엇",
            BUDGET_CHARS,
        );
        assert_eq!(whole, "[00:00:00] 김서윤 실장: 짧은 녹음입니다.");
    }
}
