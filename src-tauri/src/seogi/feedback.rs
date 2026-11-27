//! Feedback on a consultation, for the team to read together: what went
//! well, what could go better, questions not asked, moments the customer
//! hesitated and what was answered, and what to prepare for next time.
//!
//! It is written by the model that writes minutes, from the checked entries
//! only, and every point names the entries it rests on; a point that names
//! none is left out. Each point is then checked against the transcript lines
//! around those entries (`claims.rs`): one the lines do not say is left out,
//! one they say less strongly is softened. The tone is plain: no praise or blame beyond what the
//! recording shows. What a company counts as a good consultation is its
//! own, in a rules file in the user's data folder; the one here is an
//! example.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;

use super::minutes::{Entry, Source};
use super::transcript::clock;

/// The file in the app's data folder that holds a company's own standard
/// for a good consultation, in plain words.
pub const RULES_NAME: &str = "consult-feedback.md";

/// What is looked at when no rules file has been written.
pub const EXAMPLE_RULES: &str = "\
- 고객의 생활과 원하는 것을 먼저 충분히 묻는다.
- 예산, 일정, 결정할 사람을 상담 안에서 확인한다.
- 고객이 망설이거나 걱정한 것에는 근거를 들어 답하고, 모르는 것은 확인해서 알려 주겠다고 약속한다.
- 고객의 말을 끊지 않고 끝까지 듣는다.
- 상담이 끝날 때 다음에 할 일과 날짜를 함께 정리한다.";

/// The most points of each kind.
const POINTS: usize = 3;
/// The most times shown with a point.
const POINT_TIMES: usize = 2;

/// One point of feedback, with the places in the recording it rests on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Point {
    pub text: String,
    pub sources: Vec<Source>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Feedback {
    pub good: Vec<Point>,
    pub improve: Vec<Point>,
    /// Questions that were not asked and would have helped.
    pub missed_questions: Vec<Point>,
    /// Moments the customer hesitated, and what was answered.
    pub hesitations: Vec<Point>,
    pub next_time: Vec<Point>,
}

impl Feedback {
    pub fn is_empty(&self) -> bool {
        self.good.is_empty()
            && self.improve.is_empty()
            && self.missed_questions.is_empty()
            && self.hesitations.is_empty()
            && self.next_time.is_empty()
    }
}

/// The kinds of points, as the answer names them and the screens title them.
pub const KINDS: [(&str, &str); 5] = [
    ("good", "잘한 점"),
    ("improve", "아쉬운 점"),
    ("missed_questions", "놓친 질문"),
    ("hesitations", "고객이 망설인 순간과 우리 답"),
    ("next_time", "다음 미팅 준비"),
];

/// The request for feedback: instructions, and the entries numbered from 1
/// with their times. `talk` is the measured talking, in a line or two.
pub fn request(entries: &[(&str, &Entry)], rules: &str, talk: &str) -> (String, String, Value) {
    let system = format!(
        "당신은 인테리어 상담을 함께 돌아보는 동료입니다. 상담 녹음에서 뽑아 확인을 마친 항목들이 번호, 말한 시각, 갈래와 함께 주어집니다. 팀이 같이 읽고 맞는지 틀린지 이야기할 피드백을 씁니다.\n\n\
         이 회사가 좋은 상담으로 보는 것:\n{rules}\n\n\
         - good(잘한 점), improve(아쉬운 점), missed_questions(물었으면 좋았을 질문), hesitations(고객이 망설이거나 걱정한 순간과 그때 우리가 한 답), next_time(다음 미팅 전에 준비할 것)을 각각 많아야 {POINTS}개 씁니다. 없으면 비워 둡니다.\n\
         - 점마다 근거가 된 항목 번호(items)를 적습니다. 항목에 없는 일은 쓰지 않고, 근거가 없는 지적은 하지 않습니다.\n\
         - 말투는 담백하게 씁니다. 칭찬도 비난도 부풀리지 않습니다. 사람을 평가하지 말고 상담에서 있었던 일을 말합니다.\n\
         - 한 점은 한두 문장, '~했다', '~하면 좋겠다'처럼 끝냅니다."
    );
    let listing: String = entries
        .iter()
        .enumerate()
        .map(|(number, (section, entry))| {
            let ms = entry.sources.first().map_or(0, |s| s.ms);
            format!(
                "{}. [{}] [{section}] {}\n",
                number + 1,
                clock(ms),
                entry.text
            )
        })
        .collect();
    let user = format!("말하기 측정: {talk}\n\n확인을 마친 항목:\n{listing}");
    let point = json!({
        "type": "object",
        "properties": {
            "text": { "type": "string" },
            "items": { "type": "array", "items": { "type": "integer" } }
        },
        "required": ["text", "items"],
        "additionalProperties": false
    });
    let mut properties = serde_json::Map::new();
    for (kind, _) in KINDS {
        properties.insert(kind.to_string(), json!({ "type": "array", "items": point }));
    }
    let schema = json!({
        "type": "object",
        "properties": properties,
        "required": KINDS.iter().map(|(kind, _)| *kind).collect::<Vec<_>>(),
        "additionalProperties": false
    });
    (system, user, schema)
}

/// The points of an answer that rest on `entries` (numbered from 1).
pub fn read(answer: &Value, entries: &[(&str, &Entry)]) -> Feedback {
    let points = |kind: &str| -> Vec<Point> {
        answer
            .get(kind)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|point| {
                let text = point
                    .get("text")
                    .and_then(Value::as_str)?
                    .trim()
                    .to_string();
                let mut sources: Vec<Source> = point
                    .get("items")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_u64)
                    .filter_map(|n| (n as usize).checked_sub(1))
                    .filter_map(|at| entries.get(at))
                    .filter_map(|(_, entry)| entry.sources.first().cloned())
                    .collect();
                if text.is_empty() || sources.is_empty() {
                    return None;
                }
                sources.sort_by_key(|source| source.ms);
                sources.dedup_by_key(|source| source.ms);
                // Every place it rests on is kept for the check against the
                // recording; `trim_times` leaves the few that are shown.
                sources.truncate(super::claims::CLAIM_PLACES);
                Some(Point { text, sources })
            })
            .take(POINTS)
            .collect()
    };
    Feedback {
        good: points("good"),
        improve: points("improve"),
        missed_questions: points("missed_questions"),
        hesitations: points("hesitations"),
        next_time: points("next_time"),
    }
}

/// Leaves the few times shown with each point, once the points have been
/// checked against the lines around all of them.
pub fn trim_times(feedback: &mut Feedback) {
    for points in [
        &mut feedback.good,
        &mut feedback.improve,
        &mut feedback.missed_questions,
        &mut feedback.hesitations,
        &mut feedback.next_time,
    ] {
        for point in points.iter_mut() {
            point.sources.truncate(POINT_TIMES);
        }
    }
}

/// The company's own standard, or the example.
pub fn rules(data_dir: &std::path::Path) -> String {
    std::fs::read_to_string(data_dir.join(RULES_NAME))
        .ok()
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| EXAMPLE_RULES.to_string())
}

/// Writes the feedback with `ask` (system, user, schema → answer).
pub fn write(
    entries: &[(&str, &Entry)],
    rules: &str,
    talk: &str,
    ask: &dyn Fn(&str, &str, &Value) -> Result<Value>,
) -> Result<Feedback> {
    if entries.is_empty() {
        return Ok(Feedback::default());
    }
    let (system, user, schema) = request(entries, rules, talk);
    let answer = ask(&system, &user, &schema)?;
    Ok(read(&answer, entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str, ms: u64) -> Entry {
        Entry {
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
        }
    }

    #[test]
    fn every_point_rests_on_entries_and_there_are_few_of_each() {
        let worry = entry("고객이 바닥 철거 비용을 걱정함", 30_000);
        let answer = entry("바닥은 살리는 쪽으로 안내함", 31_000);
        let entries = vec![("고객의 우려", &worry), ("결정 사항", &answer)];
        let reply = json!({
            "good": [
                { "text": "걱정에 바로 근거를 들어 답했다.", "items": [2, 1] },
                { "text": "근거 없는 칭찬", "items": [] },
                { "text": "없는 항목", "items": [9] }
            ],
            "improve": [],
            "missed_questions": [
                { "text": "a", "items": [1] }, { "text": "b", "items": [1] },
                { "text": "c", "items": [1] }, { "text": "d", "items": [1] }
            ],
            "hesitations": [{ "text": "바닥 비용에서 망설였다.", "items": [1] }],
            "next_time": []
        });
        let feedback = read(&reply, &entries);
        assert_eq!(feedback.good.len(), 1);
        assert_eq!(
            feedback.good[0]
                .sources
                .iter()
                .map(|s| s.ms)
                .collect::<Vec<_>>(),
            [30_000, 31_000]
        );
        assert_eq!(feedback.missed_questions.len(), POINTS);
        assert!(feedback.improve.is_empty());
        assert!(!feedback.is_empty());

        let (system, user, schema) = request(&entries, EXAMPLE_RULES, "우리 80% · 고객 20%");
        assert!(system.contains("고객의 말을 끊지 않고"));
        assert!(user.contains("1. [00:00:30] [고객의 우려] 고객이 바닥 철거 비용을 걱정함"));
        assert_eq!(schema["required"].as_array().unwrap().len(), KINDS.len());
    }

    #[test]
    fn a_company_writes_its_own_standard() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(rules(dir.path()), EXAMPLE_RULES);
        std::fs::write(dir.path().join(RULES_NAME), "- 첫 인사에 명함을 드린다.\n").unwrap();
        assert_eq!(rules(dir.path()), "- 첫 인사에 명함을 드린다.\n");
    }
}
