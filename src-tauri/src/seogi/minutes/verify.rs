//! Numbers of the meeting that disagree, and the checker that reads each
//! entry beside the lines it cites.

use super::*;

/// The most disagreements among the meeting's numbers put to the person.
pub(super) const MOST_DISAGREEMENTS: usize = 6;

/// Finds numbers said in the meeting that disagree with each other (see
/// `amounts::disagreements`), up to what the meeting ended with, and puts
/// each to the person with both places it was said: on the entries that
/// state one of its values and cite one of those lines, or, when none does,
/// as an entry of its own (in the budget section when the template has one,
/// else with what is not settled) that says what does not agree. No number
/// is changed, and none is said to be the right one. Returns how many were
/// found.
pub(super) fn mark_disagreements(
    template: &Template,
    entries: &mut Vec<(String, Entry)>,
    transcript: &Transcript,
    end_ms: Option<u64>,
) -> usize {
    let lines: Vec<&Segment> = transcript
        .segments
        .iter()
        .filter(|segment| end_ms.is_none_or(|end| segment.start_ms < end))
        .collect();
    let texts: Vec<&str> = lines.iter().map(|segment| segment.text.as_str()).collect();
    let speakers: Vec<u32> = lines.iter().map(|segment| segment.speaker).collect();
    let found = super::amounts::disagreements_by(&texts, &speakers);
    let section = if template.section("budget").is_some() {
        "budget"
    } else {
        OPEN_SECTION
    };
    let near = |a: f64, b: f64| (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0);
    // A thing priced two ways by name is put to the person only when the
    // minutes speak of that thing: a name the reading of the words took
    // wrongly ("하나는", "제품은") is in no entry.
    let found: Vec<super::amounts::Disagreement> = found
        .into_iter()
        .filter(|d| {
            d.name.is_empty()
                || entries
                    .iter()
                    .any(|(_, entry)| entry.text.contains(d.name.as_str()))
        })
        .collect();
    for disagreement in found.iter().take(MOST_DISAGREEMENTS) {
        let reason = match disagreement.kind {
            super::amounts::DisagreementKind::Rate => CheckReason::NumbersDiffer,
            _ => CheckReason::SumsDisagree,
        };
        let mut said: Vec<Source> = Vec::new();
        for at in [disagreement.first, disagreement.second] {
            if !said.iter().any(|s| s.ms == lines[at].start_ms) {
                said.push(Source {
                    ms: lines[at].start_ms,
                    quote: lines[at].text.clone(),
                });
            }
        }
        let mut marked = false;
        for (_, entry) in entries.iter_mut() {
            let cites = entry
                .sources
                .iter()
                .any(|source| said.iter().any(|s| s.ms == source.ms));
            let values = super::amounts::values_said(&entry.text);
            let states = disagreement
                .values
                .iter()
                .any(|&v| values.iter().any(|&w| near(v, w)));
            if !cites || !states {
                continue;
            }
            push_reason(entry, reason);
            for source in &said {
                if !entry.sources.iter().any(|s| s.ms == source.ms) {
                    entry.sources.push(source.clone());
                }
            }
            marked = true;
        }
        if !marked {
            entries.push((
                section.to_string(),
                Entry {
                    text: disagreement.note.clone(),
                    owner: String::new(),
                    due: String::new(),
                    sources: said,
                    reasons: vec![reason],
                    done: false,
                    faint: false,
                    original: None,
                    added: false,
                    id: String::new(),
                },
            ));
        }
    }
    found.len().min(MOST_DISAGREEMENTS)
}

/// What the checker said of one entry: its verdict, and whether the owner
/// and the date are in the lines.
pub(super) type Verdict = (String, bool, bool, String);

/// Groups entries of the given sizes (in characters) into requests: in
/// order, each of at most `VERIFY_ENTRIES` entries and `VERIFY_CHARS`
/// characters. An entry longer than that goes alone.
pub(super) fn verify_batches(sizes: &[usize]) -> Vec<Vec<usize>> {
    let mut batches: Vec<Vec<usize>> = Vec::new();
    let mut chars = 0;
    for (position, &size) in sizes.iter().enumerate() {
        let full = batches
            .last()
            .is_none_or(|batch| batch.len() >= VERIFY_ENTRIES || chars + size > VERIFY_CHARS);
        if full {
            batches.push(Vec::new());
            chars = 0;
        }
        if let Some(batch) = batches.last_mut() {
            batch.push(position);
        }
        chars += size;
    }
    batches
}

/// The checker's answer to one request, by the number each entry had in it.
pub(super) fn read_verdicts(answer: &Value) -> BTreeMap<usize, Verdict> {
    let mut verdicts: BTreeMap<usize, Verdict> = BTreeMap::new();
    for result in answer
        .get("results")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
    {
        if let Some(id) = result.get("id").and_then(Value::as_u64) {
            verdicts.insert(
                id as usize,
                (
                    result
                        .get("verdict")
                        .and_then(Value::as_str)
                        .unwrap_or("partly")
                        .to_string(),
                    result
                        .get("owner_stated")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                    result
                        .get("due_stated")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                    result
                        .get("doubt")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                ),
            );
        }
    }
    verdicts
}

/// Applies the checker's answers. `targets` are the entries that were put to
/// it, `batches` how they were grouped (positions in `targets`), `answers`
/// one per batch, `None` where the request failed. Entries the lines do not
/// back are removed (the count is returned); entries backed only in part,
/// or not answered for, are marked.
pub(super) fn apply_verdicts(
    entries: &mut Vec<(String, Entry)>,
    targets: &[usize],
    batches: &[Vec<usize>],
    answers: &[Option<BTreeMap<usize, Verdict>>],
) -> u32 {
    let mut remove: Vec<usize> = Vec::new();
    for (batch, answer) in batches.iter().zip(answers) {
        for (number, &position) in batch.iter().enumerate() {
            let at = targets[position];
            let entry = &mut entries[at].1;
            match answer
                .as_ref()
                .and_then(|verdicts| verdicts.get(&(number + 1)))
            {
                Some((verdict, owner_stated, due_stated, doubt)) => {
                    if !owner_stated {
                        entry.owner.clear();
                    }
                    if !due_stated {
                        entry.due.clear();
                    }
                    match verdict.as_str() {
                        "supported" => {}
                        "unsupported" => remove.push(at),
                        // What exactly is in doubt, so the person knows what
                        // to listen for.
                        _ => push_reason(
                            entry,
                            match doubt.as_str() {
                                "who" => CheckReason::OwnerUnclear,
                                "when" => CheckReason::DueUnclear,
                                "decided" => CheckReason::NotSettled,
                                "differs" => CheckReason::SaidDifferently,
                                "number" => CheckReason::NumbersDiffer,
                                "subject" => CheckReason::OtherSubject,
                                _ => CheckReason::WeakSupport,
                            },
                        ),
                    }
                }
                // No answer for it is not a pass. Unverified is not the same
                // as verified: the entry says so.
                None => push_reason(entry, CheckReason::Unchecked),
            }
        }
    }
    remove.sort_unstable();
    let removed = remove.len() as u32;
    for at in remove.into_iter().rev() {
        entries.remove(at);
    }
    removed
}

/// Shows the model each task and decision beside the lines it cites and asks
/// whether those lines say it, a batch at a time. Entries they do not back
/// are removed (the count is returned); entries they back only in part are
/// marked. `None` if `cancelled` was set along the way.
pub(super) fn verify(
    input: &Input<'_>,
    template: &Template,
    entries: &mut Vec<(String, Entry)>,
    index: &Index<'_>,
    transcript: &Transcript,
    progress: &dyn Fn(f32),
    cancelled: &AtomicBool,
) -> Option<u32> {
    // What states a commitment: tasks, and anything in a section named as
    // decided; and the site, which is easily taken from someone else's
    // story. Notes on what was discussed are not put to the question.
    let targets: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, (key, _))| commits(template, key) || key == SITE_SECTION)
        .map(|(i, _)| i)
        .collect();
    if targets.is_empty() {
        return Some(0);
    }

    // Each entry as the checker reads it, without its number.
    let blocks: Vec<String> = targets
        .iter()
        .map(|&at| {
            let (_, entry) = &entries[at];
            let mut block = format!("주장: {}", entry.text);
            if !entry.owner.is_empty() {
                block.push_str(&format!(" / 담당: {}", entry.owner));
            }
            if !entry.due.is_empty() {
                block.push_str(&format!(" / 기한: {}", entry.due));
            }
            block.push_str("\n근거가 된 대목:\n");
            for source in entry.sources.iter().take(VERIFY_SOURCES) {
                let at = index
                    .segments
                    .iter()
                    .position(|segment| segment.start_ms == source.ms)
                    .unwrap_or(0);
                let first = at.saturating_sub(2);
                let last = (at + 3).min(index.segments.len() - 1);
                for segment in &index.segments[first..=last] {
                    block.push_str(&format!(
                        "[{}] {}: {}\n",
                        clock(segment.start_ms),
                        transcript.speaker_name(segment.speaker),
                        segment.text
                    ));
                }
            }
            block
        })
        .collect();
    let sizes: Vec<usize> = blocks.iter().map(|block| block.chars().count()).collect();
    let batches = verify_batches(&sizes);

    let system = "당신은 회의록을 검수하는 사람입니다. 회의록에 실릴 주장과, 그 근거로 든 녹음의 대목이 번호별로 주어집니다. 대목에 적힌 말만 보고 판정합니다.\n\n\
         verdict:\n\
         - supported: 대목이 그 주장을 분명히 말하고 있다.\n\
         - partly: 비슷한 말은 있지만 확정된 것인지, 누가 하는지, 내용이 맞는지가 분명하지 않다.\n\
         - unsupported: 대목에 그런 말이 없다.\n\
         owner_stated: 주장에 담당이 적혀 있다면, 그 사람이 맡는다는 것이 대목에서 분명한가. 담당이 없으면 true.\n\
         due_stated: 주장에 기한이 적혀 있다면, 그 기한이 대목에 나오는가. 기한이 없으면 true.\n\
         doubt: partly일 때 무엇이 분명하지 않은지 하나를 고릅니다. who(누가 할지 말이 없음), when(기한이 말에 없음), decided(정한 것인지 이야기만 한 것인지 분명하지 않음), differs(말한 내용과 조금 다르게 적혔음), number(금액이나 숫자가 다르게 나옴), subject(다른 현장·다른 사람·말하는 사람 자기 집 이야기를 이 회의 고객의 일처럼 적었을 수 있음), other(그 밖). supported나 unsupported면 none.\n\
         대목이 다른 현장이나 말하는 사람 자신의 집 이야기인데 주장이 이 회의 고객의 사실로 적혀 있으면 unsupported입니다.\n\n\
         모든 번호에 대해 하나씩 답합니다.";
    let schema = json!({
        "type": "object",
        "properties": {
            "results": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "integer" },
                        "verdict": { "type": "string", "enum": ["supported", "partly", "unsupported"] },
                        "owner_stated": { "type": "boolean" },
                        "due_stated": { "type": "boolean" },
                        "doubt": { "type": "string", "enum": ["none", "who", "when", "decided", "differs", "number", "subject", "other"] }
                    },
                    "required": ["id", "verdict", "owner_stated", "due_stated", "doubt"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["results"],
        "additionalProperties": false
    });

    // A checker that cannot be reached is not asked again for the rest.
    let unreachable = AtomicBool::new(false);
    let total = batches.len();
    let (answers, finished) = in_rounds(
        &batches,
        input.provider.at_once(),
        &|_, batch: &Vec<usize>| {
            if unreachable.load(Ordering::Acquire) {
                return None;
            }
            let listing: String = batch
                .iter()
                .enumerate()
                .map(|(number, &position)| format!("### {}\n{}\n", number + 1, blocks[position]))
                .collect();
            match ask_again(input, system, &listing, &schema) {
                Ok(answer) => Some(read_verdicts(&answer)),
                Err(e) => {
                    warn!(
                        "Could not verify {} entries ({e:#}); marking them for a look",
                        batch.len()
                    );
                    if !format!("{e:#}").contains("JSON") {
                        unreachable.store(true, Ordering::Release);
                    }
                    None
                }
            }
        },
        &|answers: &[Option<BTreeMap<usize, Verdict>>]| {
            progress(0.85 + 0.15 * answers.len() as f32 / total.max(1) as f32);
            !cancelled.load(Ordering::Acquire)
        },
    );
    if !finished {
        return None;
    }
    Some(apply_verdicts(entries, &targets, &batches, &answers))
}

#[cfg(test)]
mod tests {
    use super::super::test_help::*;
    use super::*;
    use crate::seogi::templates::built_in;

    #[test]
    fn a_long_meetings_entries_are_checked_a_batch_at_a_time() {
        // Ninety entries of some 300 characters: no request over the limits,
        // every entry in exactly one, in order.
        let sizes = vec![300usize; 90];
        let batches = verify_batches(&sizes);
        assert!(batches.len() >= 6);
        for batch in &batches {
            assert!(batch.len() <= VERIFY_ENTRIES);
            assert!(batch.iter().map(|&at| sizes[at]).sum::<usize>() <= VERIFY_CHARS);
        }
        let all: Vec<usize> = batches.iter().flatten().copied().collect();
        assert_eq!(all, (0..90).collect::<Vec<_>>());

        // Short entries are held to the count, and one too long for a
        // request still gets asked about, alone.
        assert!(verify_batches(&[40; 50])
            .iter()
            .all(|batch| batch.len() <= VERIFY_ENTRIES));
        assert_eq!(
            verify_batches(&[200, 9_000, 200]),
            vec![vec![0], vec![1], vec![2]]
        );
        assert!(verify_batches(&[]).is_empty());
    }

    #[test]
    fn a_failed_batch_marks_only_its_own_entries() {
        let entry = |text: &str| {
            let mut entry = item("decisions", text, 1, 1_000).entry;
            entry.owner = "김서윤 실장".into();
            entry.due = "금요일".into();
            ("decisions".to_string(), entry)
        };
        // Five entries; the second is a note that is not put to the checker.
        let mut entries = vec![
            entry("가"),
            (
                "discussed".to_string(),
                item("discussed", "나", 1, 2_000).entry,
            ),
            entry("다"),
            entry("라"),
            entry("마"),
        ];
        let targets = vec![0, 2, 3, 4];
        // Two requests of two; numbers start at 1 in each.
        let batches = vec![vec![0, 1], vec![2, 3]];
        let first: BTreeMap<usize, Verdict> = BTreeMap::from([
            (
                1,
                ("supported".to_string(), true, false, "none".to_string()),
            ),
            (
                2,
                ("unsupported".to_string(), true, true, "none".to_string()),
            ),
        ]);
        // The second request failed.
        let removed = apply_verdicts(&mut entries, &targets, &batches, &[Some(first), None]);

        assert_eq!(removed, 1);
        let texts: Vec<&str> = entries.iter().map(|(_, e)| e.text.as_str()).collect();
        assert_eq!(texts, ["가", "나", "라", "마"]);
        // Backed, but its date was not in the lines.
        assert!(entries[0].1.reasons.is_empty());
        assert_eq!(entries[0].1.owner, "김서윤 실장");
        assert!(entries[0].1.due.is_empty());
        // The note was never in question.
        assert!(entries[1].1.reasons.is_empty());
        // The failed request's entries are marked, not passed.
        assert_eq!(entries[2].1.reasons, vec![CheckReason::Unchecked]);
        assert_eq!(entries[3].1.reasons, vec![CheckReason::Unchecked]);

        // An answer that skips a number leaves that entry marked.
        let mut entries = vec![entry("가"), entry("나")];
        let partial: BTreeMap<usize, Verdict> =
            BTreeMap::from([(2, ("partly".to_string(), true, true, "who".to_string()))]);
        let removed = apply_verdicts(&mut entries, &[0, 1], &[vec![0, 1]], &[Some(partial)]);
        assert_eq!(removed, 0);
        assert_eq!(entries[0].1.reasons, vec![CheckReason::Unchecked]);
        // What is in doubt is said: here, who does it.
        assert_eq!(entries[1].1.reasons, vec![CheckReason::OwnerUnclear]);
    }

    #[test]
    fn numbers_that_disagree_go_to_a_look_with_both_places() {
        let transcript = meeting_of(&[
            (1_000, 1, "바닥 강마루는 평당 12만 원으로 잡았습니다."),
            (
                6_000,
                1,
                "철거 300만 원, 목공 450만 원, 합쳐서 800만 원입니다.",
            ),
            (
                11_000,
                1,
                "이 3m 48만 원이고요, 막는 벽은 각각 1m씩 32만 원입니다.",
            ),
            (16_000, 1, "강마루는 평당 15만 원 기준이고요."),
            (21_000, 1, "안녕히 가세요."),
            (26_000, 1, "타일은 평당 8만 원이고 10평이라 90만 원이에요."),
        ]);
        let mut entries = vec![
            (
                "budget".to_string(),
                task("강마루는 평당 15만 원이다", "", "", 16_000, vec![]),
            ),
            // Copies only a part of what was said ("1m씩"): found too.
            (
                "materials".to_string(),
                task("막는 벽은 각각 1m씩이다", "", "", 11_000, vec![]),
            ),
            // Cites a line of the disagreement but states none of its
            // numbers: left alone.
            (
                "requests".to_string(),
                task("고객은 강마루를 원함", "", "", 1_000, vec![]),
            ),
        ];
        // The meeting ended before the last line: it is not read.
        let found = mark_disagreements(&consult(), &mut entries, &transcript, Some(26_000));
        assert_eq!(found, 3);
        assert_eq!(entries.len(), 4);
        // The entry that states the price gets the doubt and both places.
        assert_eq!(entries[0].1.reasons, [CheckReason::NumbersDiffer]);
        assert_eq!(
            entries[0]
                .1
                .sources
                .iter()
                .map(|s| s.ms)
                .collect::<Vec<_>>(),
            [16_000, 1_000]
        );
        assert_eq!(entries[1].1.reasons, [CheckReason::NumbersDiffer]);
        assert!(entries[2].1.reasons.is_empty());
        // The sum no entry states becomes one of its own, saying what does
        // not agree; the numbers said are not changed.
        let (section, sum) = &entries[3];
        assert_eq!(section, "budget");
        assert_eq!(
            sum.text,
            "300만 원 + 450만 원을 더하면 750만 원인데 800만 원으로 말함"
        );
        assert_eq!(sum.reasons, [CheckReason::SumsDisagree]);
        assert_eq!(
            sum.sources[0].quote,
            "철거 300만 원, 목공 450만 원, 합쳐서 800만 원입니다."
        );

        // A template with no budget puts it with what is not settled. A
        // thing priced two ways by name that no entry speaks of is left
        // alone; the sums and products are told.
        let meeting = built_in().into_iter().find(|t| t.id == "meeting").unwrap();
        let mut none: Vec<(String, Entry)> = Vec::new();
        mark_disagreements(&meeting, &mut none, &transcript, None);
        assert!(none.iter().all(|(key, _)| key == OPEN_SECTION));
        assert_eq!(none.len(), 3);
        // Numbers that disagree are must-hear: never shown faintly as fact.
        assert!(MUST_BE_HEARD.contains(&CheckReason::NumbersDiffer));
    }
}
