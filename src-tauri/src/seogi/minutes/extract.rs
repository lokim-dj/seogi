//! Requests to the model, the overview and the extraction of items with
//! their quotes.

use super::*;

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// Runs `job` on each of `things`, `at_once` at a time, and gives the results
/// in the order of `things`. After each round `go_on` sees the results so far
/// and says whether to continue. Returns what was done and whether that was
/// all of it.
pub(super) fn in_rounds<T: Sync, R: Send>(
    things: &[T],
    at_once: usize,
    job: &(dyn Fn(usize, &T) -> R + Sync),
    go_on: &dyn Fn(&[R]) -> bool,
) -> (Vec<R>, bool) {
    let at_once = at_once.max(1);
    let mut results: Vec<R> = Vec::with_capacity(things.len());
    for (round, group) in things.chunks(at_once).enumerate() {
        let first = round * at_once;
        if let [only] = group {
            results.push(job(first, only));
        } else {
            let together: Vec<R> = std::thread::scope(|scope| {
                let running: Vec<_> = group
                    .iter()
                    .enumerate()
                    .map(|(offset, thing)| scope.spawn(move || job(first + offset, thing)))
                    .collect();
                running
                    .into_iter()
                    .map(|handle| {
                        handle
                            .join()
                            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                    })
                    .collect()
            });
            results.extend(together);
        }
        if !go_on(&results) {
            return (results, false);
        }
    }
    (results, true)
}

/// Makes `attempt` again after each of `pauses` while it fails with something
/// that may pass. The last failure is returned.
pub(super) fn patiently<T>(
    pauses: &[std::time::Duration],
    mut attempt: impl FnMut() -> Result<T>,
) -> Result<T> {
    let mut pauses = pauses.iter();
    loop {
        let error = match attempt() {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        let text = format!("{error:#}");
        let passing = !NOT_WORTH_WAITING.iter().any(|sign| text.contains(sign));
        match pauses.next() {
            Some(pause) if passing => {
                warn!(
                    "A request failed ({text}); making it again in {}s",
                    pause.as_secs()
                );
                std::thread::sleep(*pause);
            }
            _ => return Err(error),
        }
    }
}

/// A request made after the provider has answered once in this job, when a
/// failure is more likely a passing one than a provider that is not there.
pub(super) fn ask_again(
    input: &Input<'_>,
    system: &str,
    user: &str,
    schema: &Value,
) -> Result<Value> {
    patiently(&PATIENCE, || ask(input, system, user, schema))
}

/// One request, tried again when its answer cannot be read. A provider that
/// cannot be reached fails at once: the caller retries the whole job later.
pub(super) fn ask(input: &Input<'_>, system: &str, user: &str, schema: &Value) -> Result<Value> {
    let request = llm::Request {
        system,
        user,
        schema,
    };
    let mut last = None;
    for attempt in 1..=ATTEMPTS {
        match llm::complete(input.provider, &request, input.secrets) {
            Ok(value) => return Ok(value),
            Err(e) => {
                let unreadable = format!("{e:#}").contains("JSON");
                warn!("Minutes request failed (attempt {attempt}): {e:#}");
                last = Some(e);
                if !unreadable {
                    break;
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("the request failed")))
}

pub(super) const BASE_RULES: &str = "\
꼭 지킬 것:
1. 받아쓴 글에 있는 내용만 씁니다. 짐작으로 빈 곳을 채우지 않습니다.
2. 항목마다 근거가 된 말을 그 줄에서 그대로 옮긴 짧은 인용(quote, 8~40자)을 적습니다. 인용은 한 글자도 바꾸지 않습니다.
3. owner(담당)는 누가 맡았는지 이름이나 직함으로 분명할 때만 적습니다. \"제가 하겠습니다\"라고 말한 사람이 이름으로 적혀 있으면 그 이름을 씁니다. \"화자 2\" 같은 표기는 쓰지 않습니다. 분명하지 않으면 빈 문자열로 둡니다.
4. due(기한)는 기한이 말로 나왔을 때만, 말한 그대로 적습니다(예: \"금요일까지\", \"다음 주 화요일\"). 없으면 빈 문자열로 둡니다.
5. \"검토해 볼게요\", \"생각해 볼게요\", \"일단\", \"아마\", \"견적을 보고 결정할게요\"처럼 확정하지 않은 말은 결정도 할 일도 아닙니다. section을 open으로 합니다.
6. 숫자, 날짜, 금액, 치수, 이름은 받아쓴 글에 나온 그대로 적습니다.
7. 인사, 맞장구, 잡담은 뽑지 않습니다. [회의 끝 …] 줄 뒤의 말은 고객이 나간 뒤의 잡담이나 다른 통화일 수 있습니다. 이 회의의 결정이나 할 일이 분명한 것만 뽑고, 다른 현장·다른 고객 이야기는 뽑지 않습니다.
8. [중요 표시 …] 줄은 녹음한 사람이 그 순간을 중요하다고 표시한 것입니다. 그 앞뒤의 말은 빠뜨리지 않습니다. \"메모:\"가 붙어 있으면 왜 표시했는지 적은 것입니다.
9. text는 회의록에 그대로 실을 수 있는 한 문장으로, 군더더기 없이 씁니다.
10. 할 일 갈래(갈래 목록에 '할 일 갈래'라고 붙은 것)의 text는 '무엇을 하기' 꼴의 짧은 할 일로 씁니다. 예: \"신발장 도면을 다시 그려 보내기\", \"냉장고 크기 재서 알려 주기\". 담당과 기한은 text에 넣지 않고 owner와 due에 적습니다. '~함', '~하기로 했다', '~했습니다'로 끝내지 않습니다.
11. 같은 것을 두고 앞뒤 말이 다르면(예산을 바꿈, 색을 바꿈) 마지막에 정한 말을 따릅니다. 앞의 말은 결정·사실로 뽑지 않고, 꼭 남길 때는 '처음에는 ~였다가 ~로 바꿈'처럼 바뀐 것을 text에 밝힙니다.
12. 이미 정한 것을 나중에 다시 묻거나 다른 안을 물어본 것만으로는 '아직 안 정한 것'이 아닙니다. '다시 생각해 보겠다', '보류하자'처럼 분명히 되돌린 경우만 open으로 합니다.
13. 누구 이야기인지 봅니다. 우리 회사가 했던 다른 현장, 다른 고객, 말하는 사람 자신의 집이나 사무실 이야기(예: \"예전에 한 현장은 40평대였어요\", \"제 집은 꼭대기 층이라\")는 이 회의의 고객·현장 사실(현장, 예산, 요청, 결정)로 옮기지 않습니다. 꼭 적어야 하면 text에 '다른 현장 사례'처럼 누구 이야기인지 밝힙니다.";

pub(super) struct Overview {
    pub(super) template_id: String,
    pub(super) title: String,
    pub(super) speaker_names: Vec<(u32, String)>,
}

/// The kind of recording, a title, and the speakers whose names are said.
/// A failed request falls back to defaults: minutes with "화자 1" are better
/// than none.
pub(super) fn overview(input: &Input<'_>) -> Result<Overview> {
    let transcript = input.transcript;
    let solo = transcript.speakers.len() <= 1;
    let fallback_template = match input.template_id {
        Some(id) => id.to_string(),
        None if solo => SOLO_TEMPLATE.to_string(),
        None => DEFAULT_TEMPLATE.to_string(),
    };

    // The opening and the close say the most about what this is and who is
    // speaking.
    let lines: Vec<String> = transcript
        .paragraphs()
        .iter()
        .map(|p| {
            format!(
                "[{}] {}: {}",
                clock(p.start_ms),
                transcript.speaker_name(p.speaker),
                p.text
            )
        })
        .collect();
    let mut sample = String::new();
    let mut taken = 0;
    for line in &lines {
        if sample.chars().count() > 2_500 {
            break;
        }
        sample.push_str(line);
        sample.push('\n');
        taken += 1;
    }
    if taken < lines.len() {
        sample.push_str("…\n");
        let tail: Vec<&String> = lines[taken..].iter().rev().take(6).collect();
        for line in tail.into_iter().rev() {
            sample.push_str(line);
            sample.push('\n');
        }
    }

    let kinds: String = input
        .templates
        .iter()
        .map(|t| format!("- {}: {} — {}\n", t.id, t.name, t.description))
        .collect();
    let system = format!(
        "당신은 녹음을 받아 적은 글의 앞부분과 끝부분을 보고 세 가지를 정하는 서기입니다.\n\n\
         1. template: 이 녹음의 종류. 아래에서 하나를 고릅니다.\n{kinds}\n\
         2. title: 이 녹음을 한눈에 알아볼 제목. 받아쓴 글에 나온 말로 20자 안쪽으로 짓습니다(예: \"한빛아파트 상담\", \"주간 회의\"). 날짜는 넣지 않습니다.\n\
         3. speakers: 화자가 누구인지 받아쓴 글에서 분명히 알 수 있는 경우만 적습니다. 스스로 이름을 말했거나(\"저는 김서윤 실장입니다\") 다른 사람이 그 화자를 부른 경우입니다. id는 \"화자 2\"의 숫자, name은 이름과 직함(예: \"김서윤 실장\") 또는 이름이 없을 때의 역할(예: \"고객\"), quote는 그렇게 볼 근거가 된 말을 그대로 옮긴 것입니다. 알 수 없는 화자는 넣지 않습니다. 짐작하지 않습니다."
    );
    let people = if input.people.is_empty() {
        String::new()
    } else {
        format!("미리 알려진 참석자: {}\n\n", input.people.join(", "))
    };
    let user = format!("{people}받아쓴 글:\n{sample}");
    let ids: Vec<&str> = input.templates.iter().map(|t| t.id.as_str()).collect();
    let schema = json!({
        "type": "object",
        "properties": {
            "template": { "type": "string", "enum": ids },
            "title": { "type": "string" },
            "speakers": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "integer" },
                        "name": { "type": "string" },
                        "quote": { "type": "string" }
                    },
                    "required": ["id", "name", "quote"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["template", "title", "speakers"],
        "additionalProperties": false
    });

    let answer = match ask(input, &system, &user, &schema) {
        Ok(answer) => answer,
        Err(e) => {
            // Unreachable now is unreachable for the windows too: stop here.
            if !format!("{e:#}").contains("JSON") {
                return Err(e);
            }
            warn!("Overview could not be read; using defaults");
            json!({})
        }
    };

    let template_id = match input.template_id {
        Some(id) => id.to_string(),
        None if solo => SOLO_TEMPLATE.to_string(),
        None => answer
            .get("template")
            .and_then(Value::as_str)
            .filter(|id| input.templates.iter().any(|t| t.id == *id))
            .map_or(fallback_template, str::to_string),
    };
    let title: String = answer
        .get("title")
        .and_then(Value::as_str)
        .map(|title| title.trim().chars().take(40).collect())
        .unwrap_or_default();

    // A name is kept only when the words given for it are in the recording,
    // spoken by or to that speaker's part of the transcript.
    let index = Index::new(transcript);
    let mut speaker_names: Vec<(u32, String)> = Vec::new();
    for speaker in answer
        .get("speakers")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
    {
        let (Some(id), Some(name), Some(quote)) = (
            speaker.get("id").and_then(Value::as_u64),
            speaker.get("name").and_then(Value::as_str),
            speaker.get("quote").and_then(Value::as_str),
        ) else {
            continue;
        };
        let name = name.trim();
        let id = id as u32;
        let known = transcript.speakers.iter().any(|s| s.id == id);
        let fits = !name.is_empty() && name.chars().count() <= 16 && !name.starts_with("화자");
        let said = index
            .find(quote, None)
            .is_some_and(|(_, score)| score >= CLOSE_MATCH);
        let taken = speaker_names.iter().any(|(other, _)| *other == id);
        if known && fits && said && !taken {
            speaker_names.push((id, name.to_string()));
        }
    }
    Ok(Overview {
        template_id,
        title,
        speaker_names,
    })
}

/// An item as the model gave it.
pub(super) struct RawItem {
    pub(super) section: String,
    pub(super) text: String,
    pub(super) owner: String,
    pub(super) due: String,
    /// When the line the model took it from starts.
    pub(super) near_ms: Option<u64>,
    pub(super) quote: String,
}

pub(super) fn extract(
    input: &Input<'_>,
    template: &Template,
    window: &[Line],
    number: usize,
    of: usize,
) -> Result<Vec<RawItem>> {
    match extract_once(input, template, window, number, of) {
        Err(e) if format!("{e:#}").contains(llm::CUT_OFF) && window.len() >= 8 => {
            // The model ran out of room for its answer, which a long
            // meeting with a small model does. Ask about half as much.
            warn!(
                "Window {number} of {of} is too much for the model at once; reading it in two halves"
            );
            let middle = window.len() / 2;
            let mut items = extract(input, template, &window[..middle], number, of)?;
            items.extend(extract(input, template, &window[middle..], number, of)?);
            Ok(items)
        }
        other => other,
    }
}

pub(super) fn extract_once(
    input: &Input<'_>,
    template: &Template,
    window: &[Line],
    number: usize,
    of: usize,
) -> Result<Vec<RawItem>> {
    let (text, starts) = numbered(window);
    if starts.is_empty() {
        return Ok(Vec::new());
    }
    let sections: String = template
        .sections
        .iter()
        .map(|s| {
            format!(
                "- {} ({}){}: {}\n",
                s.key,
                s.title,
                if s.kind == SectionKind::Actions {
                    ", 할 일 갈래"
                } else {
                    ""
                },
                s.instruction
            )
        })
        .collect();
    let system = format!(
        "당신은 녹음을 받아 적은 글에서 회의록에 들어갈 내용을 뽑는 서기입니다. 받아쓴 글은 줄마다 번호가 붙어 있습니다.\n\n\
         {BASE_RULES}\n\n\
         이 녹음의 종류: {} — {}\n{}\n\n뽑을 갈래(section):\n{sections}\n\
         답하는 방법: 1번 줄부터 마지막 줄까지 차례로, 모든 줄에 대해 하나씩 답합니다. line에 줄 번호를 적고, 그 줄에서 뽑은 항목을 items에 넣습니다. 한 줄에서 여러 항목이 나올 수 있습니다. 뽑을 것이 없는 줄은 items를 비워 둡니다. 줄을 건너뛰지 않습니다.",
        template.name, template.description, template.guidance
    );
    let mut user = String::new();
    if !input.recorded_on.is_empty() {
        user.push_str(&format!("녹음한 날: {}\n", input.recorded_on));
    }
    if !input.people.is_empty() {
        user.push_str(&format!("참석자: {}\n", input.people.join(", ")));
    }
    if !input.context.is_empty() {
        // Spellings to trust; never a source of items on its own.
        user.push_str(&format!(
            "녹음한 사람이 미리 알려 준 것(이름·용어의 바른 표기로만 쓰고, 이것만으로 항목을 만들지 않음):\n{}\n",
            input.context
        ));
    }
    user.push_str(&format!(
        "\n받아쓴 글 ({number}/{of}, 1번부터 {}번 줄까지):\n{text}",
        starts.len()
    ));

    let keys: Vec<&str> = template.sections.iter().map(|s| s.key.as_str()).collect();
    let schema = json!({
        "type": "object",
        "properties": {
            "lines": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "line": { "type": "integer" },
                        "items": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "section": { "type": "string", "enum": keys },
                                    "text": { "type": "string" },
                                    "owner": { "type": "string" },
                                    "due": { "type": "string" },
                                    "quote": { "type": "string" }
                                },
                                "required": ["section", "text", "owner", "due", "quote"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["line", "items"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["lines"],
        "additionalProperties": false
    });

    // A model that stops early has read only part of the window. One more
    // try, saying so; the fuller of the two answers is kept.
    let mut best: (usize, Vec<RawItem>) = (0, Vec::new());
    for attempt in 0..2 {
        let nudge = if attempt == 0 {
            String::new()
        } else {
            format!(
                "\n\n앞선 답은 {}번 줄까지만 다뤘습니다. 1번부터 {}번까지 모든 줄에 대해 답해야 합니다.",
                best.0,
                starts.len()
            )
        };
        let answer = ask_again(input, &system, &format!("{user}{nudge}"), &schema)?;
        let (covered, items) = read_extraction(&answer, &starts);
        if covered >= best.0 {
            best = (covered, items);
        }
        if best.0 * 10 >= starts.len() * 8 {
            break;
        }
        warn!(
            "The model answered for {} of {} lines of window {number}",
            best.0,
            starts.len()
        );
    }
    Ok(best.1)
}

/// The items of one extraction answer, and how many of the window's lines it
/// answered for.
pub(super) fn read_extraction(answer: &Value, starts: &[u64]) -> (usize, Vec<RawItem>) {
    let text_of = |item: &Value, key: &str| {
        item.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let mut covered: HashSet<usize> = HashSet::new();
    let mut items: Vec<RawItem> = Vec::new();
    for line in answer
        .get("lines")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
    {
        let Some(number) = line
            .get("line")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .filter(|n| (1..=starts.len()).contains(n))
        else {
            continue;
        };
        covered.insert(number);
        for item in line
            .get("items")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
        {
            let raw = RawItem {
                section: text_of(item, "section"),
                text: text_of(item, "text"),
                owner: text_of(item, "owner"),
                due: text_of(item, "due"),
                near_ms: Some(starts[number - 1]),
                quote: text_of(item, "quote"),
            };
            if !raw.text.is_empty() {
                items.push(raw);
            }
        }
    }
    (covered.len(), items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_extraction_answer_is_read_by_line_number() {
        let answer = json!({"lines": [
            {"line": 1, "items": []},
            {"line": 2, "items": [
                {"section": "budget", "text": "예산은 4500만 원 안쪽", "owner": "", "due": "", "quote": "예산은 4,500만 원"}
            ]},
            {"line": 9, "items": [{"section": "budget", "text": "없는 줄", "owner": "", "due": "", "quote": "x"}]},
            {"line": 3, "items": [{"section": "open", "text": "", "owner": "", "due": "", "quote": ""}]}
        ]});
        let (covered, items) = read_extraction(&answer, &[1_000, 6_000, 11_000]);
        assert_eq!(covered, 3);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].near_ms, Some(6_000));
    }

    #[test]
    fn requests_run_a_few_at_a_time_and_keep_their_order() {
        use std::sync::atomic::AtomicUsize;
        let things: Vec<usize> = (0..10).collect();
        let running = AtomicUsize::new(0);
        let most = AtomicUsize::new(0);
        let rounds = std::cell::Cell::new(0);
        let (results, finished) = in_rounds(
            &things,
            3,
            &|at, thing: &usize| {
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(15));
                running.fetch_sub(1, Ordering::SeqCst);
                (at, thing * 2)
            },
            &|_| {
                rounds.set(rounds.get() + 1);
                true
            },
        );
        assert!(finished);
        assert_eq!(results.len(), 10);
        assert!(results
            .iter()
            .enumerate()
            .all(|(at, &(index, doubled))| index == at && doubled == at * 2));
        // Never more than three at once, and really side by side.
        assert_eq!(most.load(Ordering::SeqCst), 3);
        assert_eq!(rounds.get(), 4);

        // One at a time runs on the caller's own thread.
        let here = std::thread::current().id();
        let (same, _) = in_rounds(
            &things,
            1,
            &|_, _| std::thread::current().id() == here,
            &|_| true,
        );
        assert!(same.iter().all(|same| *same));

        // Saying stop after a round leaves the rest undone.
        let (some, finished) = in_rounds(&things, 3, &|at, _| at, &|done: &[usize]| done.len() < 6);
        assert!(!finished);
        assert_eq!(some, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_request_that_fails_in_passing_is_made_again() {
        let no_wait = [std::time::Duration::ZERO; 2];
        // Twice it fails, then it answers.
        let mut calls = 0;
        let answer = patiently(&no_wait, || {
            calls += 1;
            if calls < 3 {
                Err(anyhow!("Claude Code failed: overloaded"))
            } else {
                Ok(calls)
            }
        });
        assert_eq!(answer.unwrap(), 3);

        // It is not asked for ever.
        let mut calls = 0;
        let answer: Result<()> = patiently(&no_wait, || {
            calls += 1;
            Err(anyhow!("Claude Code failed: overloaded"))
        });
        assert!(answer.is_err());
        assert_eq!(calls, 3);

        // What waiting does not help comes back at once.
        for hopeless in [
            llm::CUT_OFF,
            "Claude Code did not answer within 15 minutes",
            "claude sonnet did not answer with JSON",
            "Claude Code is not installed",
        ] {
            let mut calls = 0;
            let answer: Result<()> = patiently(&no_wait, || {
                calls += 1;
                Err(anyhow!("{hopeless}"))
            });
            assert!(answer.is_err());
            assert_eq!(calls, 1, "{hopeless}");
        }
    }
}
