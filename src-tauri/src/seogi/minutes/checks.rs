//! The machine's checks on each item, putting disagreeing entries to the
//! model (reconcile), and merging items that say the same thing.

use super::*;

/// An item that passed the machine's checks.
#[derive(Debug, Clone)]
pub(super) struct Item {
    pub(super) section: String,
    pub(super) entry: Entry,
    /// The transcript lines it cites.
    pub(super) lines: std::ops::RangeInclusive<usize>,
}

/// Checks one item against the transcript. `None` when the recording does
/// not back it.
pub(super) fn check_item(
    raw: RawItem,
    template: &Template,
    index: &Index<'_>,
    transcript: &Transcript,
) -> Option<Item> {
    let section = template.section(&raw.section)?.key.clone();
    let (lines, score) = index.find(&raw.quote, raw.near_ms)?;
    let mut reasons: Vec<CheckReason> = Vec::new();
    if score < CLOSE_MATCH {
        reasons.push(CheckReason::LooseQuote);
    }

    let cited = &index.segments[lines.clone()];
    if cited
        .iter()
        .any(|line| line.unsure || line.confidence.is_some_and(|c| c < index.unsure_below))
    {
        reasons.push(CheckReason::UnsureSpeech);
    }
    if cited.iter().any(|line| line.overlap) {
        reasons.push(CheckReason::OverlappingSpeech);
    }

    // Every number the item states must be in the lines around its quote,
    // as the same digits or the same value however said ("아홉 평" is 9평).
    let stated = format!("{} {}", raw.text, raw.due);
    if !super::amounts::backed(&stated, &index.text_around(&lines, 2)) {
        reasons.push(CheckReason::NumberNotInSource);
    }
    // An amount no one says that way, in the item or in the words it copies
    // ("1억 5천억", "80,900만원"): the speech model may have misheard it.
    let quoted = index.text_around(&lines, 0);
    if super::amounts::odd_units(&raw.text)
        || (!numbers_in(&raw.text).is_empty() && super::amounts::odd_units(&quoted))
    {
        reasons.push(CheckReason::UnitOdd);
    }
    // The same rate said with another value close by ("자당 20", "자당 25만").
    if super::amounts::rates_differ(&raw.text, &index.text_around(&lines, 3)) {
        reasons.push(CheckReason::NumbersDiffer);
    }

    // An owner is a person: a name the recording has, never a speaker label.
    let owner = raw.owner.trim();
    let owner_known = !owner.is_empty()
        && !owner.starts_with("화자")
        && (transcript.speakers.iter().any(|s| s.name == owner)
            || owner.split_whitespace().next().is_some_and(|first| {
                first.chars().count() >= 2 && index.text.contains(&bare(first))
            }));
    let first = &index.segments[*lines.start()];
    // The quote as transcribed, not as the model retyped it.
    let quote = if score >= 1.0 {
        raw.quote.clone()
    } else {
        first.text.clone()
    };
    Some(Item {
        section,
        entry: Entry {
            text: raw.text,
            owner: if owner_known {
                owner.to_string()
            } else {
                String::new()
            },
            due: raw.due,
            sources: vec![Source {
                ms: first.start_ms,
                quote,
            }],
            reasons,
            done: false,
            faint: false,
            original: None,
            added: false,
            id: String::new(),
        },
        lines,
    })
}

/// Marks the entries whose changes of an amount do not add up: one says it
/// went from A to B, another (or the same) by C, near B − A but not it.
pub(super) fn mark_sums(entries: &mut [(String, Entry)]) {
    let texts: Vec<&str> = entries.iter().map(|(_, e)| e.text.as_str()).collect();
    for at in super::amounts::sums_disagree(&texts) {
        push_reason(&mut entries[at].1, CheckReason::SumsDisagree);
    }
}

/// Whether everything an item rests on was said after the meeting ended.
pub(super) fn after_the_end(item: &Item, index: &Index<'_>, end_ms: Option<u64>) -> bool {
    end_ms.is_some_and(|end| index.segments[*item.lines.start()].start_ms >= end)
}

/// Marks a fact or a choice whose words talk about another site or person
/// (a past case, the speaker's own home): it may be someone else's story
/// moved onto this meeting's customer. Tasks are not marked: who does them
/// is checked on its own.
pub(super) fn mark_elsewhere(
    item: &mut Item,
    template: &Template,
    index: &Index<'_>,
    transcript: &Transcript,
    customer: &str,
) {
    if template
        .section(&item.section)
        .is_some_and(|section| section.kind == SectionKind::Actions)
    {
        return;
    }
    let cited = &index.segments[item.lines.clone()];
    let words: Vec<&str> = cited.iter().map(|line| line.text.as_str()).collect();
    let names: Vec<String> = cited
        .iter()
        .map(|line| transcript.speaker_name(line.speaker))
        .collect();
    // A meeting with a customer: one is named, or the template has their
    // side.
    let has_customer = !customer.trim().is_empty() || template.section("customer_tasks").is_some();
    let who = super::facts::who_said(&names, customer, has_customer);
    if super::facts::about_someone_else(&item.entry.text, &words.join(" "), who, has_customer) {
        push_reason(&mut item.entry, CheckReason::OtherSubject);
    }
}

/// Doubts that keep an entry out of the story and the summary: it may not
/// have been said so, may be someone else's, or carries a number that may
/// be wrong; stating it there would undo the "확인 필요" beside it.
pub(super) const DOUBTS_NOT_TOLD: [CheckReason; 8] = [
    CheckReason::AfterTheEnd,
    CheckReason::WeakSupport,
    CheckReason::Conflicting,
    CheckReason::OtherSubject,
    CheckReason::NumberNotInSource,
    CheckReason::NumbersDiffer,
    CheckReason::UnitOdd,
    CheckReason::SumsDisagree,
];

/// The most pairs of entries put to the model as possibly disagreeing.
pub(super) const MOST_CLASHES: usize = 20;

/// Finds entries that may say different things about the same matter and
/// asks the model, for each pair, which stands: the later word when it
/// changed what was said before; the earlier when the later only asked
/// about it again; both when they are about different things. A pair it
/// cannot settle is marked for a look on both sides; one it could not be
/// asked about is left as written. Returns how many entries were left out,
/// and whether the request failed, so the minutes can say some were not
/// checked again.
pub(super) fn reconcile(
    input: &Input<'_>,
    template: &Template,
    entries: &mut Vec<(String, Entry)>,
    index: &Index<'_>,
    transcript: &Transcript,
) -> (u32, bool) {
    // Facts and choices; tasks are read together later.
    let candidates: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, (key, _))| {
            template
                .section(key)
                .is_some_and(|section| section.kind != SectionKind::Actions)
        })
        .map(|(at, _)| at)
        .collect();
    let said: Vec<super::facts::Said<'_>> = candidates
        .iter()
        .map(|&at| super::facts::Said {
            section: entries[at].0.as_str(),
            text: entries[at].1.text.as_str(),
            ms: entries[at].1.sources.first().map_or(0, |s| s.ms),
        })
        .collect();
    let pairs: Vec<(usize, usize, super::facts::Clash)> =
        super::facts::clashes(&said, OPEN_SECTION, MOST_CLASHES)
            .into_iter()
            .map(|(x, y, clash)| (candidates[x], candidates[y], clash))
            .collect();
    if pairs.is_empty() {
        return (0, false);
    }

    // What was said around an entry, and a little after: a later word may
    // be the one that settles it.
    let around = |entry: &Entry, after: usize| -> String {
        let ms = entry.sources.last().map_or(0, |s| s.ms);
        let at = index
            .segments
            .iter()
            .position(|segment| segment.start_ms >= ms)
            .unwrap_or(0);
        let first = at.saturating_sub(1);
        let last = (at + after).min(index.segments.len() - 1);
        index.segments[first..=last]
            .iter()
            .map(|segment| {
                format!(
                    "[{}] {}: {}\n",
                    clock(segment.start_ms),
                    transcript.speaker_name(segment.speaker),
                    segment.text
                )
            })
            .collect()
    };
    let section_title = |key: &str| {
        template
            .section(key)
            .map_or(key.to_string(), |s| s.title.clone())
    };
    let listing: String = pairs
        .iter()
        .enumerate()
        .map(|(number, &(a, b, _))| {
            let (ka, ea) = &entries[a];
            let (kb, eb) = &entries[b];
            format!(
                "### {}\n먼저 나온 것 [{}] {}\n{}나중에 나온 것 [{}] {}\n{}\n",
                number + 1,
                section_title(ka),
                ea.text,
                around(ea, 2),
                section_title(kb),
                eb.text,
                around(eb, 4),
            )
        })
        .collect();
    let system = "당신은 회의록을 검수하는 사람입니다. 회의록에 실릴 항목 가운데 같은 것을 두고 서로 다르게 말하는 것 같은 두 항목이 번호별로, 녹음의 그 대목과 함께 주어집니다. 대목에 적힌 말만 보고 판정합니다.\n\n\
         verdict:\n\
         - later: 나중 말이 앞의 말을 바꿨다(예산을 올림, 색을 바꿈, 다시 생각해 보겠다고 분명히 되돌림). 나중 것만 남깁니다.\n\
         - earlier: 앞에서 정한 것이 그대로다. 나중 것은 다시 물어보거나 다른 안을 물어본 것일 뿐 바꾸지 않았거나, 나중 것이 다른 사람·다른 현장 이야기다. 앞의 것만 남깁니다.\n\
         - both: 서로 다른 것에 관한 말이라 둘 다 맞다.\n\
         - unclear: 어느 쪽이 맞는지 대목만으로는 알 수 없다.\n\n\
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
                        "verdict": { "type": "string", "enum": ["later", "earlier", "both", "unclear"] }
                    },
                    "required": ["id", "verdict"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["results"],
        "additionalProperties": false
    });
    let mut failed = false;
    let verdicts: BTreeMap<usize, String> = match ask_again(input, system, &listing, &schema) {
        Ok(answer) => answer
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|result| {
                Some((
                    result.get("id")?.as_u64()? as usize,
                    result.get("verdict")?.as_str()?.to_string(),
                ))
            })
            .collect(),
        Err(e) => {
            warn!(
                "Could not ask which of {} pairs stands ({e:#}); leaving them as written",
                pairs.len()
            );
            failed = true;
            BTreeMap::new()
        }
    };
    // An answer that leaves pairs out is no answer for them.
    failed |= unanswered(pairs.len(), &verdicts);
    (apply_reconciled(entries, &pairs, &verdicts), failed)
}

/// Whether some of `pairs` pairs (numbered from 1) got no verdict: the
/// answer had no results, or skipped a number.
pub(super) fn unanswered(pairs: usize, verdicts: &BTreeMap<usize, String>) -> bool {
    (1..=pairs).any(|number| !verdicts.contains_key(&number))
}

/// Makes the verdicts so: the entry that does not stand goes, and a pair
/// found unsettled is marked on both sides. Returns how many went.
pub(super) fn apply_reconciled(
    entries: &mut Vec<(String, Entry)>,
    pairs: &[(usize, usize, super::facts::Clash)],
    verdicts: &BTreeMap<usize, String>,
) -> u32 {
    let mut gone: HashSet<usize> = HashSet::new();
    for (number, &(earlier, later, _)) in pairs.iter().enumerate() {
        match verdicts.get(&(number + 1)).map(String::as_str) {
            Some("later") => {
                gone.insert(earlier);
            }
            Some("earlier") => {
                gone.insert(later);
            }
            // Not to be told from the words: said both ways, so a person
            // hears it.
            Some("unclear") => {
                push_reason(&mut entries[earlier].1, CheckReason::Conflicting);
                push_reason(&mut entries[later].1, CheckReason::Conflicting);
            }
            // Both stand, or the checker gave no answer for it (or could not
            // be asked): the pair may well not disagree, so it is left as
            // written rather than marked on a guess.
            _ => {}
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

/// The runs of digits in `text`, with grouping commas removed: `4,500` → `4500`.
pub(super) fn numbers_in(text: &str) -> Vec<String> {
    let mut numbers = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_digit() {
            current.push(c);
        } else if c == ','
            && !current.is_empty()
            && chars.get(i + 1).is_some_and(char::is_ascii_digit)
        {
            // A grouping comma inside a number.
        } else if !current.is_empty() {
            numbers.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        numbers.push(current);
    }
    numbers
}

/// `text` with everything but digits turned to spaces and grouping commas
/// closed up, so a stated number can be looked for as a whole.
pub(super) fn digits_of(text: &str) -> String {
    numbers_in(text).join(" ")
}

/// How alike two texts are, from 0 to 1: the share of their two-character
/// pieces they have in common.
pub(super) fn likeness(a: &str, b: &str) -> f32 {
    let pieces = |text: &str| -> HashSet<(char, char)> {
        let chars: Vec<char> = bare(text).chars().collect();
        chars.windows(2).map(|pair| (pair[0], pair[1])).collect()
    };
    let (a, b) = (pieces(a), pieces(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    2.0 * a.intersection(&b).count() as f32 / (a.len() + b.len()) as f32
}

/// Two items say the same thing when they are in the same section and their
/// texts are mostly the same, or nearly the same and about the same line.
/// Numbers must agree: "40만 원" and "280만 원" are different facts however
/// alike the sentences around them.
pub(super) fn same_thing(a: &Item, b: &Item) -> bool {
    if a.section != b.section {
        return false;
    }
    let alike = likeness(&a.entry.text, &b.entry.text);
    let same_line = a.lines.start() == b.lines.start();
    let numbers_agree = {
        let mut x = numbers_in(&a.entry.text);
        let mut y = numbers_in(&b.entry.text);
        x.sort();
        y.sort();
        x == y
    };
    numbers_agree && (alike >= 0.75 || (same_line && alike >= 0.5))
}

pub(super) struct Composed {
    /// `(section key, entry)` in the order of the recording.
    pub(super) entries: Vec<(String, Entry)>,
}

/// Merges the items that say the same thing. The summary is written later,
/// from what is left once everything is checked.
pub(super) fn compose(items: Vec<Item>) -> Composed {
    let mut merged: Vec<Item> = Vec::new();
    for item in items {
        match merged.iter_mut().find(|kept| same_thing(kept, &item)) {
            Some(kept) => {
                for source in item.entry.sources {
                    if !kept.entry.sources.iter().any(|s| s.ms == source.ms) {
                        kept.entry.sources.push(source);
                    }
                }
                kept.entry.sources.sort_by_key(|source| source.ms);
                for reason in item.entry.reasons {
                    push_reason(&mut kept.entry, reason);
                }
                // What one telling leaves out, the other may give.
                if kept.entry.owner.is_empty() {
                    kept.entry.owner = item.entry.owner;
                }
                if kept.entry.due.is_empty() {
                    kept.entry.due = item.entry.due;
                }
            }
            None => merged.push(item),
        }
    }
    merged.sort_by_key(|item| item.entry.sources.first().map_or(0, |source| source.ms));
    Composed {
        entries: merged
            .into_iter()
            .map(|item| (item.section, item.entry))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_help::*;
    use super::*;
    use crate::seogi::templates::built_in;
    use crate::seogi::transcript::Speaker;

    #[test]
    fn an_item_takes_its_time_from_where_its_quote_is_found() {
        let transcript = transcript();
        let index = Index::new(&transcript);
        let template = built_in().remove(1);
        // The model gives the wrong time; the quote says where it really is.
        let item = check_item(
            raw(
                "our_tasks",
                "신발장 도면을 다시 그려 금요일까지 보낸다",
                "김서윤 실장",
                "금요일까지",
                "00:00:01",
                "신발장 도면을 다시 그려서 금요일까지",
            ),
            &template,
            &index,
            &transcript,
        )
        .unwrap();
        assert_eq!(item.entry.sources[0].ms, 11_000);
        assert_eq!(item.entry.owner, "김서윤 실장");
        assert!(item.entry.reasons.is_empty());
    }

    #[test]
    fn an_item_whose_quote_is_not_in_the_recording_is_dropped() {
        let transcript = transcript();
        let index = Index::new(&transcript);
        let template = built_in().remove(1);
        assert!(check_item(
            raw(
                "decisions",
                "욕실은 전체 철거하기로 했다",
                "",
                "",
                "00:00:06",
                "욕실은 전체 철거하는 걸로 하죠"
            ),
            &template,
            &index,
            &transcript
        )
        .is_none());
    }

    #[test]
    fn a_number_that_is_not_in_the_cited_lines_marks_the_item() {
        let transcript = transcript();
        let index = Index::new(&transcript);
        let template = built_in().remove(1);
        let right = check_item(
            raw(
                "budget",
                "예산은 4500만 원 안쪽",
                "",
                "",
                "00:00:06",
                "예산은 4,500만 원 안쪽으로",
            ),
            &template,
            &index,
            &transcript,
        )
        .unwrap();
        assert!(right.entry.reasons.is_empty());
        let wrong = check_item(
            raw(
                "budget",
                "예산은 5400만 원 안쪽",
                "",
                "",
                "00:00:06",
                "예산은 4,500만 원 안쪽으로",
            ),
            &template,
            &index,
            &transcript,
        )
        .unwrap();
        assert_eq!(wrong.entry.reasons, vec![CheckReason::NumberNotInSource]);
    }

    #[test]
    fn a_speaker_label_is_never_an_owner() {
        let transcript = transcript();
        let index = Index::new(&transcript);
        let template = built_in().remove(1);
        let item = check_item(
            raw(
                "customer_tasks",
                "신발장 폭을 줄일지 생각해 본다",
                "화자 2",
                "",
                "00:00:16",
                "일단 생각해 볼게요",
            ),
            &template,
            &index,
            &transcript,
        )
        .unwrap();
        assert_eq!(item.entry.owner, "");
        // A name the recording never has is not an owner either.
        let item = check_item(
            raw(
                "our_tasks",
                "도면을 보낸다",
                "한지안 대리",
                "",
                "00:00:11",
                "금요일까지 보내드리겠습니다",
            ),
            &template,
            &index,
            &transcript,
        )
        .unwrap();
        assert_eq!(item.entry.owner, "");
    }

    #[test]
    fn an_unsure_line_marks_what_rests_on_it() {
        let mut transcript = transcript();
        transcript.segments[2].unsure = true;
        let index = Index::new(&transcript);
        let template = built_in().remove(1);
        let item = check_item(
            raw(
                "our_tasks",
                "도면을 보낸다",
                "",
                "",
                "00:00:11",
                "금요일까지 보내드리겠습니다",
            ),
            &template,
            &index,
            &transcript,
        )
        .unwrap();
        assert_eq!(item.entry.reasons, vec![CheckReason::UnsureSpeech]);
    }

    #[test]
    fn numbers_are_read_whole_with_grouping_commas() {
        assert_eq!(
            numbers_in("예산 4,500만 원, 12월 8일"),
            vec!["4500", "12", "8"]
        );
        assert_eq!(numbers_in("숫자 없음"), Vec::<String>::new());
        assert!(digits_of("폭 2400에 280만 원").contains("2400"));
    }

    #[test]
    fn the_same_thing_said_twice_is_one_entry_but_different_facts_stay_apart() {
        let a = item(
            "decisions",
            "방 3개와 거실은 강마루로 하기로 했다",
            5,
            88_000,
        );
        let b = item("decisions", "방 3개와 거실은 강마루로 한다", 5, 88_000);
        assert!(same_thing(&a, &b));
        // Alike sentences with different numbers are different facts.
        let c = item(
            "budget",
            "붙박이장은 슬라이딩으로 하면 280만 원 정도이다",
            9,
            134_000,
        );
        let d = item(
            "budget",
            "붙박이장은 여닫이로 하면 40만 원 정도 줄어든다",
            10,
            145_000,
        );
        assert!(!same_thing(&c, &d));
        // The same words in different sections are not merged.
        let e = item(
            "requests",
            "방 3개와 거실은 강마루로 하기로 했다",
            5,
            88_000,
        );
        assert!(!same_thing(&a, &e));
        // Different topics in one section stay apart.
        let f = item(
            "decisions",
            "샷시는 교체하지 않고 실리콘만 다시 쏜다",
            12,
            152_000,
        );
        assert!(!same_thing(&a, &f));
    }

    #[test]
    fn the_later_word_stands_and_an_unsettled_pair_is_marked_both_ways() {
        use crate::seogi::facts::Clash;
        let mut entries = vec![
            (
                "budget".to_string(),
                task("예산은 4,000만 원", "", "", 1_000, vec![]),
            ),
            (
                "budget".to_string(),
                task("예산은 5,000만 원", "", "", 9_000, vec![]),
            ),
            (
                "decisions".to_string(),
                task("방은 4개로 유지", "", "", 2_000, vec![]),
            ),
            (
                "open".to_string(),
                task("방 개수는 아직 미정", "", "", 8_000, vec![]),
            ),
            (
                "site".to_string(),
                task("현장은 32평", "", "", 3_000, vec![]),
            ),
            (
                "site".to_string(),
                task("현장은 58평", "", "", 7_000, vec![]),
            ),
        ];
        let pairs = vec![
            (0, 1, Clash::Numbers),
            (2, 3, Clash::SettledAndOpen),
            (4, 5, Clash::Numbers),
        ];
        // The budget was raised; the rooms were only asked about again; the
        // size could not be told.
        let verdicts: BTreeMap<usize, String> = [
            (1, "later".to_string()),
            (2, "earlier".to_string()),
            (3, "unclear".to_string()),
        ]
        .into();
        let gone = apply_reconciled(&mut entries, &pairs, &verdicts);
        assert_eq!(gone, 2);
        let texts: Vec<&str> = entries.iter().map(|(_, e)| e.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "예산은 5,000만 원",
                "방은 4개로 유지",
                "현장은 32평",
                "현장은 58평"
            ]
        );
        assert_eq!(entries[2].1.reasons, [CheckReason::Conflicting]);
        assert_eq!(entries[3].1.reasons, [CheckReason::Conflicting]);
        assert!(entries[0].1.reasons.is_empty());
    }

    #[test]
    fn what_is_said_after_the_meeting_ended_is_marked_for_a_look() {
        let transcript = transcript();
        let index = Index::new(&transcript);
        let template = built_in().remove(1);
        let windows = windows(&transcript, &[], &[], Some(16_000));
        let text: Vec<&str> = windows[0].iter().map(|l| l.text.as_str()).collect();
        assert_eq!(text[3], AFTER_THE_END);
        let item = check_item(
            raw(
                "open",
                "신발장은 고민 중",
                "",
                "",
                "00:00:16",
                "신발장이 줄어드는 건 조금 고민되네요",
            ),
            &template,
            &index,
            &transcript,
        )
        .unwrap();
        assert!(after_the_end(&item, &index, Some(16_000)));
        assert!(!after_the_end(&item, &index, Some(20_000)));
        assert!(!after_the_end(&item, &index, None));
    }

    #[test]
    fn a_fact_from_the_speakers_own_home_is_marked_but_the_customers_is_not() {
        let mut transcript = transcript();
        transcript
            .segments
            .push(line(21_000, 1, "우리 집은 고층이라 진동이 있어요."));
        transcript
            .segments
            .push(line(26_000, 2, "저희 집은 32평이고 3층이에요."));
        transcript.speakers[1].name = "고객".into();
        let index = Index::new(&transcript);
        let template = built_in().remove(1);
        let check = |text: &str, ts: &str, quote: &str| {
            let mut item = check_item(
                raw("site", text, "", "", ts, quote),
                &template,
                &index,
                &transcript,
            )
            .unwrap();
            mark_elsewhere(&mut item, &template, &index, &transcript, "");
            item.entry.reasons
        };
        assert_eq!(
            check(
                "현장은 고층이다",
                "00:00:21",
                "우리 집은 고층이라 진동이 있어요"
            ),
            [CheckReason::OtherSubject]
        );
        assert!(check(
            "현장은 32평 3층이다",
            "00:00:26",
            "저희 집은 32평이고 3층이에요"
        )
        .is_empty());

        // The customer left as "화자 2", or no speakers told apart at all:
        // who said "저희 집" is not known, so it is not doubted.
        for (id, name) in [(2, "화자 2"), (0, "화자")] {
            let mut unknown = transcript.clone();
            unknown.speakers.retain(|s| s.id != 2);
            if id == 2 {
                unknown.speakers.push(Speaker {
                    id: 2,
                    name: name.into(),
                });
            }
            for segment in &mut unknown.segments {
                segment.speaker = if id == 0 { 0 } else { segment.speaker };
            }
            let index = Index::new(&unknown);
            let mut item = check_item(
                raw(
                    "site",
                    "현장은 32평 3층이다",
                    "",
                    "",
                    "00:00:26",
                    "저희 집은 32평이고 3층이에요",
                ),
                &template,
                &index,
                &unknown,
            )
            .unwrap();
            mark_elsewhere(&mut item, &template, &index, &unknown, "이서진");
            assert!(item.entry.reasons.is_empty(), "{name}");
        }
    }

    #[test]
    fn numbers_are_compared_as_values_and_odd_amounts_are_marked() {
        let mut transcript = transcript();
        transcript
            .segments
            .push(line(21_000, 1, "천장은 아홉 평 정도 새로 합니다."));
        transcript.segments.push(line(
            26_000,
            1,
            "시공비까지 하면 80,900만원 정도 더 올라갈 수 있습니다.",
        ));
        transcript.segments.push(line(
            31_000,
            1,
            "복도 끝은 자당 20 기준이고 서재도 여기도 자당 25만 원입니다.",
        ));
        let index = Index::new(&transcript);
        let template = built_in().remove(1);
        let reasons = |text: &str, ts: &str, quote: &str| {
            check_item(
                raw("materials", text, "", "", ts, quote),
                &template,
                &index,
                &transcript,
            )
            .unwrap()
            .entry
            .reasons
        };
        // Said in words, written in digits: the same number.
        assert!(reasons(
            "천장 9평을 새로 한다",
            "00:00:21",
            "천장은 아홉 평 정도 새로 합니다"
        )
        .is_empty());
        assert_eq!(
            reasons(
                "시공비까지 80,900만원 더 든다",
                "00:00:26",
                "80,900만원 정도 더 올라갈 수"
            ),
            [CheckReason::UnitOdd]
        );
        assert_eq!(
            reasons(
                "복도 끝 수납장은 자당 20",
                "00:00:31",
                "복도 끝은 자당 20 기준이고"
            ),
            [CheckReason::NumbersDiffer]
        );
        let mut entries = vec![
            (
                "budget".to_string(),
                task(
                    "견적이 1억 5천에서 2억 1천으로 올랐다",
                    "",
                    "",
                    1_000,
                    vec![],
                ),
            ),
            (
                "budget".to_string(),
                task("5,100만 원 늘었다", "", "", 2_000, vec![]),
            ),
        ];
        mark_sums(&mut entries);
        assert_eq!(entries[0].1.reasons, [CheckReason::SumsDisagree]);
        assert_eq!(entries[1].1.reasons, [CheckReason::SumsDisagree]);
    }

    #[test]
    fn a_pair_the_checker_could_not_be_asked_about_is_left_as_written() {
        use crate::seogi::facts::Clash;
        let mut entries = vec![
            (
                "site".to_string(),
                task("현장은 32평", "", "", 3_000, vec![]),
            ),
            (
                "site".to_string(),
                task("현장은 58평", "", "", 7_000, vec![]),
            ),
        ];
        let gone = apply_reconciled(&mut entries, &[(0, 1, Clash::Numbers)], &BTreeMap::new());
        assert_eq!(gone, 0);
        assert!(entries.iter().all(|(_, e)| e.reasons.is_empty()));
    }

    #[test]
    fn a_failed_second_question_is_told_in_the_minutes() {
        let transcript = transcript();
        let index = Index::new(&transcript);
        let templates = built_in();
        let template = consult();
        // A provider that cannot be asked: no key in the keychain.
        let provider = Provider::Anthropic {
            model: "claude-test".into(),
            key: "seogi-test-key".into(),
        };
        let input = Input {
            transcript: &transcript,
            templates: &templates,
            template_id: Some("consult"),
            marks_ms: &[],
            mark_notes: &[],
            people: &[],
            customer: "",
            recorded_on: "",
            context: "",
            feedback_rules: "",
            provider: &provider,
            secrets: &|_| None,
        };
        let pair = || {
            vec![
                (
                    "decisions".to_string(),
                    task("방은 4개 그대로 유지하기로 했다", "", "", 1_000, vec![]),
                ),
                (
                    "open".to_string(),
                    task(
                        "방을 3개로 할지 4개로 유지할지 아직 정하지 않았다",
                        "",
                        "",
                        9_000,
                        vec![],
                    ),
                ),
            ]
        };
        let mut entries = pair();
        assert_eq!(
            reconcile(&input, &template, &mut entries, &index, &transcript),
            (0, true)
        );
        assert_eq!(entries, pair());
        // An answer with no results, or one that skips a pair, is no answer.
        let answered = |numbers: &[usize]| -> BTreeMap<usize, String> {
            numbers.iter().map(|&n| (n, "both".to_string())).collect()
        };
        assert!(unanswered(2, &answered(&[])));
        assert!(unanswered(2, &answered(&[1])));
        assert!(unanswered(2, &answered(&[1, 3])));
        assert!(!unanswered(2, &answered(&[1, 2])));
        // Nothing to ask about: nothing failed.
        let mut one = pair()[..1].to_vec();
        assert_eq!(
            reconcile(&input, &template, &mut one, &index, &transcript),
            (0, false)
        );
        // Minutes written before the mark existed read as checked.
        let mut old = serde_json::to_value(Minutes {
            template_id: "consult".into(),
            template_name: String::new(),
            title: String::new(),
            summary: String::new(),
            story: Vec::new(),
            feedback: None,
            sections: Vec::new(),
            verified: 0,
            to_check: 0,
            dropped: 0,
            not_rechecked: true,
            written_by: String::new(),
        })
        .unwrap();
        old.as_object_mut().unwrap().remove("not_rechecked");
        let read: Minutes = serde_json::from_value(old).unwrap();
        assert!(!read.not_rechecked);
    }
}
