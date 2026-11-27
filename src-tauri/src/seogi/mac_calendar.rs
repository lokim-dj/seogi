//! The calendars of macOS's Calendar app, read through EventKit.
//!
//! Someone whose organisation hides the secret iCal address can still add
//! their account to the Calendar app; Seogi then reads it from there. The
//! system asks the user once whether Seogi may read calendars. Nothing is
//! written back, and events are read only for the calendars the user ticked.
//! Events the user declined, or is not invited to, are left out.
//!
//! EventKit's connection inside a long-running process can go stale: it then
//! gives no calendars at all, for hours, while the Calendar app shows them
//! and access is granted, and only a new process reads them again. So an
//! empty answer is not taken at its word: EventKit is asked to refresh and
//! read again, and then a short-lived worker process (this same app, started
//! with [`WORKER_FLAG`]) reads over a fresh connection. Only what all of them
//! agree on counts.
//!
//! Elsewhere than macOS the functions say the calendars are not there.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::calendar::Event;

/// A calendar in the Calendar app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MacCalendar {
    /// The Calendar app's own id for it, which stays the same.
    pub id: String,
    pub title: String,
    /// The account it belongs to: a Google address, "iCloud", "On My Mac".
    pub account: String,
}

/// Why the Calendar app's calendars could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    /// macOS does not let Seogi read calendars.
    Denied,
    /// The Calendar app has calendars, but none of the chosen ones: they
    /// were deleted or their account was taken out. Choosing again helps.
    Missing,
    /// Seogi may read calendars, yet none came back, even after asking
    /// EventKit to refresh and over a fresh connection, while calendars were
    /// chosen from it before or it has accounts. Restarting Seogi helps.
    Empty,
}

impl Unread {
    /// The reason as the settings screen has words for it.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Denied => "mac_denied",
            Self::Missing => "mac_missing",
            Self::Empty => "mac_unreadable",
        }
    }
}

impl std::fmt::Display for Unread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Denied => "Seogi may not read calendars",
            Self::Missing => "none of the chosen calendars is in the Calendar app",
            Self::Empty => {
                "the Calendar app gave no calendars though Seogi may read them, also after a refresh and over a fresh connection"
            }
        })
    }
}

impl std::error::Error for Unread {}

/// Why reading failed, when it is one of the reasons in [`Unread`].
pub fn unread(error: &anyhow::Error) -> Option<Unread> {
    error.downcast_ref::<Unread>().copied()
}

/// Whether Seogi may read calendars: `granted`, `denied`, `restricted`,
/// `not_determined` (not asked yet), or `unsupported` (not macOS).
pub fn access() -> &'static str {
    imp::access()
}

/// Asks the system for permission to read calendars, which shows its
/// question the first time, and returns the answer as [`access`] does.
/// Blocks until the user answers.
pub fn request() -> Result<&'static str> {
    imp::request()
}

/// The calendars in the Calendar app. An empty list means the Calendar app
/// really has none: a fresh connection saw no account either. When it has
/// accounts but no calendar came back the error is [`Unread::Empty`].
pub fn calendars() -> Result<Vec<MacCalendar>> {
    imp::calendars()
}

/// The events from `from_ms` to `to_ms` in the calendars with the given
/// ids, each occurrence of a repeating event on its own.
pub fn events(ids: &[String], from_ms: i64, to_ms: i64) -> Result<Vec<Event>> {
    imp::events(ids, from_ms, to_ms)
}

/// Starts the worker that reads over a fresh connection to EventKit:
/// `<exe> --seogi-calendar-worker <from_ms> <to_ms> [<id>...]`. It prints a
/// [`Look`] as JSON on its standard output. It never asks for access.
pub const WORKER_FLAG: &str = "--seogi-calendar-worker";

/// Whether this process was started as the calendar worker.
pub fn is_worker_invocation() -> bool {
    std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == WORKER_FLAG)
}

/// The worker: reads once and prints what it saw. Returns the exit code.
pub fn run_worker() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let [from, to, ids @ ..] = args.as_slice() else {
        eprintln!("calendar worker: expected <from_ms> <to_ms> [<id>...]");
        return 2;
    };
    let (Ok(from), Ok(to)) = (from.parse::<i64>(), to.parse::<i64>()) else {
        eprintln!("calendar worker: the times are not numbers");
        return 2;
    };
    let printed = imp::look_here(ids, from, to)
        .and_then(|look| serde_json::to_string(&look).map_err(Into::into));
    match printed {
        Ok(json) => {
            println!("{json}");
            0
        }
        Err(e) => {
            eprintln!("calendar worker: {e:#}");
            1
        }
    }
}

/// What one look at the Calendar app saw.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Look {
    pub calendars: Vec<MacCalendar>,
    /// The accounts the Calendar app has (iCloud, a Google address, On My
    /// Mac), whether or not any of their calendars came back.
    pub sources: usize,
    /// The events of the chosen calendars, when some were chosen and found.
    pub events: Vec<Event>,
}

impl Look {
    /// Whether this look can be used: it has calendars, and one of the
    /// chosen ones among them when some are chosen.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn usable(&self, ids: &[String]) -> bool {
        !self.calendars.is_empty()
            && (ids.is_empty() || self.calendars.iter().any(|c| ids.contains(&c.id)))
    }
}

/// What the looks at the Calendar app add up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
enum Verdict {
    /// The look at this place in the list is good.
    Use(usize),
    /// The Calendar app really has no calendars: a fresh connection saw no
    /// account either.
    NoCalendars,
    Unread(Unread),
}

/// Weighs the looks in the order they were taken: in this process, again
/// after asking EventKit to refresh, and over a fresh connection when one
/// could be made (`fresh`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn verdict(looks: &[Look], ids: &[String], fresh: bool) -> Verdict {
    if let Some(index) = looks.iter().position(|look| look.usable(ids)) {
        return Verdict::Use(index);
    }
    if looks.iter().any(|look| !look.calendars.is_empty()) {
        // Calendars came back, only not the chosen ones.
        return Verdict::Unread(Unread::Missing);
    }
    if ids.is_empty() && fresh && looks.iter().all(|look| look.sources == 0) {
        return Verdict::NoCalendars;
    }
    // Calendars were chosen from this app before, or it has accounts, or
    // no fresh connection could be made: no calendar coming back means the
    // connection is stuck, not that they are gone.
    Verdict::Unread(Unread::Empty)
}

/// Reads in every way there is until one gives an answer: over this
/// process's connection (`here(false)`), the same after EventKit was asked
/// to refresh (`here(true)`), then over a fresh connection (`fresh`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn read(
    ids: &[String],
    mut here: impl FnMut(bool) -> Result<Look>,
    fresh: impl FnOnce() -> Result<Look>,
) -> Result<Look> {
    let mut looks = vec![here(false)?];
    if looks[0].usable(ids) {
        return Ok(looks.swap_remove(0));
    }
    log::info!("The Calendar app gave no chosen calendar; asking EventKit to refresh");
    looks.push(here(true)?);
    let mut fresh_ran = false;
    if !looks[1].usable(ids) {
        log::info!("Still none; reading over a fresh connection");
        match fresh() {
            Ok(look) => {
                fresh_ran = true;
                looks.push(look);
            }
            Err(e) => log::warn!("Could not read over a fresh connection: {e:#}"),
        }
    }
    match verdict(&looks, ids, fresh_ran) {
        Verdict::Use(index) => {
            log::info!("The calendars came back on try {}", index + 1);
            Ok(looks.swap_remove(index))
        }
        Verdict::NoCalendars => Ok(Look::default()),
        Verdict::Unread(why) => Err(why.into()),
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::collections::HashSet;
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use anyhow::{anyhow, bail, Context, Result};
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, NSObjectProtocol};
    use objc2::sel;
    use objc2_event_kit::{
        EKAuthorizationStatus, EKCalendar, EKEntityType, EKEventStatus, EKEventStore,
        EKParticipant, EKParticipantStatus,
    };
    use objc2_foundation::{NSArray, NSDate, NSError};

    use super::super::calendar::{is_mine, standing, Person, Standing};
    use super::{Event, Look, MacCalendar, Unread, WORKER_FLAG};

    /// How long the worker may take before it is given up on.
    const WORKER_TIMEOUT: Duration = Duration::from_secs(30);
    /// How long EventKit is given to refresh before it is read again.
    const REFRESH_WAIT: Duration = Duration::from_secs(2);

    pub fn access() -> &'static str {
        let status = unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) };
        match status {
            EKAuthorizationStatus::FullAccess => "granted",
            EKAuthorizationStatus::NotDetermined => "not_determined",
            EKAuthorizationStatus::Restricted => "restricted",
            // Write-only access cannot read events.
            _ => "denied",
        }
    }

    pub fn request() -> Result<&'static str> {
        if access() != "not_determined" {
            return Ok(access());
        }
        let store = unsafe { EKEventStore::new() };
        let (send, answer) = mpsc::channel::<bool>();
        let block = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
            let _ = send.send(granted.as_bool());
        });
        let full = sel!(requestFullAccessToEventsWithCompletion:);
        unsafe {
            if store.respondsToSelector(full) {
                store.requestFullAccessToEventsWithCompletion(RcBlock::as_ptr(&block));
            } else {
                // Before macOS 14 there is one kind of access.
                #[allow(deprecated)]
                store.requestAccessToEntityType_completion(
                    EKEntityType::Event,
                    RcBlock::as_ptr(&block),
                );
            }
        }
        // The question waits for the user; a long wait is not an error.
        let _ = answer.recv_timeout(Duration::from_secs(10 * 60));
        Ok(access())
    }

    fn store() -> Result<Retained<EKEventStore>> {
        if access() != "granted" {
            return Err(Unread::Denied.into());
        }
        Ok(unsafe { EKEventStore::new() })
    }

    fn describe(calendar: &EKCalendar) -> MacCalendar {
        unsafe {
            MacCalendar {
                id: calendar.calendarIdentifier().to_string(),
                title: calendar.title().to_string(),
                account: calendar
                    .source()
                    .map(|source| source.title().to_string())
                    .unwrap_or_default(),
            }
        }
    }

    /// One look over `store`'s connection: its calendars and accounts, and
    /// the events of the chosen calendars.
    fn look(store: &EKEventStore, ids: &[String], from_ms: i64, to_ms: i64) -> Look {
        let all = unsafe { store.calendarsForEntityType(EKEntityType::Event) };
        let mut calendars: Vec<MacCalendar> = all.iter().map(|c| describe(&c)).collect();
        calendars.sort_by(|a, b| (&a.account, &a.title).cmp(&(&b.account, &b.title)));
        let sources = unsafe { store.sources() }.count();
        let chosen: Vec<Retained<EKCalendar>> = all
            .iter()
            .filter(|calendar| ids.contains(&unsafe { calendar.calendarIdentifier() }.to_string()))
            .collect();
        let events = if chosen.is_empty() {
            Vec::new()
        } else {
            events_in(store, &chosen, from_ms, to_ms)
        };
        Look {
            calendars,
            sources,
            events,
        }
    }

    /// The worker's look: over this process's own, fresh connection.
    pub fn look_here(ids: &[String], from_ms: i64, to_ms: i64) -> Result<Look> {
        let store = store()?;
        Ok(look(&store, ids, from_ms, to_ms))
    }

    /// Reads, not trusting an empty answer: see the module's notes.
    fn read(ids: &[String], from_ms: i64, to_ms: i64) -> Result<Look> {
        let store = store()?;
        super::read(
            ids,
            |refresh| {
                if refresh {
                    unsafe {
                        store.refreshSourcesIfNecessary();
                        store.reset();
                    }
                    std::thread::sleep(REFRESH_WAIT);
                }
                Ok(look(&store, ids, from_ms, to_ms))
            },
            || fresh_look(ids, from_ms, to_ms),
        )
    }

    /// A look over a fresh connection: this app started again as a worker
    /// for one read. macOS counts it as the same app, so it reads with the
    /// access already given and never asks.
    fn fresh_look(ids: &[String], from_ms: i64, to_ms: i64) -> Result<Look> {
        let exe = std::env::current_exe().context("could not find the app's own executable")?;
        let mut child = Command::new(exe)
            .arg(WORKER_FLAG)
            .arg(from_ms.to_string())
            .arg(to_ms.to_string())
            .args(ids)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("could not start the calendar worker")?;
        // Drained on their own threads: a worker that fills a pipe would block.
        let drain = |pipe: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut text = String::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_string(&mut text);
                }
                text
            })
        };
        let out = drain(
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        );
        let err = drain(
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        );
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().context("could not wait for the worker")? {
                break status;
            }
            if started.elapsed() > WORKER_TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                bail!("the calendar worker took too long");
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let out = out.join().unwrap_or_default();
        let err = err.join().unwrap_or_default();
        if !status.success() {
            return Err(anyhow!(
                "the calendar worker failed ({status}): {}",
                err.trim()
            ));
        }
        serde_json::from_str(out.trim()).context("the calendar worker's answer is not readable")
    }

    pub fn calendars() -> Result<Vec<MacCalendar>> {
        Ok(read(&[], 0, 0)?.calendars)
    }

    pub fn events(ids: &[String], from_ms: i64, to_ms: i64) -> Result<Vec<Event>> {
        Ok(read(ids, from_ms, to_ms)?.events)
    }

    /// A participant as [`standing`] reads one.
    fn person(participant: &EKParticipant, organizer: bool) -> Person {
        unsafe {
            let url = participant.URL();
            let text = url
                .resourceSpecifier()
                .or_else(|| url.absoluteString())
                .map(|text| text.to_string())
                .unwrap_or_default();
            Person {
                address: text.trim_start_matches("mailto:").to_string(),
                me: participant.isCurrentUser(),
                declined: participant.participantStatus() == EKParticipantStatus::Declined,
                organizer,
            }
        }
    }

    fn ms(date: &NSDate) -> i64 {
        (date.timeIntervalSince1970() * 1000.0).round() as i64
    }

    fn events_in(
        store: &EKEventStore,
        chosen: &[Retained<EKCalendar>],
        from_ms: i64,
        to_ms: i64,
    ) -> Vec<Event> {
        let calendars = NSArray::from_retained_slice(chosen);
        let from = NSDate::dateWithTimeIntervalSince1970(from_ms as f64 / 1000.0);
        let to = NSDate::dateWithTimeIntervalSince1970(to_ms as f64 / 1000.0);
        let found = unsafe {
            let predicate = store.predicateForEventsWithStartDate_endDate_calendars(
                &from,
                &to,
                Some(&calendars),
            );
            store.eventsMatchingPredicate(&predicate)
        };
        // Each event with its account and where the user stands on it.
        let mut read: Vec<(Event, String, Standing)> = Vec::new();
        for event in found.iter() {
            unsafe {
                // An all-day event is no meeting, as in an iCal file.
                if event.status() == EKEventStatus::Canceled || event.isAllDay() {
                    continue;
                }
                // One id for every occurrence of a repeating event, as an
                // iCal file has, so muting one mutes them all.
                let uid = event
                    .calendarItemExternalIdentifier()
                    .or_else(|| event.eventIdentifier())
                    .map(|id| id.to_string())
                    .unwrap_or_default();
                let attendees = event
                    .attendees()
                    .map(|people| {
                        people
                            .iter()
                            .filter(|person| {
                                person.participantStatus() != EKParticipantStatus::Declined
                            })
                            .filter_map(|person| person.name().map(|name| name.to_string()))
                            .filter(|name| !name.is_empty() && !name.contains('@'))
                            .collect()
                    })
                    .unwrap_or_default();
                // The account of a Google or Exchange calendar is named
                // after the user's address.
                let account = event
                    .calendar()
                    .and_then(|calendar| calendar.source())
                    .map(|source| source.title().to_string())
                    .unwrap_or_default();
                let mut people: Vec<Person> = event
                    .attendees()
                    .map(|list| list.iter().map(|p| person(&p, false)).collect())
                    .unwrap_or_default();
                if !people.is_empty() {
                    if let Some(organizer) = event.organizer() {
                        people.push(person(&organizer, true));
                    }
                }
                let me = account.contains('@').then_some(account.as_str());
                let found = standing(&people, me);
                read.push((
                    Event {
                        uid,
                        title: event.title().to_string(),
                        start_ms: ms(&event.startDate()),
                        end_ms: ms(&event.endDate()),
                        location: event.location().map(|l| l.to_string()).unwrap_or_default(),
                        attendees,
                    },
                    account,
                    found,
                ));
            }
        }
        // An account that never tells the user apart keeps every event,
        // rather than hiding all those with people in them.
        let knowing: HashSet<String> = read
            .iter()
            .filter(|(_, _, found)| matches!(found, Standing::Mine | Standing::Declined))
            .map(|(_, account, _)| account.clone())
            .collect();
        let mut events: Vec<Event> = read
            .into_iter()
            .filter(|(_, account, found)| is_mine(*found, knowing.contains(account)))
            .map(|(event, _, _)| event)
            .collect();
        events.sort_by_key(|event| event.start_ms);
        events
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use anyhow::{bail, Result};

    use super::{Event, Look, MacCalendar};

    pub fn access() -> &'static str {
        "unsupported"
    }

    pub fn request() -> Result<&'static str> {
        Ok("unsupported")
    }

    pub fn calendars() -> Result<Vec<MacCalendar>> {
        bail!("the Calendar app is on macOS only")
    }

    pub fn events(_ids: &[String], _from_ms: i64, _to_ms: i64) -> Result<Vec<Event>> {
        bail!("the Calendar app is on macOS only")
    }

    pub fn look_here(_ids: &[String], _from_ms: i64, _to_ms: i64) -> Result<Look> {
        bail!("the Calendar app is on macOS only")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn calendar(id: &str) -> MacCalendar {
        MacCalendar {
            id: id.to_string(),
            title: format!("캘린더 {id}"),
            account: "someone@example.com".to_string(),
        }
    }

    fn look(ids: &[&str], sources: usize) -> Look {
        Look {
            calendars: ids.iter().map(|id| calendar(id)).collect(),
            sources,
            events: Vec::new(),
        }
    }

    fn chosen(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn a_good_first_look_needs_nothing_more() {
        let refreshed = Cell::new(false);
        let read = read(
            &chosen(&["a"]),
            |refresh| {
                refreshed.set(refreshed.get() || refresh);
                Ok(look(&["a", "b"], 1))
            },
            || panic!("no worker needed"),
        )
        .unwrap();
        assert_eq!(read.calendars.len(), 2);
        assert!(!refreshed.get());
    }

    #[test]
    fn an_empty_answer_is_read_again_after_a_refresh() {
        let read = read(
            &chosen(&["a"]),
            |refresh| {
                Ok(if refresh {
                    look(&["a"], 1)
                } else {
                    look(&[], 0)
                })
            },
            || panic!("the refresh was enough"),
        )
        .unwrap();
        assert_eq!(read.calendars, vec![calendar("a")]);
    }

    #[test]
    fn a_stale_connection_is_read_around_with_a_fresh_one() {
        // As on the day it happened: this process kept giving nothing while
        // the Calendar app had the calendar, and a new process read it.
        let read = read(
            &chosen(&["a"]),
            |_| Ok(look(&[], 0)),
            || {
                let mut fresh = look(&["a"], 1);
                fresh.events = vec![Event {
                    uid: "u".to_string(),
                    title: "상담".to_string(),
                    start_ms: 1,
                    end_ms: 2,
                    location: String::new(),
                    attendees: Vec::new(),
                }];
                Ok(fresh)
            },
        )
        .unwrap();
        assert_eq!(read.events.len(), 1);
    }

    #[test]
    fn nothing_from_any_connection_is_a_stuck_read_not_a_missing_account() {
        let ids = chosen(&["a"]);
        let error = read(&ids, |_| Ok(look(&[], 0)), || Ok(look(&[], 0))).unwrap_err();
        assert_eq!(unread(&error), Some(Unread::Empty));
        // Without a fresh connection it is not known to be empty either.
        let error = read(&[], |_| Ok(look(&[], 0)), || Err(anyhow::anyhow!("no"))).unwrap_err();
        assert_eq!(unread(&error), Some(Unread::Empty));
        // Accounts with no calendar read from them are not "no account".
        let error = read(&[], |_| Ok(look(&[], 2)), || Ok(look(&[], 2))).unwrap_err();
        assert_eq!(unread(&error), Some(Unread::Empty));
    }

    #[test]
    fn only_a_fresh_connection_seeing_no_account_means_there_are_none() {
        let read = read(&[], |_| Ok(look(&[], 0)), || Ok(look(&[], 0))).unwrap();
        assert!(read.calendars.is_empty());
    }

    #[test]
    fn calendars_without_the_chosen_ones_ask_to_choose_again() {
        let ids = chosen(&["gone"]);
        let error = read(&ids, |_| Ok(look(&["a"], 1)), || Ok(look(&["a"], 1))).unwrap_err();
        assert_eq!(unread(&error), Some(Unread::Missing));
        assert_eq!(Unread::Missing.reason(), "mac_missing");
        assert_eq!(Unread::Empty.reason(), "mac_unreadable");
    }
}
