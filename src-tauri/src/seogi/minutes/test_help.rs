//! Lines, transcripts and entries the tests of the minutes build from.

use super::*;
use crate::seogi::templates::built_in;
use crate::seogi::transcript::{parse_clock, Speaker};

pub(super) fn task(
    text: &str,
    owner: &str,
    due: &str,
    ms: u64,
    reasons: Vec<CheckReason>,
) -> Entry {
    Entry {
        text: text.into(),
        owner: owner.into(),
        due: due.into(),
        sources: vec![Source {
            ms,
            quote: text.into(),
        }],
        reasons,
        done: false,
        faint: false,
        original: None,
        added: false,
        id: String::new(),
    }
}

pub(super) fn consult() -> Template {
    built_in()
        .into_iter()
        .find(|template| template.id == "consult")
        .unwrap()
}

pub(super) fn line(start_ms: u64, speaker: u32, text: &str) -> Segment {
    Segment {
        start_ms,
        end_ms: start_ms + 4_000,
        speaker,
        text: text.to_string(),
        confidence: Some(0.9),
        unsure: false,
        overlap: false,
        edited: None,
        fixes: Vec::new(),
    }
}

pub(super) fn transcript() -> Transcript {
    Transcript {
        language: "ko".into(),
        model: "test".into(),
        segments: vec![
            line(1_000, 1, "안녕하세요 저는 디자인을 맡은 김서윤 실장입니다."),
            line(6_000, 2, "예산은 4,500만 원 안쪽으로 맞추고 싶어요."),
            line(
                11_000,
                1,
                "네, 중문은 신발장 도면을 다시 그려서 금요일까지 보내드리겠습니다.",
            ),
            line(
                16_000,
                2,
                "신발장이 줄어드는 건 조금 고민되네요. 일단 생각해 볼게요.",
            ),
        ],
        speakers: vec![
            Speaker {
                id: 1,
                name: "김서윤 실장".into(),
            },
            Speaker {
                id: 2,
                name: "화자 2".into(),
            },
        ],
        corrections: Vec::new(),
    }
}

pub(super) fn raw(
    section: &str,
    text: &str,
    owner: &str,
    due: &str,
    ts: &str,
    quote: &str,
) -> RawItem {
    RawItem {
        section: section.into(),
        text: text.into(),
        owner: owner.into(),
        due: due.into(),
        near_ms: parse_clock(ts),
        quote: quote.into(),
    }
}

pub(super) fn item(section: &str, text: &str, line: usize, ms: u64) -> Item {
    Item {
        section: section.into(),
        entry: Entry {
            text: text.into(),
            owner: String::new(),
            due: String::new(),
            sources: vec![Source {
                ms,
                quote: text.into(),
            }],
            reasons: Vec::new(),
            done: false,
            faint: false,
            original: None,
            added: false,
            id: String::new(),
        },
        lines: line..=line,
    }
}

pub(super) fn meeting_of(lines: &[(u64, u32, &str)]) -> Transcript {
    let mut transcript = transcript();
    transcript.segments = lines
        .iter()
        .map(|&(ms, speaker, text)| line(ms, speaker, text))
        .collect();
    transcript
}
