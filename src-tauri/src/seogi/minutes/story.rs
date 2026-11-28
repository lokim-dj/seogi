//! The one-line summary, the story of the meeting, and checking what the
//! story and the feedback tell against the lines they rest on.

use super::*;

/// The section of the site's facts (address, size, floor), which the checker
/// reads beside their lines like tasks and decisions.
pub(super) const SITE_SECTION: &str = "site";

/// Whether an entry in `section` states a commitment: a task, or something
/// decided. Notes on what was discussed do not.
pub(super) fn commits(template: &Template, section: &str) -> bool {
    section != OPEN_SECTION
        && template
            .section(section)
            .is_some_and(|s| s.kind == SectionKind::Actions || section == "decisions")
}

/// The items as the summary's writer reads them. All of them when they fit;
/// for a long meeting, the tasks and decisions first and then what room is
/// left, because more than a model reads is cut off without a word.
pub(super) fn summary_listing(template: &Template, items: &[(String, Entry)]) -> String {
    let line = |(key, entry): &(String, Entry)| {
        let section = template
            .section(key)
            .map_or(key.as_str(), |section| section.title.as_str());
        format!("- [{section}] {}\n", entry.text)
    };
    let whole: String = items.iter().map(line).collect();
    if whole.chars().count() <= SUMMARY_CHARS {
        return whole;
    }
    let first = items.iter().filter(|(key, _)| commits(template, key));
    let rest = items.iter().filter(|(key, _)| !commits(template, key));
    let mut listing = String::new();
    let mut used = 0;
    for item in first.chain(rest) {
        let line = line(item);
        let size = line.chars().count();
        if used + size > SUMMARY_CHARS {
            break;
        }
        used += size;
        listing.push_str(&line);
    }
    listing
}

/// One sentence on who talked about what and how it ended, written from the
/// checked items alone.
pub(super) fn summarize(
    input: &Input<'_>,
    template: &Template,
    items: &[(String, Entry)],
    title: &str,
) -> Result<String> {
    let listing = summary_listing(template, items);
    let system = "당신은 회의록 맨 위에 들어갈 한 줄 요약을 쓰는 서기입니다. 녹음에서 뽑아 확인을 마친 항목들이 주어집니다.\n\n\
         - 누구와 무엇을 이야기했고 결론이 무엇인지, 사람이 말하듯 자연스러운 한 문장으로 씁니다. 100자 안쪽.\n\
         - 항목을 나열하지 않습니다. '~함', '~음'으로 끝내지 않습니다.\n\
         - 주어진 항목에 있는 내용만 씁니다. 새로운 사실, 숫자, 이름을 보태지 않습니다.\n\
         - '~했다', '~하기로 했다'처럼 담백하게 끝냅니다.";
    let user = format!(
        "녹음 종류: {}\n제목: {}\n\n확인을 마친 항목:\n{listing}",
        template.name,
        if title.is_empty() { "(없음)" } else { title }
    );
    let schema = json!({
        "type": "object",
        "properties": { "summary": { "type": "string" } },
        "required": ["summary"],
        "additionalProperties": false
    });
    let answer = ask_again(input, system, &user, &schema)?;
    let summary: String = answer
        .get("summary")
        .and_then(Value::as_str)
        .map(|summary| summary.trim().chars().take(300).collect())
        .unwrap_or_default();
    // A number in the summary that no item has was made up.
    let known = digits_of(&listing);
    if numbers_in(&summary)
        .iter()
        .any(|n| !known.contains(n.as_str()))
    {
        return Err(anyhow!("the summary states a number the items do not have"));
    }
    Ok(summary)
}

/// The most of the items the story's writer reads, in characters.
pub(super) const STORY_CHARS: usize = 14_000;
/// The most parts a story has, and the most times shown with one.
pub(super) const STORY_PARTS: usize = 6;
pub(super) const STORY_TIMES: usize = 3;

/// The story of the meeting in a few parts, each a heading and a paragraph
/// written from the checked entries. Every sentence names the entries it
/// rests on; one that names none, or states a number they do not have, is
/// left out, and so is a part left with no sentence. The sentences are then
/// checked against the recording (`check_told`).
pub(super) fn tell(
    input: &Input<'_>,
    template: &Template,
    entries: &[(String, Entry)],
    title: &str,
) -> Result<Story> {
    if entries.is_empty() {
        return Ok(Story::default());
    }
    // In the order they were said; tasks and decisions are kept first when
    // not all fit.
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by_key(|&at| entries[at].1.sources.first().map_or(0, |s| s.ms));
    let mut chosen: Vec<usize> = Vec::new();
    let mut used = 0;
    for pass in [true, false] {
        for &at in &order {
            if commits(template, &entries[at].0) != pass {
                continue;
            }
            let size = entries[at].1.text.chars().count() + 24;
            if used + size > STORY_CHARS {
                continue;
            }
            used += size;
            chosen.push(at);
        }
    }
    chosen.sort_by_key(|&at| entries[at].1.sources.first().map_or(0, |s| s.ms));
    let listing: String = chosen
        .iter()
        .enumerate()
        .map(|(number, &at)| {
            let (key, entry) = &entries[at];
            let section = template
                .section(key)
                .map_or(key.as_str(), |s| s.title.as_str());
            let ms = entry.sources.first().map_or(0, |s| s.ms);
            format!(
                "{}. [{}] [{section}] {}\n",
                number + 1,
                clock(ms),
                entry.text
            )
        })
        .collect();

    let system = "당신은 회의록에 들어갈 '이야기 요약'을 쓰는 서기입니다. 녹음에서 뽑아 확인을 마친 항목들이 번호, 말한 시각, 갈래와 함께 주어집니다.\n\n\
         - 회의가 어떻게 시작했고, 무엇을 이야기했고, 어디서 의견이 갈렸고, 어떻게 정리됐는지가 흐르듯 읽히게 씁니다. 항목을 나열하지 않습니다.\n\
         - 짧은 소제목을 붙인 부분 3~6개로 나눕니다. 부분마다 2~4문장입니다.\n\
         - 문장은 '~했다', '~하기로 했다'처럼 평서문으로 끝냅니다. '~함', '~음', '~됨'으로 끝내지 않습니다.\n\
         - 숫자, 금액, 날짜, 이름은 항목에 적힌 그대로 살립니다.\n\
         - 문장마다 그 문장의 근거가 된 항목 번호(items)를 적습니다. 항목에 없는 내용은 쓰지 않습니다.";
    let user = format!(
        "녹음 종류: {}\n제목: {}\n\n확인을 마친 항목:\n{listing}",
        template.name,
        if title.is_empty() { "(없음)" } else { title }
    );
    let schema = json!({
        "type": "object",
        "properties": {
            "parts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "sentences": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "text": { "type": "string" },
                                    "items": { "type": "array", "items": { "type": "integer" } }
                                },
                                "required": ["text", "items"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["title", "sentences"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["parts"],
        "additionalProperties": false
    });
    let answer = ask_again(input, system, &user, &schema)?;
    let cited: Vec<&Entry> = chosen.iter().map(|&at| &entries[at].1).collect();
    Ok(Story {
        parts: draft_story(&answer, &cited),
        cited: cited.into_iter().cloned().collect(),
    })
}

/// A sentence of the story and the entries it rests on (positions in
/// `Story::cited`).
#[derive(Debug, Clone)]
pub(super) struct Sentence {
    text: String,
    backing: Vec<usize>,
}

/// A part of the story before its sentences are checked.
#[derive(Debug, Clone)]
pub(super) struct Draft {
    title: String,
    sentences: Vec<Sentence>,
}

/// The story as written, before its sentences are checked against the
/// recording, with the entries its sentences rest on.
#[derive(Debug, Clone, Default)]
pub(super) struct Story {
    parts: Vec<Draft>,
    cited: Vec<Entry>,
}

impl Story {
    /// The parts as the minutes keep them: those with a sentence left, with
    /// the times their sentences rest on.
    pub(super) fn finish(self) -> Vec<StoryPart> {
        let cited: Vec<&Entry> = self.cited.iter().collect();
        finish_story(self.parts, &cited)
    }
}

/// The parts of a story answer that rest on `cited` (the entries by their
/// number, from 1).
#[cfg(test)]
pub(super) fn read_story(answer: &Value, cited: &[&Entry]) -> Vec<StoryPart> {
    finish_story(draft_story(answer, cited), cited)
}

/// The sentences of a story answer that rest on `cited` (the entries by
/// their number, from 1): one that names none, or states a number they and
/// their quotes do not have, is left out.
pub(super) fn draft_story(answer: &Value, cited: &[&Entry]) -> Vec<Draft> {
    let mut parts: Vec<Draft> = Vec::new();
    for part in answer
        .get("parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let title = part
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let mut sentences: Vec<Sentence> = Vec::new();
        for sentence in part
            .get("sentences")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let text = sentence
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            let mut backing: Vec<usize> = sentence
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .filter_map(|n| (n as usize).checked_sub(1))
                .filter(|&at| at < cited.len())
                .collect();
            backing.dedup();
            if text.is_empty() || backing.is_empty() {
                continue;
            }
            // A number its entries and their quotes do not have was made up.
            let known: String = backing
                .iter()
                .map(|&at| {
                    let entry = cited[at];
                    let quotes: Vec<&str> =
                        entry.sources.iter().map(|s| s.quote.as_str()).collect();
                    format!("{} {}", entry.text, quotes.join(" "))
                })
                .map(|text| digits_of(&text))
                .collect::<Vec<_>>()
                .join(" ");
            if numbers_in(&text)
                .iter()
                .any(|n| !known.contains(n.as_str()))
            {
                continue;
            }
            sentences.push(Sentence { text, backing });
        }
        if sentences.is_empty() {
            continue;
        }
        parts.push(Draft { title, sentences });
        if parts.len() >= STORY_PARTS {
            break;
        }
    }
    parts
}

/// The parts that still have a sentence, each with the first, the middle and
/// the last place its sentences rest on.
pub(super) fn finish_story(parts: Vec<Draft>, cited: &[&Entry]) -> Vec<StoryPart> {
    let mut out: Vec<StoryPart> = Vec::new();
    for part in parts {
        if part.sentences.is_empty() {
            continue;
        }
        let mut sources: Vec<Source> = Vec::new();
        for &at in part.sentences.iter().flat_map(|s| &s.backing) {
            if let Some(source) = cited.get(at).and_then(|entry| entry.sources.first()) {
                if !sources.iter().any(|s| s.ms == source.ms) {
                    sources.push(source.clone());
                }
            }
        }
        sources.sort_by_key(|source| source.ms);
        if sources.len() > STORY_TIMES {
            let last = sources.len() - 1;
            sources = vec![
                sources[0].clone(),
                sources[last / 2].clone(),
                sources[last].clone(),
            ];
        }
        let text: Vec<String> = part.sentences.into_iter().map(|s| s.text).collect();
        out.push(StoryPart {
            title: part.title,
            text: text.join(" "),
            sources,
        });
    }
    out
}

/// What the check of the story and the feedback did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ToldChecked {
    /// Claims the checker answered for.
    pub(super) read: usize,
    pub(super) softened: usize,
    pub(super) gone: usize,
}

/// Where a claim put to the checker is: a story sentence (part, sentence)
/// or a feedback point (kind, point).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToldPlace {
    Story(usize, usize),
    Point(usize, usize),
}

/// Every sentence of the story and every point of the feedback as a claim,
/// with the entries it rests on and the transcript lines around the places
/// they were said, and where each is. A feedback point keeps only the times
/// of its entries: they are found again among `told` by their first time.
pub(super) fn told_claims(
    transcript: &Transcript,
    story: &Story,
    feedback: Option<&super::feedback::Feedback>,
    told: &[(String, Entry)],
) -> (Vec<super::claims::Claim>, Vec<ToldPlace>) {
    use super::claims::{lines_around, Claim};
    let mut claims: Vec<Claim> = Vec::new();
    let mut places: Vec<ToldPlace> = Vec::new();
    for (p, part) in story.parts.iter().enumerate() {
        for (s, sentence) in part.sentences.iter().enumerate() {
            let mut times: Vec<u64> = Vec::new();
            for &at in &sentence.backing {
                for source in &story.cited[at].sources {
                    if !times.contains(&source.ms) {
                        times.push(source.ms);
                    }
                }
            }
            claims.push(Claim {
                text: sentence.text.clone(),
                items: sentence
                    .backing
                    .iter()
                    .map(|&at| story.cited[at].text.clone())
                    .collect(),
                lines: lines_around(transcript, &times),
            });
            places.push(ToldPlace::Story(p, s));
        }
    }
    if let Some(feedback) = feedback {
        for (k, points) in feedback_kinds(feedback).iter().enumerate() {
            for (n, point) in points.iter().enumerate() {
                let times: Vec<u64> = point.sources.iter().map(|s| s.ms).collect();
                let items: Vec<String> = told
                    .iter()
                    .filter(|(_, entry)| {
                        entry
                            .sources
                            .first()
                            .is_some_and(|source| times.contains(&source.ms))
                    })
                    .map(|(_, entry)| entry.text.clone())
                    .collect();
                claims.push(Claim {
                    text: point.text.clone(),
                    items,
                    lines: lines_around(transcript, &times),
                });
                places.push(ToldPlace::Point(k, n));
            }
        }
    }
    (claims, places)
}

/// Puts every sentence of the story and every point of the feedback to the
/// checker beside the transcript lines it rests on (see `claims`), and
/// applies what it said (`apply_told`). A checker that cannot be asked
/// leaves the rest as written.
pub(super) fn check_told(
    input: &Input<'_>,
    transcript: &Transcript,
    story: &mut Story,
    feedback: Option<&mut super::feedback::Feedback>,
    entries: &mut [(String, Entry)],
    told: &[(String, Entry)],
    cancelled: &AtomicBool,
) -> ToldChecked {
    use super::claims::{self, Judgement};

    let (claims_list, places) = told_claims(transcript, story, feedback.as_deref(), told);
    if claims_list.is_empty() {
        return ToldChecked::default();
    }
    let batches = claims::batches(&claims_list);
    let schema = claims::schema();
    let unreachable = AtomicBool::new(false);
    let (answers, _) = in_rounds(
        &batches,
        input.provider.at_once(),
        &|_, batch: &Vec<usize>| {
            if unreachable.load(Ordering::Acquire) {
                return None;
            }
            let user = claims::request(&claims_list, batch);
            match ask_again(input, claims::SYSTEM, &user, &schema) {
                Ok(answer) => Some(claims::read(&answer, &claims_list, batch)),
                Err(e) => {
                    warn!(
                        "Could not check {} story and feedback lines ({e:#}); leaving them as written",
                        batch.len()
                    );
                    if !format!("{e:#}").contains("JSON") {
                        unreachable.store(true, Ordering::Release);
                    }
                    None
                }
            }
        },
        &|_| !cancelled.load(Ordering::Acquire),
    );
    let mut judged: BTreeMap<usize, Judgement> = BTreeMap::new();
    for answer in answers.into_iter().flatten() {
        judged.extend(answer);
    }
    apply_told(story, feedback, entries, &places, &judged, told)
}

/// Applies the checker's judgements (by position in `places`): what the
/// lines say stays; what they say less strongly is softened; what they do
/// not say is left out. When the words that went too far are already in an
/// entry a sentence or point rests on, that entry was written too strongly
/// as well and is marked for a look. `told` is what the feedback was
/// written from; an entry is found again by any time it was said at and the
/// words, since tasks may have been tidied since.
pub(super) fn apply_told(
    story: &mut Story,
    mut feedback: Option<&mut super::feedback::Feedback>,
    entries: &mut [(String, Entry)],
    places: &[ToldPlace],
    judged: &BTreeMap<usize, super::claims::Judgement>,
    told: &[(String, Entry)],
) -> ToldChecked {
    use super::claims::{self, Judgement};

    let mut done = ToldChecked {
        read: judged.len(),
        ..ToldChecked::default()
    };
    // Where entries that said too much themselves were said, and the words.
    let mut too_strong: Vec<(u64, String)> = Vec::new();
    let mut gone_sentences: Vec<(usize, usize)> = Vec::new();
    let mut gone_points: Vec<(usize, usize)> = Vec::new();
    for (&at, judgement) in judged {
        let (Some(&place), false) = (places.get(at), *judgement == Judgement::Keep) else {
            continue;
        };
        let softer = match judgement {
            Judgement::Soften { text, .. } => Some(text.clone()),
            _ => None,
        };
        match place {
            ToldPlace::Story(p, s) => {
                let sentence = &mut story.parts[p].sentences[s];
                for &backing in &sentence.backing {
                    let entry = &story.cited[backing];
                    if claims::says(&entry.text, judgement.beyond()) {
                        for source in &entry.sources {
                            too_strong.push((source.ms, judgement.beyond().to_string()));
                        }
                    }
                }
                match softer {
                    Some(text) => sentence.text = text,
                    None => gone_sentences.push((p, s)),
                }
            }
            ToldPlace::Point(k, n) => {
                let Some(feedback) = feedback.as_deref_mut() else {
                    continue;
                };
                let times: Vec<u64> = feedback_kinds(feedback)[k][n]
                    .sources
                    .iter()
                    .map(|s| s.ms)
                    .collect();
                for (_, entry) in told {
                    let said_then = entry
                        .sources
                        .first()
                        .is_some_and(|source| times.contains(&source.ms));
                    if said_then && claims::says(&entry.text, judgement.beyond()) {
                        for source in &entry.sources {
                            too_strong.push((source.ms, judgement.beyond().to_string()));
                        }
                    }
                }
                match softer {
                    Some(text) => feedback_kinds_mut(feedback)[k][n].text = text,
                    None => gone_points.push((k, n)),
                }
            }
        }
        match judgement {
            Judgement::Soften { .. } => done.softened += 1,
            _ => done.gone += 1,
        }
    }
    for &(p, s) in gone_sentences.iter().rev() {
        story.parts[p].sentences.remove(s);
    }
    if let Some(feedback) = feedback {
        let kinds = feedback_kinds_mut(feedback);
        for &(k, n) in gone_points.iter().rev() {
            kinds[k].remove(n);
        }
    }
    for (_, entry) in entries.iter_mut() {
        let marked = too_strong.iter().any(|(ms, beyond)| {
            entry.sources.iter().any(|source| source.ms == *ms) && claims::says(&entry.text, beyond)
        });
        if marked {
            push_reason(entry, CheckReason::SaidDifferently);
        }
    }
    done
}

/// The kinds of feedback points, in the order of `feedback::KINDS`.
pub(super) fn feedback_kinds(
    feedback: &super::feedback::Feedback,
) -> [&Vec<super::feedback::Point>; 5] {
    [
        &feedback.good,
        &feedback.improve,
        &feedback.missed_questions,
        &feedback.hesitations,
        &feedback.next_time,
    ]
}

pub(super) fn feedback_kinds_mut(
    feedback: &mut super::feedback::Feedback,
) -> [&mut Vec<super::feedback::Point>; 5] {
    [
        &mut feedback.good,
        &mut feedback.improve,
        &mut feedback.missed_questions,
        &mut feedback.hesitations,
        &mut feedback.next_time,
    ]
}

#[cfg(test)]
mod tests {
    use super::super::test_help::*;
    use super::*;
    use crate::seogi::templates::built_in;

    #[test]
    fn a_story_keeps_only_sentences_that_rest_on_items() {
        let budget = task("예산은 1억 원을 넘기지 않기로 함", "", "", 60_000, vec![]);
        let kitchen = task("주방을 확장부로 옮기기로 함", "", "", 120_000, vec![]);
        let cited = vec![&budget, &kitchen];
        let answer = json!({ "parts": [
            { "title": "예산", "sentences": [
                { "text": "고객은 예산을 1억 원 안으로 잡았다.", "items": [1] },
                // A number no item has.
                { "text": "업체는 9천만 원을 제안했다.", "items": [1] },
                // Rests on nothing.
                { "text": "분위기는 좋았다.", "items": [] },
                { "text": "없는 항목에 기댄 문장이다.", "items": [7] }
            ]},
            { "title": "빈 부분", "sentences": [{ "text": "근거 없음", "items": [] }] },
            { "title": "주방", "sentences": [
                { "text": "주방은 확장부로 옮기기로 했다.", "items": [2, 1] }
            ]}
        ]});
        let story = read_story(&answer, &cited);
        assert_eq!(story.len(), 2);
        assert_eq!(story[0].title, "예산");
        assert_eq!(story[0].text, "고객은 예산을 1억 원 안으로 잡았다.");
        assert_eq!(
            story[0].sources.iter().map(|s| s.ms).collect::<Vec<_>>(),
            [60_000]
        );
        assert_eq!(
            story[1].sources.iter().map(|s| s.ms).collect::<Vec<_>>(),
            [60_000, 120_000]
        );
    }

    #[test]
    fn the_summary_of_a_long_meeting_reads_its_tasks_and_decisions_first() {
        let template = built_in().remove(0);
        let actions = template
            .sections
            .iter()
            .find(|section| section.kind == SectionKind::Actions)
            .map(|section| section.key.clone())
            .unwrap();
        let note = template
            .sections
            .iter()
            .find(|section| !commits(&template, &section.key) && section.key != OPEN_SECTION)
            .map(|section| section.key.clone())
            .unwrap();

        // A short meeting is listed whole, in the order it was said.
        let short = vec![
            item(&note, "창호 견적을 두 곳에서 받아 보았다", 1, 1_000),
            item(&actions, "도면을 금요일까지 보낸다", 2, 2_000),
        ];
        let pairs = |items: Vec<Item>| -> Vec<(String, Entry)> {
            items.into_iter().map(|i| (i.section, i.entry)).collect()
        };
        let listing = summary_listing(&template, &pairs(short));
        assert!(listing.find("창호 견적").unwrap() < listing.find("도면을").unwrap());

        // A long one is cut to what a model reads, the tasks kept.
        let mut long: Vec<Item> = (0..400u64)
            .map(|n| {
                item(
                    &note,
                    &format!("{n}번째로 나눈 이야기는 자재와 일정에 관한 것이었다"),
                    n as usize,
                    n * 1_000,
                )
            })
            .collect();
        long.push(item(&actions, "도면을 금요일까지 보낸다", 400, 400_000));
        let listing = summary_listing(&template, &pairs(long));
        assert!(listing.chars().count() <= SUMMARY_CHARS);
        assert!(listing.starts_with("- ["));
        assert!(listing
            .lines()
            .next()
            .unwrap()
            .contains("도면을 금요일까지 보낸다"));
    }

    #[test]
    fn a_story_sentence_or_feedback_point_the_lines_do_not_say_is_not_told() {
        use crate::seogi::claims::Judgement;
        use crate::seogi::feedback::{Feedback, Point};
        let transcript = meeting_of(&[
            (1_000, 2, "주방을 거실 쪽으로 옮기고 싶어요."),
            (6_000, 1, "주방 옮기는 건 괜찮습니다."),
            (11_000, 2, "창가 끝까지는요?"),
            (16_000, 1, "그건 조금 어렵습니다."),
            (21_000, 2, "공사가 커질까 걱정이에요."),
            (26_000, 1, "거실 쪽으로만 옮기면 크게 늘지 않습니다."),
        ]);
        let too_strong = task("주방을 옮기는 안은 무리라고 판단함", "", "", 6_000, vec![]);
        let worry = task("공사가 커질까 걱정함", "", "", 21_000, vec![]);
        let mut story = Story {
            parts: vec![
                Draft {
                    title: "주방".into(),
                    sentences: vec![
                        Sentence {
                            text: "주방을 옮기는 안은 무리라고 판단했다.".into(),
                            backing: vec![0],
                        },
                        Sentence {
                            text: "고객은 공사가 커질까 걱정했다.".into(),
                            backing: vec![1],
                        },
                    ],
                },
                Draft {
                    title: "남은 것".into(),
                    sentences: vec![Sentence {
                        text: "창가 끝까지 옮기는 것은 불가능하다고 했다.".into(),
                        backing: vec![0],
                    }],
                },
            ],
            cited: vec![too_strong.clone(), worry.clone()],
        };
        let point = |text: &str, ms: u64| Point {
            text: text.into(),
            sources: vec![Source {
                ms,
                quote: String::new(),
            }],
        };
        let mut feedback = Feedback {
            hesitations: vec![point("공사 걱정에 답한 기록이 없다.", 21_000)],
            good: vec![point("주방 이전 가능 여부에 바로 답했다.", 6_000)],
            ..Feedback::default()
        };
        let told = vec![
            ("materials".to_string(), too_strong.clone()),
            ("concerns".to_string(), worry.clone()),
        ];
        let (claims, places) = told_claims(&transcript, &story, Some(&feedback), &told);
        assert_eq!(claims[0].items, ["주방을 옮기는 안은 무리라고 판단함"]);
        assert_eq!(claims[4].items, ["공사가 커질까 걱정함"]);
        assert_eq!(claims.len(), 5);
        assert_eq!(places[3], ToldPlace::Point(0, 0));
        assert_eq!(places[4], ToldPlace::Point(3, 0));
        // The lines around the worry hold the answer.
        assert!(claims[4]
            .lines
            .contains("거실 쪽으로만 옮기면 크게 늘지 않습니다."));

        let mut entries = vec![
            ("materials".to_string(), too_strong),
            ("concerns".to_string(), worry),
        ];
        let judged: BTreeMap<usize, Judgement> = [
            (
                0,
                Judgement::Drop {
                    beyond: "무리".into(),
                },
            ),
            (1, Judgement::Keep),
            (
                2,
                Judgement::Soften {
                    text: "창가 끝까지 옮기는 것은 조금 어렵다고 했다.".into(),
                    beyond: "불가능".into(),
                },
            ),
            (
                4,
                Judgement::Drop {
                    beyond: "답한 기록이 없다".into(),
                },
            ),
        ]
        .into_iter()
        .collect();
        let done = apply_told(
            &mut story,
            Some(&mut feedback),
            &mut entries,
            &places,
            &judged,
            &told,
        );
        assert_eq!(
            done,
            ToldChecked {
                read: 4,
                softened: 1,
                gone: 2
            }
        );
        assert!(feedback.hesitations.is_empty());
        assert_eq!(feedback.good.len(), 1);
        // The entry that said "무리" itself goes to a look; the other not.
        assert_eq!(entries[0].1.reasons, [CheckReason::SaidDifferently]);
        assert!(entries[1].1.reasons.is_empty());
        let parts = story.finish();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].text, "고객은 공사가 커질까 걱정했다.");
        assert_eq!(
            parts[0].sources.iter().map(|s| s.ms).collect::<Vec<_>>(),
            [21_000]
        );
        assert_eq!(parts[1].text, "창가 끝까지 옮기는 것은 조금 어렵다고 했다.");

        // Nothing judged (the checker could not be asked): all stays.
        let mut kept = Story {
            parts: vec![Draft {
                title: "t".into(),
                sentences: vec![Sentence {
                    text: "그대로다.".into(),
                    backing: vec![0],
                }],
            }],
            cited: vec![task("그대로", "", "", 1_000, vec![])],
        };
        let none = apply_told(
            &mut kept,
            None,
            &mut [],
            &[ToldPlace::Story(0, 0)],
            &BTreeMap::new(),
            &[],
        );
        assert_eq!(none, ToldChecked::default());
        assert_eq!(kept.finish()[0].text, "그대로다.");

        // A feedback point that went too far marks the entry it rests on,
        // found again though tidying the tasks changed its words and its
        // first time.
        let said = task("주방 이전은 무리라고 판단함", "", "", 6_000, vec![]);
        let mut tidied = task(
            "주방 이전은 무리라고 보고 도면을 다시 그리기",
            "",
            "",
            9_000,
            vec![],
        );
        tidied.sources.push(Source {
            ms: 6_000,
            quote: String::new(),
        });
        let mut entries = vec![("our_tasks".to_string(), tidied)];
        let mut feedback = Feedback {
            improve: vec![point("주방 이전을 무리라고 단정했다.", 6_000)],
            ..Feedback::default()
        };
        let judged: BTreeMap<usize, Judgement> = [(
            0,
            Judgement::Drop {
                beyond: "무리".into(),
            },
        )]
        .into_iter()
        .collect();
        apply_told(
            &mut Story::default(),
            Some(&mut feedback),
            &mut entries,
            &[ToldPlace::Point(1, 0)],
            &judged,
            &[("materials".to_string(), said)],
        );
        assert!(feedback.improve.is_empty());
        assert_eq!(entries[0].1.reasons, [CheckReason::SaidDifferently]);
    }
}
