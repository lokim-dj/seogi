//! Meetings on disk.
//!
//! Every meeting is a folder in the library, named `2026-11-24 1400 제목`.
//! It holds the original recording (never changed), `회의록.md`, `전사.txt`,
//! `원본정보.json`, and `meeting.json`, which is the app's own record of the
//! meeting: its state, transcript and minutes. The folders are the truth;
//! the app keeps nothing about a meeting anywhere else, so a library can be
//! moved, synced or backed up as plain files.
//!
//! What the user changes is kept in `meeting.json` too: a task ticked off,
//! a speaker's name, a task changed, taken out or added (`Entry::original`,
//! `Entry::added`, `MinutesSection::removed`), a transcript line fixed
//! (`Segment::edited`), a misheard word fixed (`Segment::fixes`, also how
//! the vocabulary's own corrections are kept). New fields are only ever added, each with
//! `#[serde(default)]`, so a record saved by an older version still reads;
//! nothing denies fields it does not know, so an older version reads a
//! newer record too (and drops what it does not know if it saves it). The
//! words as transcribed stay in `Segment::text` and in `전사.txt`; the
//! minutes, `회의록.md` and searching read the user's words.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Datelike, Local, TimeZone};
use log::warn;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::minutes::{Entry, Minutes, MinutesSection};
use super::pipeline::Stats;
use super::render::{self, Header};
use super::templates::{SectionKind, OPEN_SECTION};
use super::transcript::Transcript;

/// The app's record of a meeting, inside its folder.
pub const RECORD_NAME: &str = "meeting.json";
pub const MINUTES_NAME: &str = "회의록.md";
pub const TRANSCRIPT_NAME: &str = "전사.txt";
/// The file an in-app recording is written to.
pub const RECORDING_NAME: &str = "녹음.wav";

/// How a recording came to the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Recorded in the app.
    Recorded,
    /// Brought in by hand.
    Imported,
    /// Picked up from a watched folder.
    Watched,
    /// Recorded on the phone and brought in from the user's Drive.
    Phone,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Recorded => "recorded",
            Origin::Imported => "imported",
            Origin::Watched => "watched",
            Origin::Phone => "phone",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Being recorded in the app right now.
    Recording,
    /// Waiting its turn to be processed.
    Queued,
    /// Being transcribed or written up; `stage` and `percent` say how far.
    Working,
    /// Transcribed, and the minutes are still to come: the summary model
    /// could not be reached and will be tried again.
    Transcribed,
    /// The minutes are written.
    Done,
    /// It could not be processed; `error` says why.
    Failed,
}

/// The calendar event a meeting was matched to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EventRef {
    pub uid: String,
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
}

/// A stretch of a meeting the microphone did not hear: it went away
/// `at_ms` into the recording and came back `gap_ms` later. Nothing was
/// written for it, so the recording goes straight on from that moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Gap {
    pub at_ms: u64,
    pub gap_ms: u64,
}

impl Gap {
    /// The note the transcript and the minutes put at the gap: `(12초 끊김)`,
    /// or `(끊김)` for one whose length is not known (the app stopped while
    /// the microphone was away; `gap_ms` is 0 then).
    pub fn note(&self) -> String {
        if self.gap_ms == 0 {
            "(끊김)".to_string()
        } else {
            format!("({} 끊김)", self.words())
        }
    }

    /// How long, as the transcript and the minutes say it: `12초`,
    /// `3분 5초`, `1시간 2분`.
    pub fn words(&self) -> String {
        let seconds = self.gap_ms.div_ceil(1_000).max(1);
        let (h, m, s) = (seconds / 3_600, seconds / 60 % 60, seconds % 60);
        let mut parts: Vec<String> = Vec::new();
        if h > 0 {
            parts.push(format!("{h}시간"));
        }
        if m > 0 {
            parts.push(format!("{m}분"));
        }
        if s > 0 && h == 0 {
            parts.push(format!("{s}초"));
        }
        parts.join(" ")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    /// Whether nobody chose the title: the minutes may then replace it with
    /// one taken from what was said.
    #[serde(default)]
    pub title_is_auto: bool,
    /// When the recording started, in milliseconds since 1970 UTC.
    pub started_at_ms: i64,
    /// Where `started_at_ms` comes from: `recorder` (recorded here),
    /// `file_name` (a time in the imported file's name), `file_metadata`
    /// (a time the recorder wrote inside the file), `file_modified`
    /// (the file's modification time, not yet corrected) or
    /// `file_modified_minus_length` (that time less the recording's length).
    #[serde(default)]
    pub time_source: String,
    pub duration_ms: u64,
    pub origin: Origin,
    /// The original recording's file name, in the meeting's folder.
    pub audio_file: String,
    /// Where an imported or watched file came from.
    #[serde(default)]
    pub source_path: String,
    #[serde(default)]
    pub sha256: String,
    pub status: Status,
    /// What is being done to it while `Working`: `detecting_speech`,
    /// `diarizing`, `transcribing`, `writing_minutes`.
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub percent: u8,
    /// Why it is not done, as a short code the screen has words for:
    /// `cannot_decode`, `speech_model`, `speech_model_missing`,
    /// `summary_model`, `summary_model_refused`, `summary_limit`,
    /// `summary_unavailable`, `no_speech`, `too_short`.
    #[serde(default)]
    pub error: String,
    /// The error as the failing part reported it, for the log and for help.
    #[serde(default)]
    pub error_detail: String,
    /// How many times transcription has been tried.
    #[serde(default)]
    pub attempts: u32,
    /// How many times writing the minutes has been tried.
    #[serde(default)]
    pub minutes_attempts: u32,
    /// When it joined the queue; the queue is worked in this order.
    #[serde(default)]
    pub queued_at_ms: i64,
    /// When it is next tried again after a failure; 0 when it is not waiting.
    #[serde(default)]
    pub retry_at_ms: i64,
    /// The minutes template chosen for it by a rule or the user; empty lets
    /// the content decide.
    #[serde(default)]
    pub template_id: String,
    #[serde(default)]
    pub people: Vec<String>,
    #[serde(default)]
    pub place: String,
    /// The customer, for a consultation: who the recording is about.
    #[serde(default)]
    pub customer: String,
    /// What the user wrote about this meeting beforehand (or before it is
    /// done again): what it is about, what to listen for.
    #[serde(default)]
    pub note: String,
    /// Names and words for this meeting only: picked from its documents or
    /// added by hand. With the vocabulary (which is for every meeting) they
    /// steer the speech model and the minutes.
    #[serde(default)]
    pub terms: Vec<String>,
    /// Times "중요 표시" was pressed, in milliseconds into the recording.
    #[serde(default)]
    pub marks_ms: Vec<u64>,
    /// What the user wrote beside each mark, in the order of `marks_ms`.
    #[serde(default)]
    pub mark_notes: Vec<String>,
    #[serde(default)]
    pub event: Option<EventRef>,
    /// The microphone, for a recording made in the app.
    #[serde(default)]
    pub device: String,
    /// Where the microphone went away while this was being recorded (a
    /// laptop's lid closed, a headset dropped) and came back, in the order
    /// they happened. The recording itself holds only what was heard.
    #[serde(default)]
    pub gaps: Vec<Gap>,
    /// Set when a recording made here did not end as it should have:
    /// `app_stopped`, `microphone_lost` or `file_incomplete`. It stays with
    /// the meeting and is stated in the minutes and the evidence record.
    #[serde(default)]
    pub capture_note: String,
    #[serde(default)]
    pub transcript: Option<Transcript>,
    #[serde(default)]
    pub stats: Option<Stats>,
    /// The minutes, one per template they were written with; the first is
    /// the one shown by default and saved as `회의록.md`.
    #[serde(default)]
    pub minutes: Vec<Minutes>,
    /// How long writing the minutes took, and the whole of the processing.
    #[serde(default)]
    pub minutes_ms: u64,
    #[serde(default)]
    pub total_ms: u64,
}

/// The most places a search gives for one meeting; `Found::total` counts
/// them all.
pub const FOUND_PLACES: usize = 5;

/// What part of a meeting some words were found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FoundIn {
    Title,
    /// The one-line summary, where the minutes have no story.
    Summary,
    /// A part of the story; `index` says which.
    Story,
    /// A line of the minutes; `section` and `index` say which.
    Entry,
    /// A line of the transcript; `index` says which, `ms` when it was said.
    Transcript,
    /// The people at the meeting.
    People,
}

/// One place a search found its words: the whole line they are in, and
/// enough to find that line again on the screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct FoundPlace {
    pub kind: FoundIn,
    pub text: String,
    /// The minutes section's key, for an entry; empty otherwise.
    pub section: String,
    pub index: u32,
    /// When it was said, for a transcript line.
    pub ms: Option<u64>,
}

/// A meeting a search found, and where in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Found {
    pub id: String,
    /// In the order the minutes screen shows them; at most `FOUND_PLACES`.
    pub places: Vec<FoundPlace>,
    /// How many places there are in all.
    pub total: u32,
}

/// `text` with Korean letters put together into syllables (NFC).
fn nfc(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    text.nfc().collect()
}

/// What a list of meetings shows of one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct MeetingSummary {
    pub id: String,
    pub title: String,
    pub started_at_ms: i64,
    pub duration_ms: u64,
    pub origin: Origin,
    pub status: Status,
    pub stage: String,
    pub percent: u8,
    pub error: String,
    /// The template the minutes were written with: `고객 상담`.
    pub kind: String,
    /// That template's id, or the one chosen for it before the minutes.
    pub template_id: String,
    pub summary: String,
    /// Tasks in the minutes.
    pub actions: u32,
    /// Entries the checks found nothing to doubt in.
    pub verified: u32,
    /// Entries marked for a look, and things the meeting left unsettled.
    pub to_check: u32,
    /// Tasks ticked off.
    pub done: u32,
    pub speakers: u32,
    pub folder: String,
}

impl Meeting {
    /// A meeting that has just arrived and has had nothing done to it.
    pub fn new(started_at_ms: i64, origin: Origin, audio_file: &str) -> Self {
        Self {
            id: new_id(started_at_ms),
            title: String::new(),
            title_is_auto: true,
            started_at_ms,
            time_source: String::new(),
            duration_ms: 0,
            origin,
            audio_file: audio_file.to_string(),
            source_path: String::new(),
            sha256: String::new(),
            status: Status::Queued,
            stage: String::new(),
            percent: 0,
            error: String::new(),
            error_detail: String::new(),
            attempts: 0,
            minutes_attempts: 0,
            queued_at_ms: 0,
            retry_at_ms: 0,
            template_id: String::new(),
            people: Vec::new(),
            place: String::new(),
            customer: String::new(),
            note: String::new(),
            terms: Vec::new(),
            marks_ms: Vec::new(),
            mark_notes: Vec::new(),
            event: None,
            device: String::new(),
            gaps: Vec::new(),
            capture_note: String::new(),
            transcript: None,
            stats: None,
            minutes: Vec::new(),
            minutes_ms: 0,
            total_ms: 0,
        }
    }

    pub fn summary(&self, folder: &Path) -> MeetingSummary {
        let minutes = self.minutes.first();
        MeetingSummary {
            id: self.id.clone(),
            title: self.title.clone(),
            started_at_ms: self.started_at_ms,
            duration_ms: self.duration_ms,
            origin: self.origin,
            status: self.status,
            stage: self.stage.clone(),
            percent: self.percent,
            error: self.error.clone(),
            kind: minutes.map_or(String::new(), |m| m.template_name.clone()),
            template_id: minutes
                .map_or_else(|| self.template_id.clone(), |m| m.template_id.clone()),
            summary: minutes.map_or(String::new(), |m| m.summary.clone()),
            actions: minutes.map_or(0, |m| {
                m.sections
                    .iter()
                    .filter(|s| s.kind == super::templates::SectionKind::Actions)
                    .map(|s| s.entries.len() as u32)
                    .sum()
            }),
            verified: minutes.map_or(0, |m| m.verified),
            // Only what is worth a listen; what was left unsettled is part
            // of the minutes, not something to check.
            to_check: minutes.map_or(0, |m| m.to_check),
            done: minutes.map_or(0, |m| {
                m.sections
                    .iter()
                    .filter(|s| s.kind == super::templates::SectionKind::Actions)
                    .map(|s| s.entries.iter().filter(|e| e.done).count() as u32)
                    .sum()
            }),
            speakers: self
                .transcript
                .as_ref()
                .map_or(0, |t| t.speakers.len() as u32),
            folder: folder.display().to_string(),
        }
    }

    /// Where `words` are in the meeting as its minutes screen shows it: the
    /// title, the tasks, the story (or the one-line summary when there is
    /// none), the other sections, the transcript lines, the people. `None`
    /// when they are nowhere. Case does not matter.
    pub fn found(&self, words: &str) -> Option<Found> {
        // One way of writing Korean: syllables typed on one computer and
        // letters saved apart on another are the same words.
        let words = nfc(words.trim()).to_lowercase();
        if words.is_empty() {
            return None;
        }
        let has = |text: &str| nfc(text).to_lowercase().contains(&words);
        // A task found by who does it shows who, as the minutes line does.
        let line = |entry: &Entry| {
            if has(&entry.text) {
                entry.text.clone()
            } else {
                format!("{} {}", entry.owner, entry.text)
            }
        };
        let mut places = Vec::new();
        let mut add = |kind, text: &str, section: &str, index: usize, ms| {
            places.push(FoundPlace {
                kind,
                text: nfc(text),
                section: section.to_string(),
                index: index as u32,
                ms,
            });
        };
        if has(&self.title) {
            add(FoundIn::Title, &self.title, "", 0, None);
        }
        if let Some(minutes) = self.minutes.first() {
            let entries = |section: &MinutesSection, add: &mut dyn FnMut(&Entry, usize)| {
                for (index, entry) in section.entries.iter().enumerate() {
                    if has(&entry.text) || has(&entry.owner) {
                        add(entry, index);
                    }
                }
            };
            let tasks = minutes
                .sections
                .iter()
                .filter(|s| s.kind == SectionKind::Actions);
            for section in tasks {
                entries(section, &mut |entry, index| {
                    add(FoundIn::Entry, &line(entry), &section.key, index, None)
                });
            }
            for (index, part) in minutes.story.iter().enumerate() {
                if has(&part.text) {
                    add(FoundIn::Story, &part.text, "", index, None);
                } else if has(&part.title) {
                    let text = format!("{} · {}", part.title, part.text);
                    add(FoundIn::Story, &text, "", index, None);
                }
            }
            // The screen shows the summary only where there is no story.
            if minutes.story.is_empty() && has(&minutes.summary) {
                add(FoundIn::Summary, &minutes.summary, "", 0, None);
            }
            // The rest as the screen folds them, the unsettled last.
            let rest = minutes
                .sections
                .iter()
                .filter(|s| s.kind != SectionKind::Actions && s.key != OPEN_SECTION)
                .chain(
                    minutes
                        .sections
                        .iter()
                        .filter(|s| s.kind != SectionKind::Actions && s.key == OPEN_SECTION),
                );
            for section in rest {
                entries(section, &mut |entry, index| {
                    add(FoundIn::Entry, &line(entry), &section.key, index, None)
                });
            }
        }
        if let Some(transcript) = &self.transcript {
            for (index, segment) in transcript.segments.iter().enumerate() {
                if has(segment.said()) {
                    add(
                        FoundIn::Transcript,
                        segment.said(),
                        "",
                        index,
                        Some(segment.start_ms),
                    );
                }
            }
        }
        let people = self.people.join(", ");
        if has(&people) {
            add(FoundIn::People, &people, "", 0, None);
        }
        if places.is_empty() {
            return None;
        }
        let total = places.len() as u32;
        places.truncate(FOUND_PLACES);
        Some(Found {
            id: self.id.clone(),
            places,
            total,
        })
    }

    /// When it was recorded, as the minutes show it: `2026-11-24 (화) 14:00`.
    pub fn recorded_at(&self) -> String {
        local(self.started_at_ms).map_or(String::new(), |time| {
            const WEEKDAYS: [&str; 7] = ["월", "화", "수", "목", "금", "토", "일"];
            format!(
                "{} ({}) {}",
                time.format("%Y-%m-%d"),
                WEEKDAYS[time.weekday().num_days_from_monday() as usize],
                time.format("%H:%M")
            )
        })
    }

    /// The day it was recorded, for the summary model to read dates against.
    /// What the user told about this meeting, for the minutes' writer: the
    /// place, their note and this meeting's words. Empty when nothing.
    pub fn context(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        if !self.place.is_empty() {
            lines.push(format!("현장·장소: {}", self.place));
        }
        if !self.terms.is_empty() {
            lines.push(format!(
                "이 회의에 나오는 이름·용어: {}",
                self.terms.join(", ")
            ));
        }
        if !self.note.is_empty() {
            lines.push(format!("회의 메모: {}", self.note));
        }
        lines.join("\n")
    }

    /// Whether the meeting is with a customer: one is named, or it is
    /// written as a consultation.
    pub fn with_customer(&self) -> bool {
        !self.customer.trim().is_empty() || self.template_id == "consult"
    }

    /// Minutes after its event began that a recording may start without
    /// having missed anything worth saying.
    const LATE_AFTER_MS: i64 = 3 * 60_000;

    /// How many whole minutes after its calendar event began the recording
    /// started: 0 when there is no event, the recording began in time (or
    /// before), or after the event was over (then it is not that event's
    /// start that was missed).
    pub fn late_minutes(&self) -> u32 {
        let Some(event) = &self.event else {
            return 0;
        };
        let late = self.started_at_ms - event.start_ms;
        if late < Self::LATE_AFTER_MS || self.started_at_ms >= event.end_ms {
            return 0;
        }
        (late / 60_000) as u32
    }

    pub fn recorded_on(&self) -> String {
        local(self.started_at_ms).map_or(String::new(), |time| {
            const WEEKDAYS: [&str; 7] = ["월", "화", "수", "목", "금", "토", "일"];
            format!(
                "{} {}요일",
                time.format("%Y-%m-%d"),
                WEEKDAYS[time.weekday().num_days_from_monday() as usize]
            )
        })
    }
}

fn local(ms: i64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(ms).single()
}

/// The time a recorder wrote into a file's name, as most phone and desktop
/// recorders do: `20261124_140012`, `2026-11-24 14-00-12`, `261124_140012`,
/// or this app's own `2026-11-24 1400 제목`. `None` when the name has none,
/// or what looks like one is not a real date.
pub fn time_in_name(name: &str) -> Option<i64> {
    use regex::Regex;
    use std::sync::LazyLock;
    static FULL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(20\d{2})[-_. ]?(\d{2})[-_. ]?(\d{2})[-_ T]+(\d{2})[-_.:시 ]?(\d{2})(?:[-_.:분 ]?(\d{2}))?",
        )
        .expect("a valid pattern")
    });
    static SHORT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:^|\D)(\d{2})(\d{2})(\d{2})[-_ ](\d{2})(\d{2})(\d{2})(?:\D|$)")
            .expect("a valid pattern")
    });
    let number = |captures: &regex::Captures<'_>, index: usize| -> Option<u32> {
        captures.get(index).and_then(|m| m.as_str().parse().ok())
    };
    let (year, captures) = match FULL.captures(name) {
        Some(captures) => (number(&captures, 1)? as i32, captures),
        None => {
            let captures = SHORT.captures(name)?;
            (2000 + number(&captures, 1)? as i32, captures)
        }
    };
    let time = Local
        .with_ymd_and_hms(
            year,
            number(&captures, 2)?,
            number(&captures, 3)?,
            number(&captures, 4)?,
            number(&captures, 5)?,
            number(&captures, 6).unwrap_or(0),
        )
        .single()?;
    // A "time" in the future is a number that only looks like one.
    let latest = Local::now() + chrono::Duration::days(1);
    (time <= latest).then(|| time.timestamp_millis())
}

/// The title a person gave a file by naming it, or nothing when the name is
/// only what a recorder made up (a date, a counter, "녹음", "Recording").
pub fn title_in_name(stem: &str) -> String {
    use regex::Regex;
    use std::sync::LazyLock;
    static NOISE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)20\d{2}[-_. ]?\d{2}[-_. ]?\d{2}|\d{2,}|새로운 녹음|새 녹음|통화 ?녹음|음성 ?녹음|음성 ?메모|녹음|음성|new recording|voice memo|recording|voice|audio|memo|rec|am|pm|오전|오후",
        )
        .expect("a valid pattern")
    });
    let left = NOISE.replace_all(stem, " ");
    let letters = left.chars().filter(|c| c.is_alphabetic()).count();
    if letters >= 2 {
        safe_name(stem)
    } else {
        String::new()
    }
}

/// ISO 8601 with the local offset, for the evidence record.
pub fn iso_local(ms: i64) -> String {
    local(ms).map_or(String::new(), |time| {
        time.to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
    })
}

/// A new meeting id: the start time and a few random characters, so ids sort
/// by time and two recordings in the same second still differ.
pub fn new_id(started_at_ms: i64) -> String {
    let stamp = local(started_at_ms).map_or_else(
        || started_at_ms.to_string(),
        |time| time.format("%Y%m%d-%H%M%S").to_string(),
    );
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    format!("{stamp}-{:04x}", (nanos ^ std::process::id()) & 0xffff)
}

/// Writes `bytes` to `path` so that a crash leaves either the old file or
/// the new one, never half of one.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let partial = path.with_extension(format!(
        "{}.partial",
        path.extension().and_then(|e| e.to_str()).unwrap_or("tmp")
    ));
    std::fs::write(&partial, bytes)
        .with_context(|| format!("could not write {}", partial.display()))?;
    std::fs::rename(&partial, path).with_context(|| format!("could not write {}", path.display()))
}

/// A folder or file name that every system accepts: no path separators or
/// characters Windows forbids, no leading or trailing dots and spaces.
pub fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    cleaned
        .trim_matches(|c: char| c == '.' || c == ' ')
        .chars()
        .take(60)
        .collect::<String>()
        .trim()
        .to_string()
}

/// The library: the folder that holds every meeting's folder.
#[derive(Debug, Clone)]
pub struct Library {
    root: PathBuf,
}

impl Library {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The folder name for a meeting: `2026-11-24 1400 제목`.
    pub fn folder_name(started_at_ms: i64, title: &str) -> String {
        let stamp = local(started_at_ms).map_or_else(
            || "녹음".to_string(),
            |time| time.format("%Y-%m-%d %H%M").to_string(),
        );
        let title = safe_name(title);
        if title.is_empty() {
            stamp
        } else {
            format!("{stamp} {title}")
        }
    }

    /// Makes a new, empty folder for a meeting and returns it. A name already
    /// taken gets a number after it.
    pub fn create_folder(&self, started_at_ms: i64, title: &str) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.root)
            .with_context(|| format!("could not create {}", self.root.display()))?;
        let name = Self::folder_name(started_at_ms, title);
        for number in 1..1000 {
            let candidate = if number == 1 {
                self.root.join(&name)
            } else {
                self.root.join(format!("{name} ({number})"))
            };
            match std::fs::create_dir(&candidate) {
                Ok(()) => return Ok(candidate),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(e)
                        .with_context(|| format!("could not create {}", candidate.display()))
                }
            }
        }
        Err(anyhow!("too many meetings named {name}"))
    }

    /// Renames a meeting's folder once its title or its start time is known
    /// better. The folder keeps its name if the new one is taken or the
    /// rename fails: a name is not worth losing track of a meeting for.
    pub fn retitle_folder(&self, folder: &Path, started_at_ms: i64, title: &str) -> PathBuf {
        let wanted = self.root.join(Self::folder_name(started_at_ms, title));
        if wanted == folder || wanted.exists() {
            return folder.to_path_buf();
        }
        match std::fs::rename(folder, &wanted) {
            Ok(()) => wanted,
            Err(e) => {
                warn!("Could not rename {} ({e})", folder.display());
                folder.to_path_buf()
            }
        }
    }

    /// Every meeting in the library with its folder, newest first. A folder
    /// whose record cannot be read is skipped: one damaged file must not hide
    /// the rest.
    pub fn scan(&self) -> Vec<(Meeting, PathBuf)> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut meetings: Vec<(Meeting, PathBuf)> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.is_dir())
            .filter_map(|folder| match Self::read(&folder) {
                Ok(meeting) => Some((meeting, folder)),
                Err(e) => {
                    if folder.join(RECORD_NAME).exists() {
                        warn!("Skipping {}: {e:#}", folder.display());
                    }
                    None
                }
            })
            .collect();
        meetings.sort_by_key(|(meeting, _)| std::cmp::Reverse(meeting.started_at_ms));
        meetings
    }

    pub fn read(folder: &Path) -> Result<Meeting> {
        let path = folder.join(RECORD_NAME);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("{} is damaged", path.display()))
    }

    /// Saves the meeting's record, and its minutes and transcript as plain
    /// documents when it has them: the minutes with the user's changes, the
    /// transcript as it was transcribed.
    pub fn save(meeting: &Meeting, folder: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(meeting)?;
        write_atomically(&folder.join(RECORD_NAME), json.as_bytes())?;
        if let Some(transcript) = &meeting.transcript {
            write_atomically(
                &folder.join(TRANSCRIPT_NAME),
                transcript.to_text_with_gaps(&meeting.gaps).as_bytes(),
            )?;
            if let Some(minutes) = meeting.minutes.first() {
                let markdown = render::minutes_markdown(
                    minutes,
                    &transcript.as_read(),
                    &meeting.header(),
                    &meeting.marks_ms,
                );
                write_atomically(&folder.join(MINUTES_NAME), markdown.as_bytes())?;
            }
        }
        Ok(())
    }
}

impl Meeting {
    /// What the minutes document says about the recording itself.
    pub fn header(&self) -> Header {
        Header {
            title: self.title.clone(),
            recorded_at: self.recorded_at(),
            duration_ms: self.duration_ms,
            attendees: self.people.clone(),
            place: self.place.clone(),
            customer: self.customer.clone(),
            late_minutes: self.late_minutes(),
            // Only for a consultation: a meeting among colleagues or a memo
            // often starts straight into talk.
            mid_talk: self.with_customer()
                && self
                    .transcript
                    .as_ref()
                    .is_some_and(super::facts::starts_mid_talk),
            source_file: self.audio_file.clone(),
            sha256: self.sha256.clone(),
            transcribed_by: self
                .transcript
                .as_ref()
                .map_or(String::new(), |t| short_model_name(&t.model)),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            capture_note: self.capture_note.clone(),
            gaps: self.gaps.clone(),
        }
    }
}

/// A model id as people know it: `whisper-large-v3-turbo` from
/// `org/repo-gguf/whisper-large-v3-turbo-Q8_0.gguf`.
pub fn short_model_name(id: &str) -> String {
    let file = id.rsplit('/').next().unwrap_or(id);
    let stem = file.strip_suffix(".gguf").unwrap_or(file);
    // The quantization tag is the last dash-separated part: Q8_0, F16, ...
    let is_quant = |tag: &str| {
        matches!(tag, "F16" | "F32" | "BF16")
            || (tag.starts_with('Q') && tag[1..].starts_with(|c: char| c.is_ascii_digit()))
    };
    match stem.rsplit_once('-') {
        Some((name, tag)) if is_quant(tag) => name.to_string(),
        _ => stem.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meeting(id: &str, started_at_ms: i64) -> Meeting {
        let mut meeting = Meeting::new(started_at_ms, Origin::Imported, "a.m4a");
        meeting.id = id.to_string();
        meeting.title = "주간 회의".to_string();
        meeting.duration_ms = 60_000;
        meeting
    }

    #[test]
    fn only_a_consultation_says_it_started_in_the_middle_of_talk() {
        use crate::seogi::transcript::{Segment, Transcript};
        let mut meeting = Meeting::new(0, Origin::Recorded, "녹음.wav");
        meeting.transcript = Some(Transcript {
            segments: vec![Segment {
                start_ms: 0,
                end_ms: 9_000,
                speaker: 1,
                text: "그래서 이 벽을 이렇게 옮기면 거실이 훨씬 넓어 보이는 구조가 되는 거고요"
                    .into(),
                confidence: None,
                unsure: false,
                overlap: false,
                edited: None,
                fixes: Vec::new(),
            }],
            ..Default::default()
        });
        assert!(!meeting.header().mid_talk);
        meeting.customer = "이서진".into();
        assert!(meeting.header().mid_talk);
    }

    #[test]
    fn a_recording_started_well_after_its_event_says_by_how_much() {
        let start = 1_000_000_000_000;
        let mut meeting = Meeting::new(start + 8 * 60_000 + 5_000, Origin::Recorded, "녹음.wav");
        assert_eq!(meeting.late_minutes(), 0);
        meeting.event = Some(EventRef {
            uid: "a".into(),
            title: "상담".into(),
            start_ms: start,
            end_ms: start + 3_600_000,
        });
        assert_eq!(meeting.late_minutes(), 8);
        assert_eq!(meeting.header().late_minutes, 8);
        // Two minutes is in time; after the event is over is another matter.
        meeting.started_at_ms = start + 2 * 60_000;
        assert_eq!(meeting.late_minutes(), 0);
        meeting.started_at_ms = start + 3_700_000;
        assert_eq!(meeting.late_minutes(), 0);
    }

    #[test]
    fn what_the_user_told_about_a_meeting_is_given_to_the_minutes() {
        let mut meeting = Meeting::new(0, Origin::Recorded, "녹음.wav");
        assert_eq!(meeting.context(), "");
        meeting.place = "새봄마을 101동".into();
        meeting.terms = vec!["원목마루".into(), "우물천장".into()];
        meeting.note = "주방 위치를 정함".into();
        assert_eq!(
            meeting.context(),
            "현장·장소: 새봄마을 101동\n이 회의에 나오는 이름·용어: 원목마루, 우물천장\n회의 메모: 주방 위치를 정함"
        );
        // A record from before these existed still reads.
        let old: Meeting = serde_json::from_value(serde_json::json!({
            "id": "x", "title": "", "started_at_ms": 0, "duration_ms": 0,
            "origin": "recorded", "audio_file": "a.wav", "status": "done"
        }))
        .unwrap();
        assert!(old.terms.is_empty() && old.note.is_empty());
    }

    #[test]
    fn a_search_says_which_lines_have_the_words_in_the_order_they_are_shown() {
        use super::super::transcript::{Segment, Transcript};
        let mut meeting = meeting("m1", 0);
        meeting.minutes = vec![serde_json::from_value(serde_json::json!({
            "template_id": "meeting", "template_name": "회의", "title": "",
            "summary": "이사 일정을 정했다.",
            "story": [{ "title": "일정", "text": "이사는 다음 달로 미뤘다.", "sources": [] }],
            "sections": [
                { "key": "open", "title": "남은 것", "kind": "items", "entries": [
                    { "text": "이사 날짜 다시 확인", "owner": "", "due": "", "sources": [], "reasons": [] }
                ] },
                { "key": "decisions", "title": "결정", "kind": "items", "entries": [
                    { "text": "바닥은 원목", "owner": "", "due": "", "sources": [], "reasons": [] },
                    { "text": "이사 전에 청소", "owner": "", "due": "", "sources": [], "reasons": [] }
                ] },
                { "key": "tasks", "title": "할 일", "kind": "actions", "entries": [
                    { "text": "견적 보내기", "owner": "이사팀", "due": "", "sources": [], "reasons": [] }
                ] }
            ],
            "verified": 0, "to_check": 0, "dropped": 0, "written_by": ""
        }))
        .unwrap()];
        meeting.transcript = Some(Transcript {
            segments: vec![
                Segment {
                    start_ms: 1_000,
                    text: "안녕하세요".into(),
                    ..Default::default()
                },
                Segment {
                    start_ms: 65_000,
                    text: "이사는 언제 하세요?".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });

        let found = meeting.found(" 이사 ").unwrap();
        assert_eq!(found.id, "m1");
        let order: Vec<(FoundIn, &str, u32)> = found
            .places
            .iter()
            .map(|p| (p.kind, p.section.as_str(), p.index))
            .collect();
        assert_eq!(
            order,
            vec![
                (FoundIn::Entry, "tasks", 0),
                (FoundIn::Story, "", 0),
                (FoundIn::Entry, "decisions", 1),
                (FoundIn::Entry, "open", 0),
                (FoundIn::Transcript, "", 1),
            ]
        );
        assert_eq!(found.places[4].ms, Some(65_000));
        assert_eq!(found.places[4].text, "이사는 언제 하세요?");
        assert_eq!(found.total, 5);

        // Found only in who does a task, or in a story part's heading: the
        // line shows those words too.
        assert_eq!(
            meeting.found("이사팀").unwrap().places[0].text,
            "이사팀 견적 보내기"
        );
        assert_eq!(
            meeting.found("일정").unwrap().places[0].text,
            "일정 · 이사는 다음 달로 미뤘다."
        );

        // Letters saved apart find and are found as syllables.
        let apart = |text: &str| -> String {
            use unicode_normalization::UnicodeNormalization;
            text.nfd().collect()
        };
        meeting.minutes[0].sections[2].entries[0].text = apart("견적 보내기");
        let found = meeting.found("견적").unwrap();
        assert_eq!(found.places[0].text, "견적 보내기");
        assert!(meeting.found(&apart("견적")).is_some());

        // Without a story the screen shows the summary, and so does a search.
        meeting.minutes[0].story.clear();
        let found = meeting.found("일정").unwrap();
        assert_eq!(found.places[0].kind, FoundIn::Summary);

        assert_eq!(
            meeting.found("주간").unwrap().places[0].kind,
            FoundIn::Title
        );
        assert!(meeting.found("없는 말").is_none());
        assert!(meeting.found("  ").is_none());

        // A long meeting gives the first places and counts the rest.
        let transcript = meeting.transcript.as_mut().unwrap();
        transcript.segments = (0..50)
            .map(|i| Segment {
                start_ms: i * 1000,
                text: "또 그 말".into(),
                ..Default::default()
            })
            .collect();
        let found = meeting.found("그 말").unwrap();
        assert_eq!(found.places.len(), FOUND_PLACES);
        assert_eq!(FOUND_PLACES, 5);
        assert_eq!(found.total, 50);
    }

    #[test]
    fn names_are_safe_on_every_system() {
        assert_eq!(
            safe_name("  [회의실A] 김OO / 상담: 1차?  "),
            "[회의실A] 김OO 상담 1차"
        );
        assert_eq!(safe_name("..."), "");
        assert_eq!(safe_name("a\tb\nc"), "a b c");
        assert!(safe_name(&"가".repeat(200)).chars().count() <= 60);
    }

    #[test]
    fn folders_are_named_by_time_and_title_and_never_collide() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::new(dir.path().join("서기"));
        let at = Local
            .with_ymd_and_hms(2026, 11, 24, 14, 0, 3)
            .unwrap()
            .timestamp_millis();
        let first = library.create_folder(at, "주간 회의").unwrap();
        assert_eq!(first.file_name().unwrap(), "2026-11-24 1400 주간 회의");
        let second = library.create_folder(at, "주간 회의").unwrap();
        assert_eq!(second.file_name().unwrap(), "2026-11-24 1400 주간 회의 (2)");
        let untitled = library.create_folder(at, "").unwrap();
        assert_eq!(untitled.file_name().unwrap(), "2026-11-24 1400");

        // A title learned later renames the folder, unless the name is taken.
        let renamed = library.retitle_folder(&untitled, at, "한빛아파트 상담");
        assert_eq!(
            renamed.file_name().unwrap(),
            "2026-11-24 1400 한빛아파트 상담"
        );
        assert!(renamed.is_dir() && !untitled.exists());
        assert_eq!(library.retitle_folder(&second, at, "주간 회의"), second);
    }

    #[test]
    fn a_library_is_read_back_newest_first_and_skips_damage() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::new(dir.path().to_path_buf());
        for (id, at) in [("old", 1_000_000), ("new", 9_000_000)] {
            let folder = library.create_folder(at, id).unwrap();
            Library::save(&meeting(id, at), &folder).unwrap();
        }
        let broken = dir.path().join("broken");
        std::fs::create_dir(&broken).unwrap();
        std::fs::write(broken.join(RECORD_NAME), "{").unwrap();
        std::fs::create_dir(dir.path().join("not a meeting")).unwrap();

        let ids: Vec<String> = library.scan().into_iter().map(|(m, _)| m.id).collect();
        assert_eq!(ids, vec!["new", "old"]);
    }

    #[test]
    fn the_recorded_time_reads_in_korean() {
        let at = Local
            .with_ymd_and_hms(2026, 11, 24, 14, 5, 0)
            .unwrap()
            .timestamp_millis();
        let meeting = meeting("a", at);
        assert_eq!(meeting.recorded_at(), "2026-11-24 (화) 14:05");
        assert_eq!(meeting.recorded_on(), "2026-11-24 화요일");
        assert!(new_id(at).starts_with("20261124-140500-"));
    }

    #[test]
    fn model_names_lose_their_repository_and_quantization() {
        assert_eq!(
            short_model_name(
                "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf"
            ),
            "whisper-large-v3-turbo"
        );
        assert_eq!(
            short_model_name("Qwen3-ASR-1.7B-F16.gguf"),
            "Qwen3-ASR-1.7B"
        );
        assert_eq!(short_model_name("custom-model"), "custom-model");
    }

    #[test]
    fn a_recorder_s_file_name_gives_the_time_it_was_made() {
        let at = |y, mo, d, h, mi, sec| {
            Local
                .with_ymd_and_hms(y, mo, d, h, mi, sec)
                .unwrap()
                .timestamp_millis()
        };
        assert_eq!(
            time_in_name("20251124_140012.m4a"),
            Some(at(2025, 11, 24, 14, 0, 12))
        );
        assert_eq!(
            time_in_name("녹음 2025-11-24 14-00-12.m4a"),
            Some(at(2025, 11, 24, 14, 0, 12))
        );
        assert_eq!(
            time_in_name("음성 251124_140012.m4a"),
            Some(at(2025, 11, 24, 14, 0, 12))
        );
        assert_eq!(
            time_in_name("2025-11-24 1400 주간 회의.wav"),
            Some(at(2025, 11, 24, 14, 0, 0))
        );
        assert_eq!(time_in_name("회의.m4a"), None);
        assert_eq!(time_in_name("IMG_1234.m4a"), None);
        // Not a date, and not a time that has happened yet.
        assert_eq!(time_in_name("20251340_140012.m4a"), None);
        assert_eq!(time_in_name("20991124_140012.m4a"), None);
    }

    #[test]
    fn a_name_a_person_gave_is_a_title_and_a_recorder_s_is_not() {
        assert_eq!(title_in_name("한빛아파트 상담"), "한빛아파트 상담");
        assert_eq!(
            title_in_name("2025-11-24 1400 주간 회의"),
            "2025-11-24 1400 주간 회의"
        );
        assert_eq!(title_in_name("녹음 2025-11-24 14-00-12"), "");
        assert_eq!(title_in_name("20251124_140012"), "");
        assert_eq!(title_in_name("New Recording 12"), "");
        assert_eq!(title_in_name("음성 251124_140012"), "");
    }

    #[test]
    fn what_the_user_changed_stays_and_the_transcript_file_keeps_the_words_transcribed() {
        use crate::seogi::minutes::Removed;
        // A record saved before tasks and lines could be changed.
        let old = include_str!("../../../src/seogi/preview/fixtures/consult.json");
        let mut meeting: Meeting = serde_json::from_str(old).expect("an older record reads");
        let transcript = meeting.transcript.as_mut().unwrap();
        let first = transcript.segments[0].clone();
        transcript
            .edit_lines(first.start_ms, first.end_ms, "새로 고친 첫 줄입니다")
            .unwrap();
        let minutes = &mut meeting.minutes[0];
        let tasks = minutes
            .sections
            .iter_mut()
            .find(|s| s.kind == SectionKind::Actions)
            .unwrap();
        let written = tasks.entries[0].text.clone();
        tasks.entries[0].rewrite("새로 고친 할 일");
        let gone = tasks.entries.remove(1);
        tasks.removed.push(Removed {
            index: 1,
            entry: gone,
        });
        minutes.recount();

        let dir = tempfile::tempdir().unwrap();
        Library::save(&meeting, dir.path()).unwrap();
        let back = Library::read(dir.path()).unwrap();
        assert_eq!(back, meeting);
        assert_eq!(
            back.minutes[0].sections[0].entries[0].original.as_deref(),
            Some(written.as_str())
        );
        let plain = std::fs::read_to_string(dir.path().join(TRANSCRIPT_NAME)).unwrap();
        assert!(plain.contains(&first.text));
        assert!(!plain.contains("새로 고친 첫 줄입니다"));
        let markdown = std::fs::read_to_string(dir.path().join(MINUTES_NAME)).unwrap();
        assert!(markdown.contains("새로 고친 할 일"));
        // A search finds the line in the user's words.
        let found = back.found("새로 고친 첫 줄").expect("found");
        assert_eq!(found.places[0].kind, FoundIn::Transcript);
    }

    #[test]
    fn the_transcript_file_has_the_vocabulary_s_corrections_and_never_the_user_s_fixes() {
        use crate::seogi::pipeline::apply_vocabulary;
        use crate::seogi::transcript::{Segment, Transcript};
        use crate::seogi::vocab::Vocabulary;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("v.txt");
        std::fs::write(&file, "걸레바지 => 걸레받이\n").unwrap();
        let vocabulary = Vocabulary::load(&file).unwrap();
        let mut lines = Vec::new();
        for (start_ms, heard) in [
            (0, "그  걸레바지는 흰색으로"),
            (5_000, "우물천장탄을 내리면"),
        ] {
            let mut line = Segment {
                start_ms,
                end_ms: start_ms + 900,
                speaker: 1,
                ..Default::default()
            };
            apply_vocabulary(&mut line, heard, &vocabulary);
            lines.push(line);
        }
        let mut transcript = Transcript {
            segments: lines,
            ..Default::default()
        };
        // The vocabulary's correction is marked with what was heard.
        assert_eq!(transcript.segments[0].fixes[0].from, "걸레바지");
        // The user fixes a word, and rewrites the first line.
        let none = Vocabulary::default();
        transcript
            .fix_word(
                5_000,
                0,
                "우물천장탄",
                "우물천장",
                &Vocabulary::one_correction("우물천장탄", "우물천장", &none),
            )
            .unwrap();
        transcript
            .edit_lines(0, 900, "그 걸레받이는 회색으로")
            .unwrap();
        let mut meeting = Meeting::new(0, Origin::Recorded, "녹음.wav");
        meeting.transcript = Some(transcript);
        let folder = tempfile::tempdir().unwrap();
        Library::save(&meeting, folder.path()).unwrap();
        // Byte for byte what it was before words could be fixed: the speech
        // model's words with the vocabulary's corrections, cleaned.
        assert_eq!(
            std::fs::read_to_string(folder.path().join(TRANSCRIPT_NAME)).unwrap(),
            "[00:00:00] 그 걸레받이는 흰색으로\n[00:00:05] 우물천장탄을 내리면\n"
        );
        let read = meeting.transcript.as_ref().unwrap().as_read();
        assert_eq!(read.segments[0].text, "그 걸레받이는 회색으로");
        assert_eq!(read.segments[1].text, "우물천장을 내리면");
    }
}
