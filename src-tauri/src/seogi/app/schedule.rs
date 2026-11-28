//! The calendar: reading it, today's meetings, the reminder before one
//! starts, and which event a recording belongs to.

use super::*;

/// Whether a calendar address works, and what it holds for today.
#[derive(Clone, Debug, Default, Serialize, Deserialize, Type)]
pub struct CalendarCheck {
    pub ok: bool,
    /// Events today.
    pub today: u32,
    pub error: String,
    /// Why it could not be read, as a code the screen has words for (see
    /// [`calendar::trouble`]); empty when it was read.
    pub reason: String,
    /// The address as it should be saved: what was pasted, tidied.
    pub address: String,
    /// The next event from now, within the coming weeks.
    pub next: Option<calendar::Event>,
    /// When it was read (milliseconds since 1970 UTC).
    pub read_at_ms: i64,
}

/// `public_closed` for a public address that answered that there is
/// nothing there: a calendar that is not shared with everyone has none.
/// Otherwise `reason` as it is.
fn public_closed(address: &str, reason: &str) -> String {
    if reason == "refused" && address.contains("/public/") {
        "public_closed".to_string()
    } else {
        reason.to_string()
    }
}

/// How the calendars answered when last read.
#[derive(Clone, Debug, Default, Serialize, Deserialize, Type)]
pub struct CalendarStatus {
    /// The Calendar app's calendars, when some are chosen.
    pub mac: Option<CalendarCheck>,
    /// Each secret address, in the order of the settings.
    pub addresses: Vec<CalendarCheck>,
}

impl CalendarCheck {
    /// What reading `address` gave: its text, or why there is none.
    fn of(address: String, read: Result<&str, String>, now_ms: i64) -> Self {
        use chrono::{Local, TimeZone};
        let text = match read {
            Ok(text) if text.contains("BEGIN:VCALENDAR") => text,
            Ok(_) => {
                return Self {
                    reason: calendar::trouble("not a calendar").to_string(),
                    error: "not a calendar".to_string(),
                    address,
                    read_at_ms: now_ms,
                    ..Self::default()
                }
            }
            Err(error) => {
                return Self {
                    reason: calendar::trouble(&error).to_string(),
                    error,
                    address,
                    read_at_ms: now_ms,
                    ..Self::default()
                }
            }
        };
        let now = Local
            .timestamp_millis_opt(now_ms)
            .single()
            .unwrap_or_else(Local::now);
        let midnight = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .and_then(|start| Local.from_local_datetime(&start).earliest())
            .unwrap_or(now);
        const DAY_MS: i64 = 24 * 3_600_000;
        let start = midnight.timestamp_millis();
        let today = calendar::events_between(text, start, start + DAY_MS - 1);
        let next = calendar::events_between(text, now_ms, now_ms + 60 * DAY_MS)
            .into_iter()
            .filter(|event| event.start_ms >= now_ms)
            .min_by_key(|event| event.start_ms);
        Self {
            ok: true,
            today: today.len() as u32,
            address,
            next,
            read_at_ms: now_ms,
            ..Self::default()
        }
    }
}

/// Today's events (those starting before `midnight`), or, when none of them
/// is wanted, the first wanted event after it.
fn upcoming_from(events: Vec<calendar::Event>, midnight: i64, muted: &[String]) -> Vec<Upcoming> {
    let wanted = |event: &calendar::Event| !muted.contains(&event.uid);
    let today: Vec<Upcoming> = events
        .iter()
        .filter(|event| event.start_ms < midnight)
        .map(|event| Upcoming {
            muted: !wanted(event),
            event: event.clone(),
        })
        .collect();
    if today.iter().any(|upcoming| !upcoming.muted) {
        return today;
    }
    let mut shown = today;
    if let Some(next) = events
        .into_iter()
        .find(|event| event.start_ms >= midnight && wanted(event))
    {
        shown.push(Upcoming {
            muted: false,
            event: next,
        });
    }
    shown
}

#[cfg(test)]
mod upcoming_tests {
    use super::*;

    fn event(uid: &str, start_ms: i64) -> calendar::Event {
        calendar::Event {
            uid: uid.to_string(),
            title: uid.to_string(),
            start_ms,
            end_ms: start_ms + 3_600_000,
            location: String::new(),
            attendees: Vec::new(),
        }
    }

    #[test]
    fn tomorrows_first_event_shows_when_today_has_none_left() {
        let midnight = 1_000_000;
        let shown = upcoming_from(
            vec![
                event("tomorrow", midnight + 10),
                event("later", midnight + 20),
            ],
            midnight,
            &[],
        );
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].event.uid, "tomorrow");
    }

    #[test]
    fn todays_events_are_all_there_is_while_one_is_left() {
        let midnight = 1_000_000;
        let shown = upcoming_from(
            vec![
                event("today", midnight - 10),
                event("tomorrow", midnight + 10),
            ],
            midnight,
            &[],
        );
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].event.uid, "today");
    }

    #[test]
    fn a_muted_event_does_not_hide_the_next_one() {
        let midnight = 1_000_000;
        let shown = upcoming_from(
            vec![
                event("muted", midnight - 10),
                event("tomorrow", midnight + 10),
            ],
            midnight,
            &["muted".to_string()],
        );
        assert_eq!(shown.len(), 2);
        assert!(shown[0].muted);
        assert_eq!(shown[1].event.uid, "tomorrow");
    }
}

impl Secretary {
    // ----- the calendar -----------------------------------------------------

    /// Reads the calendars now and again, and says when a meeting is about
    /// to start.
    pub(super) fn calendar_loop(self: Arc<Self>) {
        use std::sync::atomic::Ordering;
        const REFRESH_EVERY: Duration = Duration::from_secs(15 * 60);
        // What was read last time serves until the calendars answer.
        *lock(&self.calendars) = self.cached_calendars();
        let mut read_at: Option<Instant> = None;
        let mut reminded: std::collections::HashSet<(String, i64)> = Default::default();
        let mut told: Option<(crate::notices::NoticeCode, &'static str)> = None;
        loop {
            let due = read_at.is_none_or(|at| at.elapsed() >= REFRESH_EVERY);
            // macOS can take the Calendar app's permission away (an update
            // it did not recognize as the same app). Asking whether it is
            // back costs nothing, so each turn does, and the calendars are
            // read the moment it is, without restarting Seogi.
            let allowed_again =
                self.calendar_denied() && super::mac_calendar::access() == "granted";
            // Read on a thread of its own: a stuck Calendar app is read again
            // in other ways, which takes seconds, and the reminders below
            // must not wait for that. What was read last serves meanwhile.
            let reading = self.calendar_reading.load(Ordering::Acquire);
            if !reading
                && (self.calendars_stale.swap(false, Ordering::AcqRel) || due || allowed_again)
            {
                self.calendar_reading.store(true, Ordering::Release);
                read_at = Some(Instant::now());
                let me = self.clone();
                std::thread::spawn(move || {
                    // Let go of even if the read panics, or the calendars
                    // would never be read again.
                    let _done = ClearOnDrop(&me.calendar_reading);
                    me.read_calendars();
                });
            }
            let trouble = self.calendar_trouble();
            if trouble != told {
                use crate::notices::{self, Notice, NoticeCode, NoticeSource};
                match trouble {
                    Some((code, detail)) => {
                        notices::raise_without_overlay(
                            &self.app,
                            Notice::new(code, NoticeSource::Calendar)
                                .with_detail((!detail.is_empty()).then(|| detail.to_string())),
                        );
                        // Said once as well: the start reminders stop with
                        // it, and nobody looks at the menu bar for that.
                        if code == NoticeCode::CalendarUnreadable {
                            self.notify(
                                say(
                                    &self.app,
                                    "캘린더를 못 읽어 시작 알림이 안 옵니다",
                                    "The calendar cannot be read, so no start reminders",
                                ),
                                say(
                                    &self.app,
                                    "캘린더 앱의 일정은 그대로입니다. 서기를 다시 켜면 다시 읽습니다.",
                                    "Your events are still in Calendar. Restart Seogi to read them again.",
                                ),
                            );
                        }
                    }
                    None => notices::clear(&self.app, NoticeSource::Calendar),
                }
                told = trouble;
            }
            self.remind(&mut reminded);
            std::thread::sleep(Duration::from_secs(20));
        }
    }

    /// Calendars from the Calendar app were chosen, and macOS refused them
    /// at the last read.
    fn calendar_denied(&self) -> bool {
        lock(&self.calendar_status)
            .mac
            .as_ref()
            .is_some_and(|check| check.reason == "mac_denied")
    }

    /// What is wrong with reading the Calendar app, as a notice and its
    /// detail, from the last read.
    fn calendar_trouble(&self) -> Option<(crate::notices::NoticeCode, &'static str)> {
        use crate::notices::NoticeCode;
        let reason = lock(&self.calendar_status)
            .mac
            .as_ref()
            .map(|check| check.reason.clone())
            .unwrap_or_default();
        match reason.as_str() {
            // Right after a new signature macOS may not list Seogi at all
            // (not_determined): then it is asked, not sent to System Settings.
            "mac_denied" => Some((
                NoticeCode::CalendarPermissionLost,
                if super::mac_calendar::access() == "not_determined" {
                    "not_determined"
                } else {
                    "denied"
                },
            )),
            "mac_unreadable" => Some((NoticeCode::CalendarUnreadable, "")),
            _ => None,
        }
    }

    fn calendar_cache(&self, index: usize) -> PathBuf {
        self.data_dir.join("calendar").join(format!("{index}.ics"))
    }

    fn cached_calendars(&self) -> Vec<String> {
        (0..self.config().calendar_urls.len())
            .filter_map(|index| std::fs::read_to_string(self.calendar_cache(index)).ok())
            .collect()
    }

    /// Reads every calendar the user gave. One that cannot be reached keeps
    /// what was read from it last time.
    pub fn read_calendars(&self) {
        let config = self.config();
        let sources = config.calendar_urls;
        let mut texts: Vec<String> = Vec::new();
        let mut status: Vec<CalendarCheck> = Vec::new();
        let mac = (!config.mac_calendars.is_empty()).then(|| {
            const DAY_MS: i64 = 24 * 3_600_000;
            let now = now_ms();
            match super::mac_calendar::events(
                &config.mac_calendars,
                now - 30 * DAY_MS,
                now + 90 * DAY_MS,
            ) {
                Ok(events) => {
                    let text = calendar::to_ics(&events);
                    let check = CalendarCheck::of(String::new(), Ok(&text), now);
                    texts.push(text);
                    check
                }
                Err(e) => {
                    warn!("The Calendar app could not be read: {e:#}");
                    use super::mac_calendar::{unread, Unread};
                    // Read from what the reading found, not guessed: an
                    // allowed read that came back empty is not a calendar
                    // that was taken out of the Calendar app.
                    let why = unread(&e).unwrap_or(if super::mac_calendar::access() == "granted" {
                        Unread::Empty
                    } else {
                        Unread::Denied
                    });
                    CalendarCheck {
                        reason: why.reason().to_string(),
                        error: format!("{e:#}"),
                        read_at_ms: now,
                        ..CalendarCheck::default()
                    }
                }
            }
        });
        for (index, source) in sources.iter().enumerate() {
            let cache = self.calendar_cache(index);
            let read = calendar::fetch(source);
            // The address is a secret: the status keeps only its place.
            let mut check = CalendarCheck::of(
                String::new(),
                read.as_deref().map_err(|e| format!("{e:#}")),
                now_ms(),
            );
            check.reason = public_closed(source, &check.reason);
            status.push(check);
            match read {
                Ok(text) => {
                    if let Some(dir) = cache.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    if let Err(e) = store::write_atomically(&cache, text.as_bytes()) {
                        warn!("Could not keep a copy of calendar {}: {e:#}", index + 1);
                    }
                    texts.push(text);
                }
                Err(e) => {
                    // The address is a secret and is not part of the message.
                    warn!("Calendar {} could not be read: {e:#}", index + 1);
                    if let Ok(text) = std::fs::read_to_string(&cache) {
                        texts.push(text);
                    }
                }
            }
        }
        let all: Vec<&CalendarCheck> = mac.iter().chain(status.iter()).collect();
        info!(
            "Read {} of {} calendar(s), {} event(s) today",
            all.iter().filter(|check| check.ok).count(),
            all.len(),
            all.iter().map(|check| check.today).sum::<u32>()
        );
        *lock(&self.calendars) = texts;
        *lock(&self.calendar_status) = CalendarStatus {
            mac,
            addresses: status,
        };
    }

    /// How the calendars answered when last read.
    pub fn calendar_status(&self) -> CalendarStatus {
        lock(&self.calendar_status).clone()
    }

    /// Today's calendar events that have not ended, earliest first. When
    /// none is left today, the next one within a week, so the home can say
    /// "내일 10:00 …". The day ends at midnight on this computer's clock.
    pub fn today(&self) -> Vec<Upcoming> {
        use chrono::{Duration as Span, Local, TimeZone};
        let now = Local::now();
        let midnight = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .and_then(|start| Local.from_local_datetime(&start).earliest())
            .map_or(now.timestamp_millis(), |start| {
                (start + Span::days(1)).timestamp_millis()
            });
        let muted = self.config().muted_events;
        let calendars = lock(&self.calendars).clone();
        let week = now.timestamp_millis() + 7 * 24 * 3_600_000;
        upcoming_from(
            calendar::events_of(&calendars, now.timestamp_millis(), week),
            midnight,
            &muted,
        )
    }

    /// Whether a calendar address (or file) can be read, how many events it
    /// has today and which is next.
    pub fn check_calendar(source: &str) -> CalendarCheck {
        let address = calendar::normalize(source);
        let read = calendar::fetch(&address);
        let mut check = CalendarCheck::of(
            address,
            read.as_deref().map_err(|e| format!("{e:#}")),
            now_ms(),
        );
        check.reason = public_closed(&check.address, &check.reason);
        if check.ok {
            info!("A calendar was checked: {} event(s) today", check.today);
        } else {
            // The address is a secret and is not part of the message.
            warn!("A calendar could not be read: {}", check.error);
        }
        check
    }

    /// The calendar's events from now until `hours` from now.
    pub fn upcoming(&self, hours: u32) -> Vec<calendar::Event> {
        let now = now_ms();
        let calendars = lock(&self.calendars).clone();
        calendar::events_of(&calendars, now, now + i64::from(hours) * 3_600_000)
    }

    /// Gives a meeting what its calendar event knows: a title, the kind of
    /// minutes, who was there. What the user set themselves is left alone.
    ///
    /// `chosen` names the event (its id and start) when the user picked one;
    /// otherwise it is the event the recording shares the most time with.
    pub(super) fn apply_calendar(&self, meeting: &mut Meeting, chosen: Option<(&str, i64)>) {
        if meeting.event.is_some() {
            return;
        }
        let calendars = lock(&self.calendars).clone();
        if calendars.is_empty() {
            return;
        }
        const DAY_MS: i64 = 24 * 3_600_000;
        let start = meeting.started_at_ms;
        let end = start + meeting.duration_ms as i64;
        let events = calendar::events_of(&calendars, start - DAY_MS, end + DAY_MS);
        let found = match chosen {
            Some((uid, at)) => events
                .iter()
                .find(|event| event.uid == uid && event.start_ms == at),
            None => calendar::best_match(&events, start, end),
        };
        let Some(event) = found else {
            return;
        };
        let reading = calendar::read_title(&calendar::load_rules(&self.data_dir), &event.title);
        meeting.event = Some(EventRef {
            uid: event.uid.clone(),
            title: event.title.clone(),
            start_ms: event.start_ms,
            end_ms: event.end_ms,
        });
        if meeting.title_is_auto && !reading.title.is_empty() {
            meeting.title = store::safe_name(&reading.title);
            meeting.title_is_auto = false;
        }
        let known = templates::load(&self.data_dir.join("templates"));
        if meeting.template_id.is_empty() && known.iter().any(|t| t.id == reading.template_id) {
            meeting.template_id = reading.template_id;
        }
        if meeting.place.is_empty() {
            meeting.place = if reading.place.is_empty() {
                event.location.clone()
            } else {
                reading.place
            };
        }
        for name in event.attendees.iter().chain(reading.mentions.iter()) {
            if !meeting.people.contains(name) {
                meeting.people.push(name.clone());
            }
        }
        info!("{} matched a calendar event", meeting.id);
    }

    /// Tells the screen, once, about each event that is about to start
    /// while nothing is being recorded.
    fn remind(&self, reminded: &mut std::collections::HashSet<(String, i64)>) {
        let config = self.config();
        if !config.remind || self.is_recording() {
            return;
        }
        let now = now_ms();
        let lead = i64::from(config.remind_minutes_before) * 60_000;
        let calendars = lock(&self.calendars).clone();
        // From `lead` before the start until five minutes into it.
        for event in calendar::events_of(&calendars, now - 5 * 60_000, now + lead) {
            let in_window = event.start_ms - lead <= now && now < event.start_ms + 5 * 60_000;
            if !in_window
                || config.muted_events.contains(&event.uid)
                || !reminded.insert((event.uid.clone(), event.start_ms))
            {
                continue;
            }
            let minutes_until = ((event.start_ms - now) as f64 / 60_000.0).round() as i32;
            let title =
                calendar::read_title(&calendar::load_rules(&self.data_dir), &event.title).title;
            let _ = MeetingStarting {
                event: event.clone(),
                minutes_until,
            }
            .emit(&self.app);
            let _ = minutes_until;
            let _ = title;
            self.show_banner(Banner::Starting { event });
        }
    }

    /// Stops reminders for a calendar event (every occurrence of it).
    pub fn mute_event(&self, uid: &str, muted: bool) -> Result<()> {
        let mut config = self.config();
        config.muted_events.retain(|other| other != uid);
        if muted {
            config.muted_events.push(uid.to_string());
        }
        self.set_config(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_calendar_is_checked_for_today_and_the_next_event() {
        let now = 1_795_500_000_000; // a moment on a working day
        let stamp = |ms: i64| {
            use chrono::TimeZone;
            chrono::Utc
                .timestamp_millis_opt(ms)
                .unwrap()
                .format("%Y%m%dT%H%M%SZ")
                .to_string()
        };
        let text = format!(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:다음 회의\r\nDTSTART:{}\r\nDTEND:{}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
            stamp(now + 3_600_000),
            stamp(now + 7_200_000)
        );
        let check = CalendarCheck::of("https://x/basic.ics".into(), Ok(&text), now);
        assert!(check.ok);
        assert_eq!(
            check.next.map(|event| event.title),
            Some("다음 회의".into())
        );

        let page = CalendarCheck::of(String::new(), Ok("<html>"), now);
        assert!(!page.ok);
        assert_eq!(page.reason, "not_calendar");

        // A public address of a calendar that is not public answers 404.
        let closed = CalendarCheck::of(
            String::new(),
            Err("the calendar answered 404 Not Found".into()),
            now,
        );
        assert_eq!(closed.reason, "refused");
        assert_eq!(
            public_closed(
                "https://calendar.google.com/calendar/ical/me%40example.com/public/basic.ics",
                &closed.reason
            ),
            "public_closed"
        );
        assert_eq!(
            public_closed(
                "https://calendar.google.com/calendar/ical/me%40example.com/private-1/basic.ics",
                &closed.reason
            ),
            "refused"
        );
    }
}
