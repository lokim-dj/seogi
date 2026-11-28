//! Tidying the tasks (what is no promise, the same promise told twice, who
//! does it) and choosing which doubts are put to the person.

use super::*;

/// Who a task can be given to, beyond the people in the meeting: the side
/// that promised it, by the section it is in.
pub(super) fn side_of(section: &str) -> Option<&'static str> {
    match section {
        "our_tasks" => Some("우리(상담 담당)"),
        "customer_tasks" => Some("고객"),
        _ => None,
    }
}

/// Reads the tasks together: drops what is not a promise for later, makes
/// one task of a promise told more than once (keeping every time it was
/// said), and gives each the one who does it, picked from `people` or the
/// side that promised. Nothing new is written: the text of a task is one
/// the extraction gave, an owner is one of the offered names, a date one of
/// those the task's tellings gave.
pub(super) fn tidy_tasks(
    input: &Input<'_>,
    template: &Template,
    entries: &mut Vec<(String, Entry)>,
    transcript: &Transcript,
    people: &[String],
) -> Result<()> {
    let tasks: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, (key, _))| {
            template
                .section(key)
                .is_some_and(|section| section.kind == SectionKind::Actions)
        })
        .map(|(at, _)| at)
        .collect();
    if tasks.is_empty() {
        return Ok(());
    }
    let mut owners: Vec<String> = people
        .iter()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();
    for &at in &tasks {
        if let Some(side) = side_of(&entries[at].0) {
            if !owners.iter().any(|owner| owner == side) {
                owners.push(side.to_string());
            }
        }
    }
    let listing: String = tasks
        .iter()
        .enumerate()
        .map(|(number, &at)| {
            let (key, entry) = &entries[at];
            let section = template
                .section(key)
                .map_or(key.as_str(), |s| s.title.as_str());
            let mut line = format!("{}. [{section}] {}", number + 1, entry.text);
            if !entry.owner.is_empty() {
                line.push_str(&format!(" / 담당: {}", entry.owner));
            }
            if !entry.due.is_empty() {
                line.push_str(&format!(" / 기한: {}", entry.due));
            }
            if let Some(source) = entry.sources.first() {
                let at = transcript
                    .segments
                    .iter()
                    .position(|segment| segment.start_ms == source.ms);
                // The line before, where someone may have handed it over.
                if let Some(before) = at
                    .and_then(|at| at.checked_sub(1))
                    .and_then(|at| transcript.segments.get(at))
                {
                    line.push_str(&format!(
                        "\n   (앞 줄) {}: {}",
                        transcript.speaker_name(before.speaker),
                        before.text
                    ));
                }
                let speaker = at
                    .map(|at| transcript.speaker_name(transcript.segments[at].speaker))
                    .unwrap_or_default();
                line.push_str(&format!(
                    "\n   [{}] {speaker}: 「{}」",
                    clock(source.ms),
                    source.quote
                ));
            }
            line.push('\n');
            line
        })
        .collect();
    let system = "당신은 회의록의 할 일 목록을 다듬는 서기입니다. 녹음에서 뽑은 할 일 후보가 번호와 함께, 근거가 된 말과 그 말을 한 사람과 같이 주어집니다.\n\n\
         1. promise: 회의가 끝난 뒤 누군가 실제로 해야 하는 약속이면 true입니다. 회의 중에 이미 한 일이나 하고 있는 일을 묘사한 것(예: \"상담 중에 견적서를 쓰고 있음\", \"가전 자리를 반영해 제안함\"), 지나가며 한 말, 일반적인 설명, 가벼운 안내나 권유(예: \"사례 영상을 참고해 달라\", \"편하게 연락 달라\")는 false입니다. 할 일은 적을수록 좋습니다. 한두 시간 회의에서 남는 할 일은 보통 다섯에서 열 개입니다.\n\
         2. 같은 약속이 여러 번 나왔으면 한 묶음(ids)으로 모읍니다. 묶음의 첫 번호는 그 약속을 가장 잘 적은 것으로 합니다. 비슷해 보여도 다른 일이면 따로 둡니다.\n\
         3. owner: 그 일을 할 사람을 owners 목록에서만 고르고, 사람 이름을 먼저 봅니다. 근거가 된 말을 한 사람이 이름으로 적혀 있고 \"제가 하겠습니다\"처럼 자기가 하겠다고 했으면 그 사람입니다. 다른 사람이 이름을 대며 맡긴 말(\"그건 박 소장님이 봐 주실 거예요\")이 바로 앞에 있으면 그 사람입니다. 그렇게 사람을 알 수 없을 때만 약속한 쪽(목록에 있는 '우리(상담 담당)'이나 '고객')을 고르고, 그것도 알 수 없으면 빈 문자열로 둡니다.\n\
         4. weight: 3은 돈·일정·계약·고객의 결정에 바로 걸린 약속, 2는 누군가 실제로 해야 하는 작업, 1은 가벼운 안내·추천·참고 자료 보내기입니다.\n\
         5. 회의 중에 이미 끝낸 일(\"링크를 전달함\", \"도면을 고침\", \"사이즈를 확인함\", \"추천함\")은 앞으로 할 약속이 아니므로 promise를 false로 합니다.\n\
         6. 모든 번호가 정확히 한 묶음에 들어가야 합니다. 새 내용을 지어내지 않습니다.";
    let user = format!(
        "owners: {}\n\n할 일 후보:\n{listing}",
        if owners.is_empty() {
            "(없음)".to_string()
        } else {
            owners.join(", ")
        }
    );
    let schema = json!({
        "type": "object",
        "properties": {
            "groups": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "ids": { "type": "array", "items": { "type": "integer" } },
                        "promise": { "type": "boolean" },
                        "owner": { "type": "string" },
                        "weight": { "type": "integer" }
                    },
                    "required": ["ids", "promise", "owner", "weight"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["groups"],
        "additionalProperties": false
    });
    let answer = ask_again(input, system, &user, &schema)?;
    let mut groups = read_task_groups(&answer, tasks.len());
    keep_weightiest(&mut groups, entries, &tasks);
    apply_task_groups(entries, &tasks, &groups, &owners);
    Ok(())
}

/// A group of task numbers (from 0) that are one promise, or none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TaskGroup {
    ids: Vec<usize>,
    promise: bool,
    owner: String,
    /// 3 for money, dates, the contract or the customer's decision; 2 for
    /// work to do; 1 for a tip or a suggestion.
    weight: u8,
}

/// Leaves a meeting a list of tasks people go through: light ones go when
/// there are more than a few, and no more than [`MOST_TASKS`] stay, the
/// weightiest and then the earliest. What goes is still in the story.
pub(super) fn keep_weightiest(
    groups: &mut [TaskGroup],
    entries: &[(String, Entry)],
    tasks: &[usize],
) {
    let first_ms = |group: &TaskGroup| {
        group
            .ids
            .iter()
            .filter_map(|&id| entries[tasks[id]].1.sources.first().map(|s| s.ms))
            .min()
            .unwrap_or(0)
    };
    let promises = groups.iter().filter(|g| g.promise).count();
    if promises > ENOUGH_TASKS {
        for group in groups.iter_mut().filter(|g| g.promise && g.weight <= 1) {
            group.promise = false;
        }
    }
    let mut kept: Vec<(u8, u64, usize)> = groups
        .iter()
        .enumerate()
        .filter(|(_, g)| g.promise)
        .map(|(at, g)| (g.weight, first_ms(g), at))
        .collect();
    if kept.len() > MOST_TASKS {
        kept.sort_by_key(|&(weight, ms, _)| (std::cmp::Reverse(weight), ms));
        for &(_, _, at) in kept.iter().skip(MOST_TASKS) {
            groups[at].promise = false;
        }
    }
}

/// The groups in an answer, each task in one group at most; numbers the
/// answer leaves out are kept as they were, each on its own.
pub(super) fn read_task_groups(answer: &Value, count: usize) -> Vec<TaskGroup> {
    let mut seen = vec![false; count];
    let mut groups: Vec<TaskGroup> = Vec::new();
    for group in answer
        .get("groups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let ids: Vec<usize> = group
            .get("ids")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .filter_map(|id| (id as usize).checked_sub(1))
            .filter(|&id| id < count)
            .filter(|&id| !std::mem::replace(&mut seen[id], true))
            .collect();
        if ids.is_empty() {
            continue;
        }
        groups.push(TaskGroup {
            ids,
            promise: group
                .get("promise")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            owner: group
                .get("owner")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string(),
            weight: group
                .get("weight")
                .and_then(Value::as_u64)
                .map_or(2, |weight| weight.clamp(1, 3) as u8),
        });
    }
    for (id, seen) in seen.iter().enumerate() {
        if !seen {
            groups.push(TaskGroup {
                ids: vec![id],
                promise: true,
                owner: String::new(),
                weight: 2,
            });
        }
    }
    groups
}

/// Makes the groups so: the first task of each promise takes the times and
/// doubts of the others, and the others and what is no promise go.
pub(super) fn apply_task_groups(
    entries: &mut Vec<(String, Entry)>,
    tasks: &[usize],
    groups: &[TaskGroup],
    owners: &[String],
) {
    let mut gone: HashSet<usize> = HashSet::new();
    for group in groups {
        let positions: Vec<usize> = group.ids.iter().map(|&id| tasks[id]).collect();
        if !group.promise {
            gone.extend(positions);
            continue;
        }
        let keep = positions[0];
        let mut sources: Vec<Source> = Vec::new();
        let mut reasons: Vec<CheckReason> = Vec::new();
        let mut due = String::new();
        let mut owner_said = String::new();
        let mut done = false;
        for &at in &positions {
            let entry = &entries[at].1;
            for source in &entry.sources {
                if !sources.iter().any(|s| s.ms == source.ms) {
                    sources.push(source.clone());
                }
            }
            for reason in &entry.reasons {
                if !reasons.contains(reason) {
                    reasons.push(*reason);
                }
            }
            if due.is_empty() {
                due = entry.due.clone();
            }
            if owner_said.is_empty() && !entry.owner.starts_with("화자") {
                owner_said = entry.owner.clone();
            }
            done |= entry.done;
        }
        sources.sort_by_key(|source| source.ms);
        let entry = &mut entries[keep].1;
        entry.sources = sources;
        entry.reasons = reasons;
        entry.due = due;
        entry.done = done;
        // An owner from the offered names; one the recording named stays
        // when the answer gives none of them.
        entry.owner = if owners.contains(&group.owner) {
            group.owner.clone()
        } else {
            owner_said
        };
        gone.extend(positions.into_iter().skip(1));
    }
    let mut at = 0;
    entries.retain(|_| {
        let keep = !gone.contains(&at);
        at += 1;
        keep
    });
}

/// How a promise of one's own deed ends: "할게요", "보내드리겠습니다".
pub(super) const OWN_DEEDS: &[&str] = &[
    "할게요",
    "하겠습니다",
    "드릴게요",
    "드리겠습니다",
    "보낼게요",
    "보내겠습니다",
    "볼게요",
    "보겠습니다",
    "갈게요",
    "가겠습니다",
    "챙길게요",
    "챙기겠습니다",
    "줄게요",
    "주겠습니다",
    "올게요",
    "오겠습니다",
];

/// Someone else named as the doer in the same line.
pub(super) const OTHER_DOERS: &[&str] =
    &["님이", "님께서", "님한테", "님께", "씨가", "씨한테", "분이"];

/// Gives tasks to people by name where the recording says who: a task the
/// customer's side promised goes to the customer by name when the meeting
/// has one, and one left to our side (or to no one) goes to the speaker who
/// promised their own deed in the first person ("제가 보내드릴게요"), when
/// that speaker is named, is not the customer, and the line names no one
/// else as the doer. Nothing is guessed: without such words
/// the side stays.
pub(super) fn owners_by_name(
    template: &Template,
    entries: &mut [(String, Entry)],
    transcript: &Transcript,
    customer: &str,
) {
    let customer = customer.trim();
    for (key, entry) in entries.iter_mut() {
        if !template
            .section(key)
            .is_some_and(|section| section.kind == SectionKind::Actions)
        {
            continue;
        }
        let side = side_of(key);
        if side == Some("고객") {
            if entry.owner == "고객" && !customer.is_empty() {
                entry.owner = customer.to_string();
            }
            continue;
        }
        if !(entry.owner.is_empty() || side.is_some_and(|side| entry.owner == side)) {
            continue;
        }
        let Some(source) = entry.sources.first() else {
            continue;
        };
        let Some(line) = transcript
            .segments
            .iter()
            .find(|segment| segment.start_ms == source.ms)
        else {
            continue;
        };
        let name = transcript.speaker_name(line.speaker);
        let named = !name.starts_with("화자") && !super::facts::is_customer(&name, customer);
        // "제가" or "내가" as a word of its own, not inside "안내가".
        let first_person = line
            .text
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| word == "제가" || word == "내가");
        // The speaker's own deed for later ("제가 … 보낼게요"), and no one
        // else named as the one who does it ("소장님이 봐 주실 거예요").
        let squeezed: String = line.text.chars().filter(|c| !c.is_whitespace()).collect();
        let own_deed = OWN_DEEDS.iter().any(|end| squeezed.contains(end));
        let someone_else = OTHER_DOERS.iter().any(|word| squeezed.contains(word));
        if named && first_person && own_deed && !someone_else {
            entry.owner = name;
        }
    }
}

/// How much an entry matters to be checked: tasks and decisions, money and
/// dates first.
pub(super) fn weight(template: &Template, key: &str, entry: &Entry) -> u32 {
    let mut weight = 0;
    if template
        .section(key)
        .is_some_and(|section| section.kind == SectionKind::Actions)
    {
        weight += 4;
    } else if key == "decisions" {
        weight += 3;
    }
    if key == "budget" || key == "schedule" || !entry.due.is_empty() {
        weight += 2;
    }
    if !numbers_in(&entry.text).is_empty() {
        weight += 2;
    }
    if entry.reasons.iter().any(|r| {
        matches!(
            r,
            CheckReason::NumberNotInSource
                | CheckReason::UnitOdd
                | CheckReason::SumsDisagree
                | CheckReason::NumbersDiffer
        )
    }) {
        weight += 2;
    }
    // Said both ways, maybe someone else's, or maybe after the meeting:
    // what a person most needs to hear before relying on the minutes.
    if entry.reasons.iter().any(|r| {
        matches!(
            r,
            CheckReason::Conflicting | CheckReason::OtherSubject | CheckReason::AfterTheEnd
        )
    }) {
        weight += 3;
    }
    if entry.reasons.iter().any(|reason| {
        matches!(
            reason,
            CheckReason::WeakSupport
                | CheckReason::SaidDifferently
                | CheckReason::OwnerUnclear
                | CheckReason::NotSettled
        )
    }) {
        weight += 1;
    }
    weight
}

/// Leaves at most `most` entries to be checked by ear, the weightiest. Of
/// the rest, one the checker found no clear support for is dropped (it may
/// not have been said); the others are kept, shown faintly. Returns how many
/// were dropped.
pub(super) fn keep_checks(
    template: &Template,
    entries: &mut Vec<(String, Entry)>,
    most: usize,
) -> u32 {
    let mut doubted: Vec<(usize, u32, u64)> = entries
        .iter()
        .enumerate()
        .filter(|(_, (_, entry))| entry.needs_check())
        .map(|(at, (key, entry))| {
            let ms = entry.sources.first().map_or(0, |source| source.ms);
            (at, weight(template, key, entry), ms)
        })
        .collect();
    if doubted.len() <= most {
        return 0;
    }
    doubted.sort_by_key(|&(_, weight, ms)| (std::cmp::Reverse(weight), ms));
    let mut gone: HashSet<usize> = HashSet::new();
    for &(at, _, _) in doubted.iter().skip(most) {
        let entry = &mut entries[at].1;
        if entry
            .reasons
            .iter()
            .any(|r| matches!(r, CheckReason::WeakSupport | CheckReason::OtherSubject))
        {
            gone.insert(at);
        } else if entry.reasons.iter().any(|r| MUST_BE_HEARD.contains(r)) {
            // Said two ways, or an amount no one says so: shown faintly it
            // would read as fact, so it stays to be heard past the limit.
        } else {
            entry.reasons.clear();
            entry.faint = true;
        }
    }
    let mut at = 0;
    entries.retain(|_| {
        let keep = !gone.contains(&at);
        at += 1;
        keep
    });
    gone.len() as u32
}

/// Doubts never turned into a faint line by the limit on checks: the entry
/// as written may be plainly wrong.
pub(super) const MUST_BE_HEARD: [CheckReason; 5] = [
    CheckReason::AfterTheEnd,
    CheckReason::Conflicting,
    CheckReason::UnitOdd,
    CheckReason::SumsDisagree,
    CheckReason::NumbersDiffer,
];

pub(super) fn push_reason(entry: &mut Entry, reason: CheckReason) {
    if !entry.reasons.contains(&reason) {
        entry.reasons.push(reason);
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_help::*;
    use super::*;

    #[test]
    fn one_promise_told_three_times_is_one_task_and_what_was_done_goes() {
        let mut entries = vec![
            (
                "our_tasks".to_string(),
                task("견적서를 보내기로 함", "", "", 3_000, vec![]),
            ),
            (
                "our_tasks".to_string(),
                task("상담 중 견적서를 쓰고 있음", "", "", 4_000, vec![]),
            ),
            (
                "decisions".to_string(),
                task("마루는 살리기로 함", "", "", 5_000, vec![]),
            ),
            (
                "our_tasks".to_string(),
                task(
                    "견적서를 바로 보내 드림",
                    "",
                    "오늘",
                    9_000,
                    vec![CheckReason::UnsureSpeech],
                ),
            ),
            (
                "customer_tasks".to_string(),
                task("냉장고 치수를 재서 알려 주기", "화자 2", "", 7_000, vec![]),
            ),
        ];
        let tasks = vec![0, 1, 3, 4];
        let owners = vec![
            "민트".to_string(),
            "우리(상담 담당)".to_string(),
            "고객".to_string(),
        ];
        let answer = json!({ "groups": [
            { "ids": [1, 3], "promise": true, "owner": "민트" },
            { "ids": [2], "promise": false, "owner": "" },
            // A number twice, one out of range, and a name not offered.
            { "ids": [3, 9], "promise": true, "owner": "" },
        ]});
        let groups = read_task_groups(&answer, tasks.len());
        // Number 4 was left out of the answer: it stays as it was.
        assert_eq!(groups.len(), 3);
        apply_task_groups(&mut entries, &tasks, &groups, &owners);

        let texts: Vec<&str> = entries.iter().map(|(_, e)| e.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "견적서를 보내기로 함",
                "마루는 살리기로 함",
                "냉장고 치수를 재서 알려 주기"
            ]
        );
        let merged = &entries[0].1;
        assert_eq!(merged.owner, "민트");
        assert_eq!(merged.due, "오늘");
        assert_eq!(
            merged.sources.iter().map(|s| s.ms).collect::<Vec<_>>(),
            [3_000, 9_000]
        );
        assert_eq!(merged.reasons, [CheckReason::UnsureSpeech]);
        // A speaker label never becomes the owner.
        assert_eq!(entries[2].1.owner, "");

        let other = json!({ "groups": [{ "ids": [1], "promise": true, "owner": "아무개" }] });
        let groups = read_task_groups(&other, 1);
        let mut one = vec![(
            "our_tasks".to_string(),
            task("도면 보내기", "김서윤 실장", "", 0, vec![]),
        )];
        apply_task_groups(&mut one, &[0], &groups, &owners);
        assert_eq!(
            one[0].1.owner, "김서윤 실장",
            "a name not offered keeps the one said"
        );
    }

    #[test]
    fn a_long_list_of_tasks_keeps_the_weightiest() {
        let entries: Vec<(String, Entry)> = (0..14)
            .map(|n| {
                (
                    "our_tasks".to_string(),
                    task(&format!("일 {n}"), "", "", 1_000 * n, vec![]),
                )
            })
            .collect();
        let tasks: Vec<usize> = (0..14).collect();
        let mut groups: Vec<TaskGroup> = (0..14)
            .map(|n| TaskGroup {
                ids: vec![n],
                promise: true,
                owner: String::new(),
                // Two light ones, three that matter most, the rest work.
                weight: match n {
                    0 | 1 => 1,
                    11..=13 => 3,
                    _ => 2,
                },
            })
            .collect();
        keep_weightiest(&mut groups, &entries, &tasks);
        let kept: Vec<usize> = groups
            .iter()
            .filter(|g| g.promise)
            .map(|g| g.ids[0])
            .collect();
        assert_eq!(kept.len(), MOST_TASKS);
        assert!(!kept.contains(&0) && !kept.contains(&1));
        assert!(kept.contains(&11) && kept.contains(&12) && kept.contains(&13));
        // Of the work, the earliest stay.
        assert!(kept.contains(&2) && !kept.contains(&10));

        // Few tasks: a light one stays.
        let mut few = vec![TaskGroup {
            ids: vec![0],
            promise: true,
            owner: String::new(),
            weight: 1,
        }];
        keep_weightiest(&mut few, &entries, &tasks);
        assert!(few[0].promise);
    }

    #[test]
    fn only_the_weightiest_doubts_are_put_to_the_person() {
        let template = consult();
        let mut entries: Vec<(String, Entry)> = Vec::new();
        for n in 0..12 {
            entries.push((
                "materials".to_string(),
                task(
                    &format!("자재 메모 {n}"),
                    "",
                    "",
                    60_000 * n,
                    vec![CheckReason::UnsureSpeech],
                ),
            ));
        }
        entries.push((
            "materials".to_string(),
            task(
                "근거가 약한 자재",
                "",
                "",
                1,
                vec![CheckReason::WeakSupport],
            ),
        ));
        entries.push((
            "our_tasks".to_string(),
            task(
                "견적서 보내기",
                "",
                "금요일까지",
                2,
                vec![CheckReason::UnsureSpeech],
            ),
        ));
        entries.push((
            "budget".to_string(),
            task("예산 1억", "", "", 3, vec![CheckReason::OverlappingSpeech]),
        ));
        entries.push((
            "decisions".to_string(),
            task("마루 유지", "", "", 4, vec![]),
        ));

        let dropped = keep_checks(&template, &mut entries, 3);
        let mut checked: Vec<&str> = entries
            .iter()
            .filter(|(_, e)| e.needs_check())
            .map(|(_, e)| e.text.as_str())
            .collect();
        checked.sort_unstable();
        // The task and the money first, then the earliest of the rest.
        assert_eq!(checked, ["견적서 보내기", "예산 1억", "자재 메모 0"]);
        // The unsupported one beyond the few is left out; the rest stay, faint.
        assert_eq!(dropped, 1);
        assert!(!entries.iter().any(|(_, e)| e.text == "근거가 약한 자재"));
        assert_eq!(entries.iter().filter(|(_, e)| e.faint).count(), 11);
        assert!(entries
            .iter()
            .any(|(_, e)| e.text == "마루 유지" && !e.faint));

        // Few enough: nothing changes.
        let mut few = vec![(
            "budget".to_string(),
            task("예산", "", "", 0, vec![CheckReason::WeakSupport]),
        )];
        assert_eq!(keep_checks(&template, &mut few, 3), 0);
        assert!(few[0].1.needs_check());
    }

    #[test]
    fn a_task_goes_to_the_person_who_promised_it_by_name() {
        let template = consult();
        let mut transcript = transcript();
        transcript
            .segments
            .push(line(21_000, 2, "제가 집에 가서 냉장고 치수를 재 볼게요."));
        transcript
            .segments
            .push(line(26_000, 1, "저희가 견적은 다음 주에 드리겠습니다."));
        let mut entries = vec![
            (
                "our_tasks".to_string(),
                task(
                    "신발장 도면 다시 그려 보내기",
                    "우리(상담 담당)",
                    "",
                    11_000,
                    vec![],
                ),
            ),
            (
                "customer_tasks".to_string(),
                task("냉장고 치수 재기", "고객", "", 21_000, vec![]),
            ),
            (
                "our_tasks".to_string(),
                task("견적 보내기", "우리(상담 담당)", "", 26_000, vec![]),
            ),
        ];
        // Line 11 s is the named designer's own "보내드리겠습니다" without
        // "제가": the side stays; "저희가" is the company, not a person.
        owners_by_name(&template, &mut entries, &transcript, "이서진");
        assert_eq!(entries[0].1.owner, "우리(상담 담당)");
        assert_eq!(entries[1].1.owner, "이서진");
        assert_eq!(entries[2].1.owner, "우리(상담 담당)");

        transcript.segments[2].text =
            "네, 중문은 제가 신발장 도면을 다시 그려서 보내드리겠습니다.".into();
        owners_by_name(&template, &mut entries, &transcript, "이서진");
        assert_eq!(entries[0].1.owner, "김서윤 실장");

        // "제가" with someone else as the doer is not the speaker's task.
        entries[0].1.owner = "우리(상담 담당)".into();
        transcript.segments[2].text =
            "제가 확인해 보니 그건 박도현 소장님이 보내 주실 거예요.".into();
        owners_by_name(&template, &mut entries, &transcript, "이서진");
        assert_eq!(entries[0].1.owner, "우리(상담 담당)");
    }
}
