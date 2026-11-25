//! The calendars of macOS's Calendar app, read through EventKit.
//!
//! Someone whose organisation hides the secret iCal address can still add
//! their account to the Calendar app; Seogi then reads it from there. The
//! system asks the user once whether Seogi may read calendars. Nothing is
//! written back, and events are read only for the calendars the user ticked.
//! Events the user declined, or is not invited to, are left out.
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

/// The calendars in the Calendar app.
pub fn calendars() -> Result<Vec<MacCalendar>> {
    imp::calendars()
}

/// The events from `from_ms` to `to_ms` in the calendars with the given
/// ids, each occurrence of a repeating event on its own.
pub fn events(ids: &[String], from_ms: i64, to_ms: i64) -> Result<Vec<Event>> {
    imp::events(ids, from_ms, to_ms)
}

#[cfg(target_os = "macos")]
mod imp {
    use std::collections::HashSet;
    use std::sync::mpsc;
    use std::time::Duration;

    use anyhow::{anyhow, bail, Result};
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
    use super::{Event, MacCalendar};

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
            bail!("Seogi may not read calendars");
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

    pub fn calendars() -> Result<Vec<MacCalendar>> {
        let store = store()?;
        let all = unsafe { store.calendarsForEntityType(EKEntityType::Event) };
        let mut found: Vec<MacCalendar> = all.iter().map(|c| describe(&c)).collect();
        found.sort_by(|a, b| (&a.account, &a.title).cmp(&(&b.account, &b.title)));
        Ok(found)
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

    pub fn events(ids: &[String], from_ms: i64, to_ms: i64) -> Result<Vec<Event>> {
        let store = store()?;
        let chosen: Vec<Retained<EKCalendar>> = unsafe {
            store
                .calendarsForEntityType(EKEntityType::Event)
                .iter()
                .filter(|calendar| ids.contains(&calendar.calendarIdentifier().to_string()))
                .collect()
        };
        if chosen.is_empty() {
            return Err(anyhow!(
                "none of the chosen calendars is in the Calendar app"
            ));
        }
        let calendars = NSArray::from_retained_slice(&chosen);
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
        Ok(events)
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use anyhow::{bail, Result};

    use super::{Event, MacCalendar};

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
}
