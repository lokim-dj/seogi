//! The minutes as a Markdown document: what is saved beside the recording.
//!
//! The order is fixed by what a reader needs first, and what shows first
//! fits a page: the one-line summary, who does what, the story of how the
//! meeting went, and the few things to check by ear, each with its time in
//! the recording. The rest of the template's
//! sections follow folded under "자세히".

use super::minutes::{Entry, Minutes, MinutesSection};
use super::templates::{SectionKind, OPEN_SECTION};
use super::transcript::{clock, Transcript};

/// What the document says about the recording itself.
#[derive(Debug, Clone, Default)]
pub struct Header {
    pub title: String,
    /// When it was recorded, as shown: `2026-11-24 (화) 14:00`.
    pub recorded_at: String,
    pub duration_ms: u64,
    pub attendees: Vec<String>,
    pub place: String,
    /// The customer the meeting is about, when known.
    pub customer: String,
    /// How many whole minutes after its calendar event began the recording
    /// started, when that is enough to have missed something; 0 otherwise.
    pub late_minutes: u32,
    /// The recording seems to begin in the middle of talk.
    pub mid_talk: bool,
    /// The original file's name.
    pub source_file: String,
    pub sha256: String,
    /// The speech model that transcribed it.
    pub transcribed_by: String,
    pub app_version: String,
    /// Something the reader should know about how it was recorded:
    /// `app_stopped`, `microphone_lost` or `file_incomplete`. Empty when the
    /// recording went as it should.
    pub capture_note: String,
}

/// Seconds before and after a mark whose words are shown with it.
const MARK_BEFORE_MS: u64 = 20_000;
const MARK_AFTER_MS: u64 = 10_000;

pub fn minutes_markdown(
    minutes: &Minutes,
    transcript: &Transcript,
    header: &Header,
    marks_ms: &[u64],
) -> String {
    let mut out = String::new();
    let title = if header.title.is_empty() {
        &minutes.title
    } else {
        &header.title
    };
    out.push_str(&format!(
        "# {}\n\n",
        if title.is_empty() { "녹음" } else { title }
    ));

    let mut facts: Vec<String> = Vec::new();
    // Whom it is about, before anything else: the name the reader looks for.
    if !header.customer.is_empty() {
        facts.push(format!("- 고객: {}", header.customer));
    }
    if !header.recorded_at.is_empty() {
        facts.push(format!(
            "- 일시: {} · {}",
            header.recorded_at,
            duration(header.duration_ms)
        ));
    } else if header.duration_ms > 0 {
        facts.push(format!("- 길이: {}", duration(header.duration_ms)));
    }
    let attendees: Vec<String> = if header.attendees.is_empty() {
        transcript
            .speakers
            .iter()
            .map(|speaker| speaker.name.clone())
            .collect()
    } else {
        header.attendees.clone()
    };
    if !attendees.is_empty() {
        facts.push(format!("- 참석자: {}", attendees.join(", ")));
    }
    if !header.place.is_empty() {
        facts.push(format!("- 장소: {}", header.place));
    }
    facts.push(format!("- 종류: {}", minutes.template_name));
    // The minutes miss what came before the recording: the reader should
    // know before taking them as the whole meeting.
    if header.late_minutes > 0 {
        facts.push(format!(
            "- 녹음: 일정보다 {}분 늦게 시작됐어요. 그 앞의 이야기는 회의록에 없어요.",
            header.late_minutes
        ));
    } else if header.mid_talk {
        facts.push(
            "- 녹음: 대화 중간부터 시작된 것 같아요. 그 앞의 이야기는 회의록에 없어요.".to_string(),
        );
    }
    out.push_str(&facts.join("\n"));
    out.push_str("\n\n");

    if !minutes.summary.is_empty() {
        out.push_str(&format!("> {}\n\n", minutes.summary));
    }

    // 1. Who does what. A list the user took every task out of is left out.
    let tasks: Vec<&MinutesSection> = minutes
        .sections
        .iter()
        .filter(|section| section.kind == SectionKind::Actions && !section.entries.is_empty())
        .collect();
    if !tasks.is_empty() {
        // The people of the meeting, whose names a task line may leave out.
        let names: Vec<String> = header
            .attendees
            .iter()
            .cloned()
            .chain(
                transcript
                    .speakers
                    .iter()
                    .map(|speaker| speaker.name.clone()),
            )
            .collect();
        out.push_str("## 할 일\n\n");
        for section in &tasks {
            if tasks.len() > 1 || Side::of_title(&section.title).is_some() {
                out.push_str(&format!("### {}\n\n", section.title));
            }
            out.push_str(&task_lines(section, &names));
            out.push('\n');
        }
    }

    // 2. How it went.
    if !minutes.story.is_empty() {
        out.push_str("## 이야기 요약\n\n");
        for part in &minutes.story {
            if !part.title.is_empty() {
                out.push_str(&format!("### {}\n\n", part.title));
            }
            let times: Vec<String> = part
                .sources
                .iter()
                .map(|source| format!("`{}`", clock(source.ms)))
                .collect();
            out.push_str(&format!("{} {}\n\n", part.text, times.join(" ")));
        }
    }

    // 3. The few things to hear again.
    let flagged: Vec<(&MinutesSection, &Entry)> = minutes
        .sections
        .iter()
        .flat_map(|section| section.entries.iter().map(move |entry| (section, entry)))
        .filter(|(_, entry)| entry.needs_check())
        .collect();
    if !flagged.is_empty() {
        out.push_str("## 확인 필요\n\n");
        out.push_str("들어 보고 확인할 것 (시각을 눌러 그 대목만 들으면 됩니다):\n\n");
        for (section, entry) in &flagged {
            let reasons: Vec<&str> = entry.reasons.iter().map(|r| r.korean()).collect();
            out.push_str(&format!(
                "- [{}] {} — {} {}\n",
                section.title,
                entry.text,
                reasons.join(", "),
                times(entry)
            ));
        }
        out.push('\n');
    }

    // 4. Feedback on the conversation, for everyone to read together.
    if let Some(feedback) = &minutes.feedback {
        out.push_str(&feedback_markdown(feedback, transcript));
    }

    // 5. The rest, folded; what was left unsettled last.
    let rest: Vec<&MinutesSection> = minutes
        .sections
        .iter()
        .filter(|section| section.kind != SectionKind::Actions && section.key != OPEN_SECTION)
        .chain(
            minutes
                .sections
                .iter()
                .filter(|section| section.key == OPEN_SECTION),
        )
        .collect();
    let folded = !rest.is_empty() || !marks_ms.is_empty();
    if folded {
        out.push_str("<details>\n<summary>자세히</summary>\n\n");
    }
    for section in &rest {
        out.push_str(&section_markdown(section));
    }

    if !marks_ms.is_empty() {
        out.push_str("## 중요 표시\n\n");
        let mut marks = marks_ms.to_vec();
        marks.sort_unstable();
        for mark in marks {
            let said: Vec<String> = transcript
                .segments
                .iter()
                .filter(|segment| {
                    segment.end_ms >= mark.saturating_sub(MARK_BEFORE_MS)
                        && segment.start_ms <= mark + MARK_AFTER_MS
                })
                .map(|segment| {
                    format!(
                        "{}: {}",
                        transcript.speaker_name(segment.speaker),
                        segment.text
                    )
                })
                .collect();
            out.push_str(&format!("- `{}`", clock(mark)));
            if said.is_empty() {
                out.push_str(" (이 무렵에는 받아쓴 말이 없습니다)\n");
            } else {
                out.push('\n');
                for line in said {
                    out.push_str(&format!("  - {line}\n"));
                }
            }
        }
        out.push('\n');
    }

    if folded {
        out.push_str("</details>\n\n");
    }

    out.push_str("---\n\n");
    let mut quality = format!(
        "검증된 항목 {} · 확인 필요 {}",
        minutes.verified, minutes.to_check
    );
    if minutes.dropped > 0 {
        quality.push_str(&format!(" · 근거가 없어 뺀 항목 {}", minutes.dropped));
    }
    out.push_str(&quality);
    out.push_str("\n\n");
    if !transcript.corrections.is_empty() {
        let changed: Vec<String> = transcript
            .corrections
            .iter()
            .map(|c| {
                let first = c.at_ms.first().copied().unwrap_or(0);
                match c.at_ms.len() {
                    0 | 1 => format!("{} → {} `{}`", c.from, c.to, clock(first)),
                    places => format!(
                        "{} → {} `{}` 외 {}곳",
                        c.from,
                        c.to,
                        clock(first),
                        places - 1
                    ),
                }
            })
            .collect();
        out.push_str(&format!("용어집으로 고친 말: {}\n\n", changed.join(" · ")));
    }
    let mut origin: Vec<String> = Vec::new();
    if !header.source_file.is_empty() {
        origin.push(format!("원본 파일: {}", header.source_file));
    }
    if !header.sha256.is_empty() {
        origin.push(format!("SHA-256: `{}`", header.sha256));
    }
    if !header.recorded_at.is_empty() {
        origin.push(format!("녹음 시각: {}", header.recorded_at));
    }
    if !origin.is_empty() {
        out.push_str(&origin.join(" · "));
        out.push_str("\n\n");
    }
    let note = match header.capture_note.as_str() {
        "" => None,
        "app_stopped" => Some(
            "녹음하는 동안 앱이 꺼졌습니다. 그때까지 저장된 녹음으로 만든 회의록이며, 마지막 몇 초는 빠졌을 수 있습니다.",
        ),
        "microphone_lost" => Some(
            "녹음하는 동안 마이크 연결이 끊겼습니다. 그때까지의 녹음으로 만든 회의록입니다.",
        ),
        _ => Some("녹음 파일이 온전히 마무리되지 않아 되살린 녹음입니다."),
    };
    if let Some(note) = note {
        out.push_str(&format!("녹음 메모: {note}\n\n"));
    }
    let mut made: Vec<String> = Vec::new();
    if !header.app_version.is_empty() {
        made.push(format!("서기 {}", header.app_version));
    }
    if !header.transcribed_by.is_empty() {
        made.push(format!("받아쓰기: {}", header.transcribed_by));
    }
    if !minutes.written_by.is_empty() {
        made.push(format!("회의록: {}", minutes.written_by));
    }
    out.push_str(&made.join(" · "));
    out.push('\n');
    out
}

/// Speakers who said less than this share are counted together.
const SMALL_SHARE: f32 = 1.0;

/// The talking measured, and the points of feedback with their times.
fn feedback_markdown(feedback: &super::feedback::Feedback, transcript: &Transcript) -> String {
    let mut out = String::from("## 상담 피드백\n\n");
    let talk = super::talk::measure(transcript);
    if !talk.speakers.is_empty() {
        out.push_str("### 말하기\n\n");
        out.push_str(
            "기계가 녹음의 시각과 화자로 잰 숫자입니다. 화자 구분이 목소리로 검증되기 전이라 대략입니다.\n\n",
        );
        out.push_str("| 말한 사람 | 말한 시간 | 비율 | 말한 차례 | 가장 길게 혼자 말한 구간 |\n| --- | --- | --- | --- | --- |\n");
        let (big, small): (Vec<_>, Vec<_>) =
            talk.speakers.iter().partition(|s| s.share >= SMALL_SHARE);
        for speaker in &big {
            out.push_str(&format!(
                "| {} | {} | {:.0}% | {}번 | {} `{}` |\n",
                transcript.speaker_name(speaker.speaker),
                duration(speaker.talk_ms),
                speaker.share,
                speaker.turns,
                duration(speaker.longest_ms),
                clock(speaker.longest_at_ms)
            ));
        }
        if !small.is_empty() {
            let ms: u64 = small.iter().map(|s| s.talk_ms).sum();
            let share: f32 = small.iter().map(|s| s.share).sum();
            let turns: u32 = small.iter().map(|s| s.turns).sum();
            out.push_str(&format!(
                "| 기타 {}명 | {} | {:.0}% | {}번 | – |\n",
                small.len(),
                duration(ms),
                share,
                turns
            ));
        }
        out.push('\n');
        let count = |pick: &dyn Fn(&super::talk::SpeakerTalk) -> u32| -> String {
            big.iter()
                .map(|s| format!("{} {}번", transcript.speaker_name(s.speaker), pick(s)))
                .collect::<Vec<_>>()
                .join(", ")
        };
        out.push_str(&format!(
            "- 상대 말이 끝나기 전에 이어받은 횟수(맞장구는 빼고): {}\n",
            count(&|s| s.cut_ins)
        ));
        out.push_str(&format!("- 질문: {}\n", count(&|s| s.questions)));
        if !talk.moments.is_empty() {
            let moments: Vec<String> = talk
                .moments
                .iter()
                .map(|m| {
                    format!(
                        "`{}` {} → {}",
                        clock(m.at_ms),
                        transcript.speaker_name(m.from),
                        transcript.speaker_name(m.by)
                    )
                })
                .collect();
            out.push_str(&format!(
                "- 이어받은 순간 (들어 보고 판단해 주세요): {}\n",
                moments.join(" · ")
            ));
        }
        out.push('\n');
    }
    for (kind, title) in super::feedback::KINDS {
        let points = match kind {
            "good" => &feedback.good,
            "improve" => &feedback.improve,
            "missed_questions" => &feedback.missed_questions,
            "hesitations" => &feedback.hesitations,
            _ => &feedback.next_time,
        };
        if points.is_empty() {
            continue;
        }
        out.push_str(&format!("### {title}\n\n"));
        for point in points {
            let times: Vec<String> = point
                .sources
                .iter()
                .map(|source| format!("`{}`", clock(source.ms)))
                .collect();
            out.push_str(&format!("- {} {}\n", point.text, times.join(" ")));
        }
        out.push('\n');
    }
    out.push_str("AI가 회의록 항목만 보고 쓴 피드백입니다. 틀린 곳은 틀렸다고, 맞는 곳은 맞다고 함께 이야기해 주세요.\n\n");
    out
}

/// A section's tasks, one line each, the same everywhere: who – what – by
/// when. Who is left out when the section's title already says it; nobody
/// known is a small "담당 미정" after the task. `names` are the people of
/// the meeting.
fn task_lines(section: &MinutesSection, names: &[String]) -> String {
    let mut out = String::new();
    for entry in &section.entries {
        let line = task_line(&section.title, &entry.owner, &entry.text, names);
        out.push_str(&format!(
            "- {}{}{}{} – {} {}\n",
            line.who
                .as_ref()
                .map_or(String::new(), |who| format!("**{who}** – ")),
            one_line(&line.text),
            if line.unowned {
                " _(담당 미정)_"
            } else {
                ""
            },
            if entry.needs_check() {
                " _(확인 필요)_"
            } else {
                ""
            },
            if entry.due.trim().is_empty() {
                "미정"
            } else {
                entry.due.trim()
            },
            times(entry)
        ));
    }
    out
}

// How a task line reads under its group. The same rules are in
// src/seogi/lib/tasks.ts for the screen, and both are tested against
// src/seogi/lib/tasks.cases.json; keep the two alike.

/// Whose tasks a group holds, when its title says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Us,
    Customer,
    Them,
}

impl Side {
    const ALL: [Side; 3] = [Side::Us, Side::Customer, Side::Them];

    /// The words that name the side in a group's title or as an owner.
    fn words(self) -> &'static str {
        match self {
            Side::Us => "우리|저희",
            Side::Customer => "고객",
            Side::Them => r"상대\s*업체|상대방|상대|업체",
        }
    }

    /// The words that may be taken off as a sentence's subject. A bare
    /// "업체" is left: "업체는 두 곳으로 좁혀" speaks of vendors.
    fn subject_words(self) -> &'static str {
        match self {
            Side::Them => r"상대\s*업체|상대방|상대",
            side => side.words(),
        }
    }

    fn subject(self) -> String {
        format!(r"(?:{})(?:\s*님)?(?:\s*(?:쪽|측))?", self.subject_words())
    }

    /// The side a group's title names: "우리 숙제", "고객 숙제", "업체 할
    /// 일". "업체에 요청할 일" or "고객 응대 개선" is not a side's group.
    fn of_title(title: &str) -> Option<Side> {
        let title = title.trim();
        Side::ALL.into_iter().find(|side| {
            matches(
                &format!(
                    r"^(?:{})(?:\s*(?:쪽|측))?\s*(?:숙제|할\s*일)$",
                    side.words()
                ),
                title,
            )
        })
    }

    /// The side an owner is, when the owner is a side and not a person:
    /// "우리(상담 담당)", "고객님", "상대 업체" — but not "고객 김서윤".
    fn of_owner(owner: &str) -> Option<Side> {
        Side::ALL.into_iter().find(|side| {
            matches(
                &format!(
                    r"^(?:{})(?:\s*님)?(?:\s*(?:쪽|측))?\s*(?:\(.*\))?$",
                    side.words()
                ),
                owner,
            )
        })
    }
}

fn matches(pattern: &str, text: &str) -> bool {
    regex::Regex::new(pattern).is_ok_and(|re| re.is_match(text))
}

/// Titles that may follow a name: "김서윤 실장이".
const TITLES: &str = "실장|팀장|소장|대표|이사|부장|차장|과장|대리|주임|매니저|디자이너|선생";
/// The particles a subject ends with, longest first.
const PARTICLES: &str = "에서는|에서|께서는|께서|이|가|은|는";
/// What, right after the subject, says the subject is not the one who does
/// the task: "우리가 아니라 …", "저희는 빼고 …".
const NOT_THE_DOER: &str = "^(?:아니라|아닌|빼고|물론|말고)";
/// Endings of a word that describes the next one or sets a condition:
/// "고객이 원하는 타일", "업체가 정해지면". Whose word it is stays.
const DESCRIBES: &str = "(?:는|은|던|진|른|낸|든|쓴|한|된|뀐|면)$";
/// Words with such an ending that only tell the time or name a thing.
const DESCRIBES_NOT: &[&str] = &[
    "사진", "기한", "이번", "지난", "한번", "도면", "화면", "벽면", "측면", "전면", "정면", "평면",
    "단면", "표면",
];
/// Endings of a word that is the subject of a clause: "관리실이".
const ANOTHER_SUBJECT: &str = "(?:이|가|께서|은|는)$";
/// Words with such an ending that are not subjects.
const ANOTHER_SUBJECT_NOT: &[&str] = &[
    "같이",
    "깊이",
    "높이",
    "넓이",
    "길이",
    "많이",
    "깨끗이",
    "일찍이",
    "사이",
    "차이",
    "종이",
    "추가",
    "단가",
    "원가",
    "정가",
];

/// Whether an owner is a speaker label ("화자 2", "화자 A (디자이너)")
/// rather than someone's name.
fn is_speaker_label(owner: &str) -> bool {
    owner.trim().starts_with("화자")
}

/// Whether `owner` is one of the people of the meeting.
fn listed(owner: &str, names: &[String]) -> bool {
    names
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty() && !is_speaker_label(name))
        .any(|name| {
            name == owner
                || owner.starts_with(&format!("{name} "))
                || name.starts_with(&format!("{owner} "))
        })
}

/// A person's name as a subject. A name shorter than three letters must
/// come with a title or 님, so that an owner that is also a word ("유리",
/// "이사") is not taken off a sentence about the thing.
fn name_subject(owner: &str) -> String {
    let words: Vec<&str> = owner.split_whitespace().collect();
    let mut forms = vec![owner.to_string(), words.concat()];
    if words.len() > 1 && words[0].chars().count() >= 2 {
        forms.push(words[0].to_string());
    }
    forms.sort_by_key(|form| std::cmp::Reverse(form.chars().count()));
    forms.dedup();
    let title = format!(r"(?:\s*(?:{TITLES}))");
    let names: Vec<String> = forms
        .iter()
        .map(|form| {
            let name = regex::escape(form).replace(' ', r"\s*");
            if form.chars().count() >= 3 {
                format!(r"{name}{title}?(?:\s*님)?")
            } else {
                format!(r"{name}(?:{title}(?:\s*님)?|\s*님)")
            }
        })
        .collect();
    format!("(?:{})", names.join("|"))
}

/// Takes the subject `pattern` describes off the front of `text`, unless
/// that would change what the sentence says.
fn drop_subject(text: &str, pattern: &str) -> String {
    let Ok(subject) = regex::Regex::new(&format!(r"^(?:{pattern})(?:{PARTICLES})[\s,]+")) else {
        return text.to_string();
    };
    let Some(found) = subject.find(text) else {
        return text.to_string();
    };
    let rest = text[found.end()..].trim();
    if rest.is_empty() || rest.contains([',', '，']) {
        return text.to_string();
    }
    let bare = |word: &str| word.trim_end_matches(['.', '!', '?', '。']).to_string();
    let words: Vec<String> = rest.split_whitespace().map(bare).collect();
    let first = words[0].as_str();
    if matches(NOT_THE_DOER, first)
        || (matches(DESCRIBES, first) && !DESCRIBES_NOT.contains(&first))
    {
        return text.to_string();
    }
    if words.iter().any(|word| {
        word.chars().count() >= 2
            && matches(ANOTHER_SUBJECT, word)
            && !ANOTHER_SUBJECT_NOT.contains(&word.as_str())
    }) {
        return text.to_string();
    }
    rest.to_string()
}

/// One task line as written.
#[derive(Debug, PartialEq, Eq)]
struct TaskLine {
    /// The person or side to name beside the task; `None` when the group
    /// says the side, or nobody is known.
    who: Option<String>,
    text: String,
    /// Nobody is known: a small "담당 미정" stands in for a name.
    unowned: bool,
}

/// How a task reads under its group's `title`. A group such as "우리 숙제"
/// already says whose task it is, so its lines neither name that side again
/// nor start with it as the subject ("우리 쪽에서", "고객이"). The saved
/// minutes keep every word.
fn task_line(title: &str, owner: &str, text: &str, names: &[String]) -> TaskLine {
    let side = Side::of_title(title);
    let raw = owner.trim();
    let owner = if is_speaker_label(raw) { "" } else { raw };
    let owner_side = if owner.is_empty() {
        None
    } else {
        Side::of_owner(owner)
    };

    let mut text = text.trim().to_string();
    if let Some(side) = side {
        text = drop_subject(&text, &side.subject());
    }
    if owner.is_empty() {
        return TaskLine {
            who: None,
            text,
            unowned: true,
        };
    }
    if side.is_some() && owner_side == side {
        return TaskLine {
            who: None,
            text,
            unowned: false,
        };
    }
    if let Some(named) = owner_side {
        text = drop_subject(&text, &named.subject());
    } else if listed(owner, names) {
        text = drop_subject(&text, &name_subject(owner));
    }
    TaskLine {
        who: Some(owner.to_string()),
        text,
        unowned: false,
    }
}

fn section_markdown(section: &MinutesSection) -> String {
    let mut out = format!("## {}\n\n", section.title);
    match section.kind {
        SectionKind::Actions => out.push_str(&task_lines(section, &[])),
        SectionKind::Items => {
            for entry in &section.entries {
                out.push_str(&format!(
                    "- {}{} {}\n",
                    entry.text,
                    if entry.needs_check() {
                        " _(확인 필요)_"
                    } else {
                        ""
                    },
                    times(entry)
                ));
            }
        }
        SectionKind::Quotes => {
            for entry in &section.entries {
                // The words as transcribed lead; the model's sentence explains.
                let quote = entry
                    .sources
                    .first()
                    .map_or("", |source| source.quote.as_str());
                if quote.is_empty() {
                    out.push_str(&format!("- {} {}\n", entry.text, times(entry)));
                } else {
                    out.push_str(&format!(
                        "- \"{}\" — {} {}\n",
                        quote,
                        entry.text,
                        times(entry)
                    ));
                }
            }
        }
    }
    out.push('\n');
    out
}

/// An entry's source times: `` `00:12:34` `00:40:02` ``.
fn times(entry: &Entry) -> String {
    entry
        .sources
        .iter()
        .map(|source| format!("`{}`", clock(source.ms)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Text kept on one line of a list.
fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// `1시간 12분`, `37분`, `45초`.
pub fn duration(ms: u64) -> String {
    let secs = ms / 1000;
    match (secs / 3600, secs / 60 % 60) {
        (0, 0) => format!("{secs}초"),
        (0, minutes) => format!("{minutes}분"),
        (hours, 0) => format!("{hours}시간"),
        (hours, minutes) => format!("{hours}시간 {minutes}분"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seogi::minutes::{CheckReason, Source};
    use crate::seogi::transcript::{Segment, Speaker};

    fn entry(text: &str, owner: &str, due: &str, ms: u64, reasons: Vec<CheckReason>) -> Entry {
        Entry {
            text: text.into(),
            owner: owner.into(),
            due: due.into(),
            sources: vec![Source {
                ms,
                quote: "근거가 된 말".into(),
            }],
            reasons,
            done: false,
            faint: false,
            original: None,
            added: false,
        }
    }

    fn sample() -> (Minutes, Transcript) {
        let minutes = Minutes {
            template_id: "consult".into(),
            template_name: "고객 상담".into(),
            title: "한빛아파트 상담".into(),
            summary: "32평 상담에서 바닥과 중문 방향을 정했다.".into(),
            feedback: None,
            story: vec![crate::seogi::minutes::StoryPart {
                title: "중문".into(),
                text: "중문은 도면을 다시 그려 보내기로 했다.".into(),
                sources: vec![Source {
                    ms: 63_000,
                    quote: "도면".into(),
                }],
            }],
            sections: vec![
                MinutesSection {
                    key: "our_tasks".into(),
                    title: "우리 숙제".into(),
                    kind: SectionKind::Actions,
                    entries: vec![
                        entry(
                            "신발장 도면 다시 그려 보내기",
                            "김서윤 실장",
                            "금요일까지",
                            63_000,
                            vec![],
                        ),
                        entry(
                            "타일 | 견본 준비",
                            "",
                            "",
                            125_000,
                            vec![CheckReason::UnsureSpeech],
                        ),
                    ],
                    removed: Vec::new(),
                },
                MinutesSection {
                    key: "concerns".into(),
                    title: "고객의 우려".into(),
                    kind: SectionKind::Quotes,
                    entries: vec![entry(
                        "신발장이 줄어드는 것을 걱정함",
                        "",
                        "",
                        70_000,
                        vec![],
                    )],
                    removed: Vec::new(),
                },
                MinutesSection {
                    key: "open".into(),
                    title: "아직 안 정한 것".into(),
                    kind: SectionKind::Items,
                    entries: vec![entry(
                        "상부장을 없앨지는 견적을 보고 정하기로 함",
                        "",
                        "",
                        180_000,
                        vec![],
                    )],
                    removed: Vec::new(),
                },
            ],
            verified: 3,
            to_check: 1,
            dropped: 1,
            not_rechecked: false,
            written_by: "ollama gemma3:12b".into(),
        };
        let transcript = Transcript {
            language: "ko".into(),
            model: "whisper".into(),
            segments: vec![Segment {
                start_ms: 60_000,
                end_ms: 66_000,
                speaker: 1,
                text: "중문은 도면을 다시 그려서 보내드리겠습니다.".into(),
                confidence: None,
                unsure: false,
                overlap: false,
                edited: None,
                fixes: Vec::new(),
            }],
            speakers: vec![Speaker {
                id: 1,
                name: "김서윤 실장".into(),
            }],
            corrections: Vec::new(),
        };
        (minutes, transcript)
    }

    #[test]
    fn tasks_come_first_then_the_summary_and_the_rest_is_folded() {
        let (minutes, transcript) = sample();
        let header = Header {
            recorded_at: "2026-11-24 (화) 14:00".into(),
            duration_ms: 2_220_000,
            ..Default::default()
        };
        let markdown = minutes_markdown(&minutes, &transcript, &header, &[]);
        assert!(markdown.starts_with("# 한빛아파트 상담\n"));
        assert!(markdown.contains("- 일시: 2026-11-24 (화) 14:00 · 37분"));
        // Tasks, then the summary, then what to hear again: the first page.
        // The rest is folded beneath, what was left unsettled last.
        let line = markdown.find("> 32평 상담에서").unwrap();
        let tasks = markdown.find("## 할 일").unwrap();
        let summary = markdown.find("## 이야기 요약").unwrap();
        assert!(line < tasks);
        assert!(
            markdown.contains("### 중문\n\n중문은 도면을 다시 그려 보내기로 했다. `00:01:03`\n")
        );
        let check = markdown.find("## 확인 필요").unwrap();
        let folded = markdown.find("<summary>자세히</summary>").unwrap();
        let concerns = markdown.find("## 고객의 우려").unwrap();
        let open = markdown.find("## 아직 안 정한 것").unwrap();
        assert!(tasks < summary && summary < check && check < folded);
        assert!(folded < concerns && concerns < open);
        assert!(markdown.find("</details>").unwrap() > open);
        assert!(markdown.contains(
            "- **김서윤 실장** – 신발장 도면 다시 그려 보내기 – 금요일까지 `00:01:03`\n"
        ));
        // The section's title says whose task it is; no deadline is said in
        // plain words.
        assert!(markdown.contains("### 우리 숙제\n"));
        assert!(
            markdown.contains("- 타일 | 견본 준비 _(담당 미정)_ _(확인 필요)_ – 미정 `00:02:05`\n")
        );
    }

    /// The cases the screen is tested on too (src/seogi/lib/tasks.test.ts):
    /// the same input reads the same on the screen and in 회의록.md.
    #[test]
    fn task_lines_read_as_the_screen_draws_them() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../../../src/seogi/lib/tasks.cases.json"))
                .expect("the shared cases are JSON");
        let names: Vec<String> = cases["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap().to_string())
            .collect();
        for case in cases["titles"].as_array().unwrap() {
            let title = case["title"].as_str().unwrap();
            let side = Side::of_title(title).map(|side| match side {
                Side::Us => "us",
                Side::Customer => "customer",
                Side::Them => "them",
            });
            assert_eq!(side, case["side"].as_str(), "{title}");
        }
        let lines = cases["lines"].as_array().unwrap();
        assert!(lines.len() > 30);
        for case in lines {
            let text = |key: &str| case[key].as_str().unwrap();
            assert_eq!(
                task_line(text("title"), text("owner"), text("text"), &names),
                TaskLine {
                    who: case["who"].as_str().map(str::to_string),
                    text: text("out").to_string(),
                    unowned: case["unowned"].as_bool().unwrap(),
                },
                "{} / {} / {}",
                text("title"),
                text("owner"),
                text("text")
            );
        }
        for label in ["화자 2", "화자 A", "화자 2 (디자이너)", "화자대표"] {
            assert!(is_speaker_label(label), "{label}");
        }
    }

    #[test]
    fn a_task_line_in_the_document() {
        let section = MinutesSection {
            key: "our_tasks".into(),
            title: "우리 숙제".into(),
            kind: SectionKind::Actions,
            entries: vec![
                entry(
                    "우리 쪽에서 제안서를 보내기로 함.",
                    "우리(상담 담당)",
                    "",
                    0,
                    vec![],
                ),
                entry(
                    "김서윤 실장이 도면을 다시 그려 보내기",
                    "김서윤 실장",
                    "금요일까지",
                    0,
                    vec![],
                ),
                entry("견본 목록 정리하기", "", "", 0, vec![]),
            ],
            removed: Vec::new(),
        };
        let names = vec!["김서윤".to_string()];
        assert_eq!(
            task_lines(&section, &names),
            "- 제안서를 보내기로 함. – 미정 `00:00:00`\n\
             - **김서윤 실장** – 도면을 다시 그려 보내기 – 금요일까지 `00:00:00`\n\
             - 견본 목록 정리하기 _(담당 미정)_ – 미정 `00:00:00`\n"
        );
    }

    #[test]
    fn a_consultation_carries_its_talking_and_feedback() {
        let (mut minutes, transcript) = sample();
        assert!(
            !minutes_markdown(&minutes, &transcript, &Header::default(), &[])
                .contains("상담 피드백")
        );
        minutes.feedback = Some(crate::seogi::feedback::Feedback {
            good: vec![crate::seogi::feedback::Point {
                text: "도면 수정을 기한과 함께 약속했다.".into(),
                sources: vec![Source {
                    ms: 63_000,
                    quote: String::new(),
                }],
            }],
            ..Default::default()
        });
        let markdown = minutes_markdown(&minutes, &transcript, &Header::default(), &[]);
        let feedback = markdown.find("## 상담 피드백").unwrap();
        assert!(feedback < markdown.find("<summary>자세히</summary>").unwrap());
        assert!(markdown.contains("| 말한 사람 | 말한 시간 | 비율 | 말한 차례 |"));
        assert!(
            markdown.contains("### 잘한 점\n\n- 도면 수정을 기한과 함께 약속했다. `00:01:03`\n")
        );
        assert!(!markdown.contains("### 아쉬운 점"));
    }

    #[test]
    fn what_needs_a_look_is_listed_with_its_reason_and_time() {
        let (minutes, transcript) = sample();
        let markdown = minutes_markdown(&minutes, &transcript, &Header::default(), &[]);
        assert!(markdown.contains("- 상부장을 없앨지는 견적을 보고 정하기로 함 `00:03:00`"));
        assert!(markdown
            .contains("- [우리 숙제] 타일 | 견본 준비 — 받아쓰기가 불확실한 구간 `00:02:05`"));
        assert!(markdown.contains("- \"근거가 된 말\" — 신발장이 줄어드는 것을 걱정함 `00:01:10`"));
        assert!(markdown.contains("검증된 항목 3 · 확인 필요 1 · 근거가 없어 뺀 항목 1"));
        assert!(markdown.contains("회의록: ollama gemma3:12b"));
    }

    #[test]
    fn a_mark_shows_what_was_said_around_it() {
        let (minutes, transcript) = sample();
        let markdown = minutes_markdown(
            &minutes,
            &transcript,
            &Header::default(),
            &[70_000, 900_000],
        );
        assert!(markdown.contains(
            "- `00:01:10`\n  - 김서윤 실장: 중문은 도면을 다시 그려서 보내드리겠습니다."
        ));
        assert!(markdown.contains("- `00:15:00` (이 무렵에는 받아쓴 말이 없습니다)"));
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(duration(45_000), "45초");
        assert_eq!(duration(2_220_000), "37분");
        assert_eq!(duration(3_600_000), "1시간");
        assert_eq!(duration(4_320_000), "1시간 12분");
    }

    #[test]
    fn the_head_names_the_customer_and_says_the_recording_started_late() {
        let (minutes, transcript) = sample();
        let header = Header {
            customer: "이서진".into(),
            place: "한빛아파트 103동".into(),
            attendees: vec!["김서윤 실장".into()],
            late_minutes: 8,
            ..Default::default()
        };
        let markdown = minutes_markdown(&minutes, &transcript, &header, &[]);
        let head: Vec<&str> = markdown.lines().take(8).collect();
        assert_eq!(head[2], "- 고객: 이서진");
        assert!(head.contains(&"- 장소: 한빛아파트 103동"));
        assert!(head
            .contains(&"- 녹음: 일정보다 8분 늦게 시작됐어요. 그 앞의 이야기는 회의록에 없어요."));
        let mid = Header {
            mid_talk: true,
            ..Default::default()
        };
        assert!(minutes_markdown(&minutes, &transcript, &mid, &[]).contains("대화 중간부터"));
    }
}
