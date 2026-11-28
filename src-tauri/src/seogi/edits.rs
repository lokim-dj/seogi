//! Changes made on either side, shared through the meeting's Drive folder.
//!
//! The computer writes the meeting's files in Drive, but the user may tick a
//! task, fix a line or name a speaker on the phone as well as here (issue
//! #67). Each device writes only its own file in the meeting's Drive folder,
//! `고친것-컴퓨터.json` or `고친것-폰.json`, holding its latest change of each
//! thing with the time it was made. Reading both, each side keeps, for every
//! thing changed, the later change; the other one, when it says something
//! else, stays beside it as a faint line the user can tap to take instead.
//! Nothing is dropped without a trace.
//!
//! The things are named by keys that do not move: `done:<task id>` (a task
//! ticked, `true`/`false`), `line:<start ms>` (a line of the transcript as
//! the user reads it) and `speaker:<id>` (a speaker's name).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use super::minutes::Entry;
use super::store::Meeting;
use super::templates::SectionKind;
use super::transcript::{NameScope, Transcript};

/// The computer's file in a meeting's Drive folder.
pub const COMPUTER_FILE: &str = "고친것-컴퓨터.json";
/// The phone's file in a meeting's Drive folder.
pub const PHONE_FILE: &str = "고친것-폰.json";

/// One change: what was changed, to what, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub key: String,
    pub value: Value,
    /// When it was made, in milliseconds since 1970 UTC.
    pub at_ms: i64,
}

/// A device's file: its latest change of each thing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EditsFile {
    #[serde(default = "one")]
    pub format: u32,
    /// `computer` or `phone`.
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub meeting: String,
    #[serde(default)]
    pub edits: Vec<Edit>,
}

fn one() -> u32 {
    1
}

impl EditsFile {
    /// The computer's file for a meeting, from its changes by key.
    pub fn computer(meeting: &str, edits: &BTreeMap<String, Edit>) -> Self {
        Self {
            format: 1,
            device: "computer".into(),
            meeting: meeting.into(),
            edits: edits.values().cloned().collect(),
        }
    }

    /// Reads a device's file; `None` for anything that is not one.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }

    /// The changes by key, the latest when a key comes twice.
    pub fn by_key(&self) -> BTreeMap<String, Edit> {
        let mut out: BTreeMap<String, Edit> = BTreeMap::new();
        for edit in &self.edits {
            if out.get(&edit.key).is_none_or(|e| e.at_ms <= edit.at_ms) {
                out.insert(edit.key.clone(), edit.clone());
            }
        }
        out
    }
}

/// The value of every thing that can be changed in a meeting, by key.
pub fn values(meeting: &Meeting) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for entry in task_entries(meeting) {
        if !entry.id.is_empty() {
            out.insert(format!("done:{}", entry.id), Value::Bool(entry.done));
        }
    }
    if let Some(t) = &meeting.transcript {
        for segment in &t.segments {
            out.insert(
                format!("line:{}", segment.start_ms),
                Value::String(segment.said().trim().to_string()),
            );
        }
        for speaker in &t.speakers {
            out.insert(
                format!("speaker:{}", speaker.id),
                Value::String(speaker.name.clone()),
            );
        }
    }
    out
}

fn task_entries(meeting: &Meeting) -> impl Iterator<Item = &Entry> {
    meeting
        .minutes
        .first()
        .into_iter()
        .flat_map(|m| m.sections.iter())
        .filter(|s| s.kind == SectionKind::Actions)
        .flat_map(|s| s.entries.iter())
}

/// What the user changed between `before` and `after`: the things in both
/// whose value moved. Things that came or went (minutes written again, a
/// task added) are not changes of a thing.
pub fn changed(
    before: &BTreeMap<String, Value>,
    after: &BTreeMap<String, Value>,
) -> Vec<(String, Value)> {
    after
        .iter()
        .filter(|(key, value)| before.get(*key).is_some_and(|old| old != *value))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Which side made a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Computer,
    Phone,
}

/// Two changes of one thing that say different things: the later one
/// stands, the other is kept to be taken instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Conflict {
    pub key: String,
    /// The change that lost, as text for the screen (`true`/`false` for a
    /// task's tick).
    pub other: String,
    /// Where the change that lost was made.
    pub other_side: Side,
    #[specta(type = f64)]
    pub other_at_ms: i64,
}

/// For each thing changed on either side, the change that stands: the
/// later one; at the very same moment the computer's (both sides decide it
/// the same way). Returns the winners by key and the losers that say
/// something else.
pub fn merge(
    computer: &BTreeMap<String, Edit>,
    phone: &BTreeMap<String, Edit>,
) -> (BTreeMap<String, (Edit, Side)>, Vec<Conflict>) {
    let mut winners = BTreeMap::new();
    let mut losers = Vec::new();
    let keys: std::collections::BTreeSet<&String> = computer.keys().chain(phone.keys()).collect();
    for key in keys {
        match (computer.get(key), phone.get(key)) {
            (Some(c), Some(p)) => {
                let (win, win_side, lose, lose_side) = if p.at_ms > c.at_ms {
                    (p, Side::Phone, c, Side::Computer)
                } else {
                    (c, Side::Computer, p, Side::Phone)
                };
                if win.value != lose.value {
                    losers.push(Conflict {
                        key: key.clone(),
                        other: as_text(&lose.value),
                        other_side: lose_side,
                        other_at_ms: lose.at_ms,
                    });
                }
                winners.insert(key.clone(), (win.clone(), win_side));
            }
            (Some(c), None) => {
                winners.insert(key.clone(), (c.clone(), Side::Computer));
            }
            (None, Some(p)) => {
                winners.insert(key.clone(), (p.clone(), Side::Phone));
            }
            (None, None) => {}
        }
    }
    (winners, losers)
}

fn as_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Makes the meeting say `value` for `key`. Returns whether anything
/// changed; a key the meeting does not have (a task since removed, a line
/// transcribed again) changes nothing. The recording is never touched: a
/// line keeps the speech model's words and gets the user's beside them.
pub fn apply(meeting: &mut Meeting, key: &str, value: &Value) -> bool {
    let Some((kind, target)) = key.split_once(':') else {
        return false;
    };
    match kind {
        "done" => {
            let Some(done) = value.as_bool() else {
                return false;
            };
            let Some(minutes) = meeting.minutes.first_mut() else {
                return false;
            };
            for section in minutes
                .sections
                .iter_mut()
                .filter(|s| s.kind == SectionKind::Actions)
            {
                if let Some(entry) = section.entries.iter_mut().find(|e| e.id == target) {
                    let moved = entry.done != done;
                    entry.done = done;
                    return moved;
                }
            }
            false
        }
        "line" => {
            let (Ok(start), Some(text)) = (target.parse::<u64>(), value.as_str()) else {
                return false;
            };
            let Some(t) = meeting.transcript.as_mut() else {
                return false;
            };
            let Some(segment) = t.segments.iter_mut().find(|s| s.start_ms == start) else {
                return false;
            };
            let text = text.trim();
            if segment.said().trim() == text {
                return false;
            }
            segment.edited = (text != segment.text.trim()).then(|| text.to_string());
            segment.fixes.clear();
            true
        }
        "speaker" => {
            let (Ok(id), Some(name)) = (target.parse::<u32>(), value.as_str()) else {
                return false;
            };
            let name = name.trim();
            let Some(t) = meeting.transcript.as_mut() else {
                return false;
            };
            if name.is_empty()
                || t.speaker_name(id) == name
                || !t.speakers.iter().any(|s| s.id == id)
            {
                return false;
            }
            rename(t, id, name).is_some()
        }
        _ => false,
    }
}

/// Names a speaker throughout, as "이름 바꾸기" here does; the old name.
fn rename(t: &mut Transcript, id: u32, name: &str) -> Option<String> {
    t.name_speaker(id, name, NameScope::All, 0, 0)
        .map(|(old, _, _)| old)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn edit(key: &str, value: Value, at_ms: i64) -> Edit {
        Edit {
            key: key.into(),
            value,
            at_ms,
        }
    }

    fn map(edits: &[Edit]) -> BTreeMap<String, Edit> {
        edits.iter().map(|e| (e.key.clone(), e.clone())).collect()
    }

    #[test]
    fn the_later_change_stands_and_the_other_is_kept() {
        let computer = map(&[
            edit("done:a", json!(true), 100),
            edit("line:0", json!("가"), 300),
        ]);
        let phone = map(&[
            edit("done:a", json!(false), 200),
            edit("line:0", json!("나"), 250),
        ]);
        let (winners, losers) = merge(&computer, &phone);
        assert_eq!(
            winners["done:a"],
            (edit("done:a", json!(false), 200), Side::Phone)
        );
        assert_eq!(winners["line:0"].1, Side::Computer);
        assert_eq!(losers.len(), 2);
        let tick = losers.iter().find(|c| c.key == "done:a").unwrap();
        assert_eq!(
            (tick.other.as_str(), tick.other_side),
            ("true", Side::Computer)
        );
        let line = losers.iter().find(|c| c.key == "line:0").unwrap();
        assert_eq!((line.other.as_str(), line.other_side), ("나", Side::Phone));
    }

    #[test]
    fn the_same_words_on_both_sides_are_no_conflict() {
        let computer = map(&[edit("speaker:1", json!("가"), 100)]);
        let phone = map(&[edit("speaker:1", json!("가"), 200)]);
        let (winners, losers) = merge(&computer, &phone);
        assert!(losers.is_empty());
        assert_eq!(winners["speaker:1"].1, Side::Phone);
    }

    #[test]
    fn at_the_same_moment_the_computer_stands_on_both_sides() {
        let computer = map(&[edit("done:a", json!(true), 100)]);
        let phone = map(&[edit("done:a", json!(false), 100)]);
        assert_eq!(merge(&computer, &phone).0["done:a"].1, Side::Computer);
    }

    #[test]
    fn a_change_on_one_side_alone_stands() {
        let (winners, losers) = merge(&BTreeMap::new(), &map(&[edit("done:a", json!(true), 1)]));
        assert_eq!(winners.len(), 1);
        assert!(losers.is_empty());
    }

    #[test]
    fn only_things_in_both_whose_value_moved_are_changes() {
        let before: BTreeMap<String, Value> = [
            ("done:a".to_string(), json!(false)),
            ("line:0".to_string(), json!("x")),
        ]
        .into();
        let after: BTreeMap<String, Value> = [
            ("done:a".to_string(), json!(true)),
            ("done:b".to_string(), json!(false)),
            ("line:0".to_string(), json!("x")),
        ]
        .into();
        assert_eq!(
            changed(&before, &after),
            vec![("done:a".to_string(), json!(true))]
        );
    }

    #[test]
    fn a_file_keeps_the_latest_of_each_key_and_ignores_what_it_does_not_know() {
        let file = EditsFile::parse(
            br#"{"format":1,"device":"phone","meeting":"m","edits":[
                {"key":"done:a","value":true,"at_ms":5},{"key":"done:a","value":false,"at_ms":9}],"later":1}"#,
        )
        .unwrap();
        assert_eq!(file.by_key()["done:a"].value, json!(false));
        assert!(EditsFile::parse(b"not json").is_none());
    }

    fn a_meeting() -> Meeting {
        let mut m = Meeting::new(0, super::super::store::Origin::Recorded, "녹음.wav");
        m.transcript = Some(
            serde_json::from_value(json!({
                "language": "ko", "model": "test",
                "segments": [
                    {"start_ms": 0, "end_ms": 1000, "speaker": 1, "text": "견적서는 금요일까지"},
                    {"start_ms": 1000, "end_ms": 2000, "speaker": 2, "text": "네 알겠습니다"}
                ],
                "speakers": [{"id": 1, "name": "화자 1"}, {"id": 2, "name": "화자 2"}]
            }))
            .unwrap(),
        );
        m.minutes = vec![serde_json::from_value(json!({
            "template_id": "meeting", "template_name": "회의", "title": "", "summary": "",
            "sections": [{"key": "actions", "title": "할 일", "kind": "actions", "entries": [
                {"id": "t1", "text": "견적서 보내기", "owner": "화자 1", "due": "", "sources": [], "reasons": []}
            ]}],
            "verified": 0, "to_check": 0, "dropped": 0, "written_by": "test"
        }))
        .unwrap()];
        m
    }

    #[test]
    fn a_change_is_made_to_the_meeting_and_the_words_heard_stay() {
        let mut m = a_meeting();
        assert!(apply(&mut m, "done:t1", &json!(true)));
        assert!(!apply(&mut m, "done:t1", &json!(true)));
        assert!(apply(&mut m, "line:1000", &json!("네, 알겠습니다")));
        let t = m.transcript.as_ref().unwrap();
        assert_eq!(t.segments[1].said(), "네, 알겠습니다");
        assert_eq!(t.segments[1].text, "네 알겠습니다");
        // Back to the words heard: no longer marked as fixed.
        assert!(apply(&mut m, "line:1000", &json!("네 알겠습니다")));
        assert_eq!(m.transcript.as_ref().unwrap().segments[1].edited, None);
        assert!(apply(&mut m, "speaker:2", &json!("나")));
        assert_eq!(m.transcript.as_ref().unwrap().speaker_name(2), "나");
        // What the meeting does not have changes nothing.
        assert!(!apply(&mut m, "done:gone", &json!(true)));
        assert!(!apply(&mut m, "line:5", &json!("x")));
        assert!(!apply(&mut m, "speaker:9", &json!("x")));
        assert!(!apply(&mut m, "other", &json!("x")));
    }

    #[test]
    fn the_values_name_every_task_line_and_speaker() {
        let v = values(&a_meeting());
        assert_eq!(v["done:t1"], json!(false));
        assert_eq!(v["line:0"], json!("견적서는 금요일까지"));
        assert_eq!(v["speaker:2"], json!("화자 2"));
    }
}
