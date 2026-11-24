//! The user's calendar: what meeting a recording belongs to.
//!
//! The calendar is read from the secret iCal addresses the user gives (a
//! calendar app's "secret address in iCal format"), or from `.ics` files.
//! Nothing is written back. From an event a recording gets its title, its
//! kind of minutes (through the title rules, which are the user's own data)
//! and the names of the people in it, which also steer the speech model.
//!
//! The addresses are secrets: they are never logged.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use log::warn;
use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;

const RULES_NAME: &str = "calendar-rules.json";
/// An event counts for a recording that starts this long before it or goes
/// on this long after it: people press record early and talk on.
const SLACK_MS: i64 = 15 * 60 * 1_000;
/// The most occurrences of one repeating event read for one question.
const MAX_OCCURRENCES: u16 = 400;

/// One occurrence of a calendar event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Event {
    pub uid: String,
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub location: String,
    /// The names of those invited who have not declined.
    pub attendees: Vec<String>,
}

/// How an event's title is read. The first rule whose pattern matches wins.
///
/// `pattern` is a regular expression; its named groups can be used as
/// `{name}` in `title`, `place` and `mentions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TitleRule {
    pub pattern: String,
    /// The minutes template for such events; empty lets the content decide.
    #[serde(default)]
    pub template: String,
    /// The meeting's title; empty keeps the event's title as it is.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub place: String,
    /// Names in the title that the speech model should know: a customer, a
    /// company.
    #[serde(default)]
    pub mentions: Vec<String>,
}

/// What a title says, by the rules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reading {
    pub template_id: String,
    pub title: String,
    pub place: String,
    pub mentions: Vec<String>,
}

/// The rules a new user starts with: generic examples of a naming habit,
/// `[where] who / what`. Users replace them with their own.
pub fn default_rules() -> Vec<TitleRule> {
    let rule =
        |pattern: &str, template: &str, title: &str, place: &str, mentions: &[&str]| TitleRule {
            pattern: pattern.to_string(),
            template: template.to_string(),
            title: title.to_string(),
            place: place.to_string(),
            mentions: mentions.iter().map(|m| m.to_string()).collect(),
        };
    vec![
        // [회의] 주간 회의
        rule(r"^\[회의\]\s*(?P<topic>.+)$", "meeting", "{topic}", "", &[]),
        // [미팅] 업체 이름 / 납품 일정
        rule(
            r"^\[미팅\]\s*(?P<who>[^/]+?)\s*/\s*(?P<topic>.+)$",
            "vendor",
            "{who} {topic}",
            "",
            &["{who}"],
        ),
        // [통화] 한지안 / 견적 문의
        rule(r"^\[통화\]\s*(?P<topic>.+)$", "call", "{topic}", "", &[]),
        // [회의실A] 한지안 / 1차 상담
        rule(
            r"^\[(?P<place>[^\]]+)\]\s*(?P<who>[^/]+?)\s*/\s*(?P<topic>.+)$",
            "consult",
            "{who} {topic}",
            "{place}",
            &["{who}"],
        ),
    ]
}

/// The user's title rules from `data_dir`, or the starting ones.
pub fn load_rules(data_dir: &Path) -> Vec<TitleRule> {
    let path = data_dir.join(RULES_NAME);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            warn!(
                "{} could not be read ({e}); using the starting rules",
                path.display()
            );
            default_rules()
        }),
        Err(_) => default_rules(),
    }
}

pub fn save_rules(data_dir: &Path, rules: &[TitleRule]) -> Result<()> {
    for rule in rules {
        Regex::new(&rule.pattern)
            .with_context(|| format!("'{}' is not a valid pattern", rule.pattern))?;
    }
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("could not create {}", data_dir.display()))?;
    let json = serde_json::to_string_pretty(rules)?;
    super::store::write_atomically(&data_dir.join(RULES_NAME), json.as_bytes())
}

/// Reads an event's title by the rules. A title no rule matches is kept as
/// the meeting's title, with nothing else known.
pub fn read_title(rules: &[TitleRule], title: &str) -> Reading {
    let title = title.trim();
    for rule in rules {
        let Ok(pattern) = Regex::new(&rule.pattern) else {
            continue;
        };
        let Some(found) = pattern.captures(title) else {
            continue;
        };
        let fill = |text: &str| -> String {
            let mut filled = text.to_string();
            for name in pattern.capture_names().flatten() {
                let value = found.name(name).map_or("", |m| m.as_str()).trim();
                filled = filled.replace(&format!("{{{name}}}"), value);
            }
            filled.trim().to_string()
        };
        let named = fill(&rule.title);
        return Reading {
            template_id: rule.template.clone(),
            title: if named.is_empty() {
                title.to_string()
            } else {
                named
            },
            place: fill(&rule.place),
            mentions: rule
                .mentions
                .iter()
                .map(|mention| fill(mention))
                .filter(|mention| !mention.is_empty())
                .collect(),
        };
    }
    Reading {
        title: title.to_string(),
        ..Default::default()
    }
}

/// The event a recording from `start_ms` to `end_ms` belongs to: the one it
/// shares the most time with, allowing for starting early and running over.
/// A recording that has only just started (`end_ms == start_ms`) belongs to
/// the event going on or about to start.
pub fn best_match(events: &[Event], start_ms: i64, end_ms: i64) -> Option<&Event> {
    let end_ms = end_ms.max(start_ms);
    let length = end_ms - start_ms;
    events
        .iter()
        .filter_map(|event| {
            let from = (event.start_ms - SLACK_MS).max(start_ms);
            let to = (event.end_ms + SLACK_MS).min(end_ms);
            let shared = to - from;
            // At least five minutes together, or half of a short recording.
            let enough = if length == 0 {
                shared >= 0
            } else {
                shared >= (length / 2).min(5 * 60 * 1_000)
            };
            enough.then_some((event, shared))
        })
        .max_by_key(|(event, shared)| (*shared, -(event.start_ms - start_ms).abs()))
        .map(|(event, _)| event)
}

/// An address as it is pasted, made into one that can be read: spaces,
/// quotes and angle brackets around it go, and `webcal://` (what a calendar
/// app's "subscribe" link starts with) becomes `https://`.
pub fn normalize(source: &str) -> String {
    let source = source
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'' || c == '<' || c == '>')
        .trim();
    let lower = source.to_ascii_lowercase();
    if lower.starts_with("webcal://") {
        format!("https://{}", &source["webcal://".len()..])
    } else {
        source.to_string()
    }
}

/// Why a calendar could not be read, as a short code the screen has words
/// for: `not_address` (neither a web address nor a file), `refused` (the
/// address answered that there is nothing there: mistyped, or reset since),
/// `unreachable` (no answer), `not_calendar` (an answer that is not a
/// calendar: a web page's address was given instead of the iCal one).
pub fn trouble(error: &str) -> &'static str {
    if error.contains("calendar file") {
        "not_address"
    } else if error.contains("answered") {
        "refused"
    } else if error.contains("not a calendar") {
        "not_calendar"
    } else {
        "unreachable"
    }
}

/// Reads a calendar from where the user said it is: a web address
/// (`https://` or `webcal://`) or an `.ics` file.
pub fn fetch(source: &str) -> Result<String> {
    let url = normalize(source);
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return std::fs::read_to_string(&url).context("could not read the calendar file");
    }
    tauri::async_runtime::block_on(async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(crate::brand::USER_AGENT)
            .build()?;
        // Errors are reported without the address: it is a secret.
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("the calendar could not be reached ({})", e.without_url()))?;
        if !response.status().is_success() {
            return Err(anyhow!("the calendar answered {}", response.status()));
        }
        response
            .text()
            .await
            .map_err(|e| anyhow!("the calendar could not be read ({})", e.without_url()))
    })
}

/// Events written as an iCalendar file, each occurrence on its own, so a
/// calendar read some other way (the Calendar app on macOS) is read by
/// everything else exactly as a secret address is.
pub fn to_ics(events: &[Event]) -> String {
    fn escape(text: &str) -> String {
        text.replace('\\', "\\\\")
            .replace(';', "\\;")
            .replace(',', "\\,")
            .replace('\r', "")
            .replace('\n', "\\n")
    }
    fn stamp(ms: i64) -> String {
        Utc.timestamp_millis_opt(ms)
            .single()
            .map(|time| time.format("%Y%m%dT%H%M%SZ").to_string())
            .unwrap_or_default()
    }
    let mut text = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Seogi//EN\r\n");
    for event in events {
        text.push_str("BEGIN:VEVENT\r\n");
        text.push_str(&format!("UID:{}\r\n", escape(&event.uid)));
        text.push_str(&format!("SUMMARY:{}\r\n", escape(&event.title)));
        text.push_str(&format!("DTSTART:{}\r\n", stamp(event.start_ms)));
        text.push_str(&format!("DTEND:{}\r\n", stamp(event.end_ms)));
        if !event.location.is_empty() {
            text.push_str(&format!("LOCATION:{}\r\n", escape(&event.location)));
        }
        for name in &event.attendees {
            let name = name.replace(['"', '\r', '\n'], "");
            text.push_str(&format!("ATTENDEE;CN=\"{name}\":mailto:x@invalid\r\n"));
        }
        text.push_str("END:VEVENT\r\n");
    }
    text.push_str("END:VCALENDAR\r\n");
    text
}

/// A property line of an iCalendar file: `NAME;PARAMS:VALUE`.
struct Property<'a> {
    name: String,
    params: &'a str,
    value: &'a str,
}

fn property(line: &str) -> Option<Property<'_>> {
    // The value starts at the first colon outside double quotes.
    let mut quoted = false;
    let colon = line.char_indices().find_map(|(index, c)| match c {
        '"' => {
            quoted = !quoted;
            None
        }
        ':' if !quoted => Some(index),
        _ => None,
    })?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let (name, params) = head.split_once(';').unwrap_or((head, ""));
    Some(Property {
        name: name.trim().to_ascii_uppercase(),
        params,
        value: value.trim(),
    })
}

/// The value of one parameter, without its quotes.
fn param<'a>(params: &'a str, key: &str) -> Option<&'a str> {
    let mut quoted = false;
    let mut start = 0;
    let mut parts: Vec<&str> = Vec::new();
    for (index, c) in params.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ';' if !quoted => {
                parts.push(&params[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&params[start..]);
    parts.into_iter().find_map(|part| {
        let (name, value) = part.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case(key)
            .then(|| value.trim().trim_matches('"'))
    })
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push(' '),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out.trim().to_string()
}

/// A date-time value as milliseconds since 1970 UTC. `None` for a date
/// without a time (an all-day event) or a value that cannot be read.
fn time(value: &str, params: &str) -> Option<i64> {
    let value = value.trim();
    if value.len() == 8 && NaiveDate::parse_from_str(value, "%Y%m%d").is_ok() {
        return None;
    }
    let (body, utc) = match value.strip_suffix('Z') {
        Some(body) => (body, true),
        None => (value, false),
    };
    let naive = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").ok()?;
    if utc {
        return Some(Utc.from_utc_datetime(&naive).timestamp_millis());
    }
    if let Some(zone) = param(params, "TZID").and_then(|id| id.parse::<chrono_tz::Tz>().ok()) {
        return zone
            .from_local_datetime(&naive)
            .earliest()
            .map(|time| time.timestamp_millis());
    }
    // No zone given: the time on this computer's clock.
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|time| time.timestamp_millis())
}

/// One `VEVENT` as written in the file.
#[derive(Default)]
struct Raw {
    uid: String,
    title: String,
    location: String,
    attendees: Vec<String>,
    cancelled: bool,
    start: Option<i64>,
    end: Option<i64>,
    /// `DTSTART`, `RRULE`, `EXDATE` and `RDATE` as written, for the
    /// recurrence reader.
    start_line: String,
    recurrence: Vec<String>,
    repeats: bool,
    /// The occurrence this event replaces, for a changed instance of a
    /// repeating event.
    replaces: Option<i64>,
}

fn raw_events(ics: &str) -> Vec<Raw> {
    // Long lines are folded: a line starting with a space or a tab
    // continues the one before.
    let mut lines: Vec<String> = Vec::new();
    for line in ics.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        match line.strip_prefix(' ').or_else(|| line.strip_prefix('\t')) {
            Some(rest) if !lines.is_empty() => {
                if let Some(last) = lines.last_mut() {
                    last.push_str(rest);
                }
            }
            _ => lines.push(line.to_string()),
        }
    }

    let mut events: Vec<Raw> = Vec::new();
    let mut current: Option<Raw> = None;
    // Alarms and other parts inside an event have properties of their own.
    let mut nested = 0u32;
    for line in &lines {
        let Some(found) = property(line) else {
            continue;
        };
        match (
            found.name.as_str(),
            found.value.to_ascii_uppercase().as_str(),
        ) {
            ("BEGIN", "VEVENT") => {
                current = Some(Raw::default());
                nested = 0;
                continue;
            }
            ("END", "VEVENT") => {
                if let Some(event) = current.take() {
                    events.push(event);
                }
                continue;
            }
            ("BEGIN", _) => {
                nested += 1;
                continue;
            }
            ("END", _) => {
                nested = nested.saturating_sub(1);
                continue;
            }
            _ => {}
        }
        let Some(event) = current.as_mut().filter(|_| nested == 0) else {
            continue;
        };
        match found.name.as_str() {
            "UID" => event.uid = found.value.to_string(),
            "SUMMARY" => event.title = unescape(found.value),
            "LOCATION" => event.location = unescape(found.value),
            "STATUS" => event.cancelled = found.value.eq_ignore_ascii_case("CANCELLED"),
            "DTSTART" => {
                event.start = time(found.value, found.params);
                event.start_line = line.clone();
            }
            "DTEND" => event.end = time(found.value, found.params),
            "RRULE" => {
                event.repeats = true;
                event.recurrence.push(line.clone());
            }
            "EXDATE" | "RDATE" => event.recurrence.push(line.clone()),
            "RECURRENCE-ID" => event.replaces = time(found.value, found.params),
            "ATTENDEE" => {
                let declined = param(found.params, "PARTSTAT")
                    .is_some_and(|status| status.eq_ignore_ascii_case("DECLINED"));
                // Only a name is of use; an address alone is not one.
                let name = param(found.params, "CN").map(unescape).unwrap_or_default();
                if !declined && !name.is_empty() && !name.contains('@') {
                    event.attendees.push(name);
                }
            }
            _ => {}
        }
    }
    events
}

/// The starts of a repeating event between `from_ms` and `to_ms`.
fn occurrences(raw: &Raw, from_ms: i64, to_ms: i64) -> Result<Vec<i64>> {
    let mut text = raw.start_line.clone();
    for line in &raw.recurrence {
        text.push('\n');
        text.push_str(line);
    }
    let set: rrule::RRuleSet = text
        .parse()
        .map_err(|e| anyhow!("the repeat rule could not be read ({e})"))?;
    let at = |ms: i64| {
        Utc.timestamp_millis_opt(ms)
            .single()
            .map(|time| time.with_timezone(&rrule::Tz::UTC))
            .ok_or_else(|| anyhow!("a time out of range"))
    };
    let found = set
        .after(at(from_ms)?)
        .before(at(to_ms)?)
        .all(MAX_OCCURRENCES);
    Ok(found
        .dates
        .into_iter()
        .map(|time| time.timestamp_millis())
        .collect())
}

/// The events of an iCalendar text that take place, in part, between
/// `from_ms` and `to_ms`, earliest first. Repeating events are expanded,
/// with their exceptions and changed instances. All-day and cancelled
/// events are left out: nobody records those.
pub fn events_between(ics: &str, from_ms: i64, to_ms: i64) -> Vec<Event> {
    let raws = raw_events(ics);
    // Instances of repeating events that were changed or cancelled.
    let replaced: HashSet<(&str, i64)> = raws
        .iter()
        .filter_map(|raw| raw.replaces.map(|at| (raw.uid.as_str(), at)))
        .collect();

    let mut events: Vec<Event> = Vec::new();
    for raw in &raws {
        let Some(start) = raw.start else {
            continue;
        };
        if raw.cancelled {
            continue;
        }
        let length = raw.end.map_or(60 * 60 * 1_000, |end| (end - start).max(0));
        let starts = if raw.repeats && raw.replaces.is_none() {
            match occurrences(raw, from_ms - length, to_ms) {
                Ok(starts) => starts
                    .into_iter()
                    .filter(|at| !replaced.contains(&(raw.uid.as_str(), *at)))
                    .collect(),
                Err(e) => {
                    warn!("A repeating calendar event was read as a single one: {e:#}");
                    vec![start]
                }
            }
        } else {
            vec![start]
        };
        for at in starts {
            if at + length < from_ms || at > to_ms {
                continue;
            }
            events.push(Event {
                uid: raw.uid.clone(),
                title: raw.title.clone(),
                start_ms: at,
                end_ms: at + length,
                location: raw.location.clone(),
                attendees: raw.attendees.clone(),
            });
        }
    }
    events.sort_by(|a, b| (a.start_ms, &a.uid).cmp(&(b.start_ms, &b.uid)));
    events.dedup_by(|a, b| a.uid == b.uid && a.start_ms == b.start_ms);
    events
}

/// The events of several calendars together, each event once.
pub fn events_of(calendars: &[String], from_ms: i64, to_ms: i64) -> Vec<Event> {
    let mut seen: HashMap<(String, i64), ()> = HashMap::new();
    let mut events: Vec<Event> = calendars
        .iter()
        .flat_map(|ics| events_between(ics, from_ms, to_ms))
        .filter(|event| {
            seen.insert((event.uid.clone(), event.start_ms), ())
                .is_none()
        })
        .collect();
    events.sort_by_key(|event| event.start_ms);
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Asia::Seoul;

    const CALENDAR: &str = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
BEGIN:VEVENT\r\n\
UID:weekly\r\n\
SUMMARY:[회의] 주간 회의\r\n\
DTSTART;TZID=Asia/Seoul:20261102T100000\r\n\
DTEND;TZID=Asia/Seoul:20261102T110000\r\n\
RRULE:FREQ=WEEKLY;BYDAY=MO\r\n\
EXDATE;TZID=Asia/Seoul:20261116T100000\r\n\
BEGIN:VALARM\r\n\
TRIGGER:-PT10M\r\n\
SUMMARY:알림\r\n\
END:VALARM\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
UID:weekly\r\n\
RECURRENCE-ID;TZID=Asia/Seoul:20261123T100000\r\n\
SUMMARY:[회의] 주간 회의\r\n\
DTSTART;TZID=Asia/Seoul:20261123T140000\r\n\
DTEND;TZID=Asia/Seoul:20261123T150000\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
UID:consult\r\n\
SUMMARY:[회의실A] 김서윤 /\r\n  1차 상담\\, 견적\r\n\
DTSTART:20261124T050000Z\r\n\
DTEND:20261124T060000Z\r\n\
LOCATION:회의실A\r\n\
ATTENDEE;CN=박도현;PARTSTAT=ACCEPTED:mailto:a@example.com\r\n\
ATTENDEE;CN=\"정다은\";PARTSTAT=DECLINED:mailto:b@example.com\r\n\
ATTENDEE;PARTSTAT=ACCEPTED:mailto:c@example.com\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
UID:allday\r\n\
SUMMARY:종일 행사\r\n\
DTSTART;VALUE=DATE:20261124\r\n\
DTEND;VALUE=DATE:20261125\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
UID:off\r\n\
SUMMARY:취소된 회의\r\n\
STATUS:CANCELLED\r\n\
DTSTART:20261124T070000Z\r\n\
DTEND:20261124T080000Z\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

    fn seoul(month: u32, day: u32, hour: u32, minute: u32) -> i64 {
        Seoul
            .with_ymd_and_hms(2026, month, day, hour, minute, 0)
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn repeating_events_are_expanded_with_exceptions_and_changes() {
        let events = events_between(CALENDAR, seoul(11, 1, 0, 0), seoul(12, 1, 0, 0));
        let weekly: Vec<i64> = events
            .iter()
            .filter(|event| event.uid == "weekly")
            .map(|event| event.start_ms)
            .collect();
        assert_eq!(
            weekly,
            vec![
                seoul(11, 2, 10, 0),
                seoul(11, 9, 10, 0),
                // 11/16 is excluded, 11/23 was moved to the afternoon.
                seoul(11, 23, 14, 0),
                seoul(11, 30, 10, 0),
            ]
        );
        assert!(events
            .iter()
            .all(|event| event.end_ms - event.start_ms == 3_600_000));
    }

    #[test]
    fn an_event_is_read_with_its_people_and_without_what_nobody_records() {
        let events = events_between(CALENDAR, seoul(11, 24, 0, 0), seoul(11, 25, 0, 0));
        assert_eq!(events.len(), 1);
        let event = &events[0];
        // The folded line is joined and the escaped comma restored.
        assert_eq!(event.title, "[회의실A] 김서윤 / 1차 상담, 견적");
        assert_eq!(event.start_ms, seoul(11, 24, 14, 0));
        assert_eq!(event.location, "회의실A");
        // Whoever declined, or has only an address, is not listed.
        assert_eq!(event.attendees, vec!["박도현"]);
    }

    #[test]
    fn titles_are_read_by_the_rules_and_kept_when_none_fits() {
        let rules = default_rules();
        let consult = read_title(&rules, "[회의실A] 김서윤 / 1차 상담");
        assert_eq!(consult.template_id, "consult");
        assert_eq!(consult.title, "김서윤 1차 상담");
        assert_eq!(consult.place, "회의실A");
        assert_eq!(consult.mentions, vec!["김서윤"]);

        let internal = read_title(&rules, "[회의] 주간 회의");
        assert_eq!(
            (internal.template_id.as_str(), internal.title.as_str()),
            ("meeting", "주간 회의")
        );
        let vendor = read_title(&rules, "[미팅] 새봄타일 / 납품 일정");
        assert_eq!(
            (vendor.template_id.as_str(), vendor.title.as_str()),
            ("vendor", "새봄타일 납품 일정")
        );
        assert_eq!(vendor.mentions, vec!["새봄타일"]);

        let plain = read_title(&rules, "점심");
        assert_eq!(plain.title, "점심");
        assert!(plain.template_id.is_empty() && plain.mentions.is_empty());
        // A rule that is not a valid pattern is skipped, not fatal.
        let broken = vec![TitleRule {
            pattern: "([".into(),
            template: "x".into(),
            title: String::new(),
            place: String::new(),
            mentions: Vec::new(),
        }];
        assert_eq!(read_title(&broken, "점심").title, "점심");
    }

    #[test]
    fn rules_are_saved_only_when_their_patterns_are_valid() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_rules(dir.path()), default_rules());
        let mut rules = default_rules();
        rules.truncate(1);
        save_rules(dir.path(), &rules).unwrap();
        assert_eq!(load_rules(dir.path()), rules);
        rules[0].pattern = "([".into();
        assert!(save_rules(dir.path(), &rules).is_err());
    }

    #[test]
    fn a_recording_belongs_to_the_event_it_shares_the_most_time_with() {
        let events = events_between(CALENDAR, seoul(11, 23, 0, 0), seoul(11, 25, 0, 0));
        let title_at =
            |start: i64, end: i64| best_match(&events, start, end).map(|e| e.uid.as_str());
        // Started ten minutes early, ran five minutes over.
        assert_eq!(
            title_at(seoul(11, 24, 13, 50), seoul(11, 24, 15, 5)),
            Some("consult")
        );
        // A recording that has only just begun, a little before the hour.
        assert_eq!(
            title_at(seoul(11, 24, 13, 55), seoul(11, 24, 13, 55)),
            Some("consult")
        );
        // Hours away from anything.
        assert_eq!(title_at(seoul(11, 24, 9, 0), seoul(11, 24, 9, 30)), None);
        // The moved weekly meeting is found at its new time, not its old one.
        assert_eq!(
            title_at(seoul(11, 23, 14, 5), seoul(11, 23, 14, 50)),
            Some("weekly")
        );
        assert_eq!(title_at(seoul(11, 23, 10, 5), seoul(11, 23, 10, 50)), None);
    }

    #[test]
    fn events_read_another_way_are_read_back_the_same() {
        let events = vec![
            Event {
                uid: "weekly".into(),
                title: "[회의] 주간 회의, 1차; 정리".into(),
                start_ms: seoul(11, 24, 10, 0),
                end_ms: seoul(11, 24, 11, 0),
                location: "회의실A".into(),
                attendees: vec!["박도현".into()],
            },
            Event {
                uid: "weekly".into(),
                title: "[회의] 주간 회의, 1차; 정리".into(),
                start_ms: seoul(12, 1, 10, 0),
                end_ms: seoul(12, 1, 11, 0),
                location: String::new(),
                attendees: Vec::new(),
            },
        ];
        let text = to_ics(&events);
        let read = events_of(&[text], seoul(11, 1, 0, 0), seoul(12, 31, 0, 0));
        assert_eq!(read, events);
    }

    #[test]
    fn a_pasted_address_is_made_readable() {
        let google =
            "https://calendar.google.com/calendar/ical/me%40example.com/private-0123/basic.ics";
        assert_eq!(normalize(&format!("  {google}\n")), google);
        assert_eq!(normalize(&format!("\"{google}\"")), google);
        assert_eq!(
            normalize("webcal://calendar.google.com/calendar/ical/x/private-1/basic.ics"),
            "https://calendar.google.com/calendar/ical/x/private-1/basic.ics"
        );
        assert_eq!(
            normalize("WEBCAL://example.com/a.ics"),
            "https://example.com/a.ics"
        );
        assert_eq!(trouble("could not read the calendar file"), "not_address");
        assert_eq!(trouble("the calendar answered 404 Not Found"), "refused");
        assert_eq!(trouble("not a calendar"), "not_calendar");
        assert_eq!(
            trouble("the calendar could not be reached (timed out)"),
            "unreachable"
        );
    }

    #[test]
    fn a_calendar_is_read_from_a_file_and_a_missing_one_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("달력.ics");
        std::fs::write(&path, CALENDAR).unwrap();
        assert_eq!(fetch(&path.display().to_string()).unwrap(), CALENDAR);
        assert!(fetch(&dir.path().join("없음.ics").display().to_string()).is_err());
        // Two copies of a calendar give each event once.
        let both = vec![CALENDAR.to_string(), CALENDAR.to_string()];
        assert_eq!(
            events_of(&both, seoul(11, 24, 0, 0), seoul(11, 25, 0, 0)).len(),
            1
        );
    }
}
