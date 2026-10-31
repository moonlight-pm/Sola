//! The coming days for the menubar clock's panel (`Topic::CalendarAgenda`).
//!
//! Calendar publishes the shown calendars' events from today through the
//! next week whenever they, or the day, change. The topic is persistent,
//! so the panel lists today's events while Calendar is closed.

use chrono::{Duration, NaiveDate};
use sola_bus::topics::{AgendaDay, AgendaEvent, CalendarAgenda, Topic};

use crate::model::Store;

/// Today and the week after it, so the panel has a day to show after
/// midnight with Calendar closed.
const DAYS: i64 = 8;

/// The agenda from `today` on: each day's shown events as Calendar lists
/// them.
pub fn build(store: &Store, today: NaiveDate) -> CalendarAgenda {
    let day = |i: i64| {
        let day = today + Duration::days(i);
        AgendaDay {
            date: day.format("%Y-%m-%d").to_string(),
            events: store
                .visible_events_on(day)
                .into_iter()
                .map(|ev| AgendaEvent {
                    id: ev.id.clone(),
                    title: ev.display_title().to_string(),
                    color: store
                        .calendar(&ev.calendar_id)
                        .map(|c| c.display_color().to_string())
                        .unwrap_or_default(),
                    all_day: ev.all_day,
                    start: ev.start.timestamp(),
                    end: ev.end.timestamp(),
                    location: ev.location.lines().next().unwrap_or("").trim().to_string(),
                })
                .collect(),
        }
    };
    CalendarAgenda {
        days: (0..DAYS).map(day).collect(),
    }
}

/// Sends the agenda when it differs from the last one sent.
#[derive(Default)]
pub struct Publisher {
    last: Option<CalendarAgenda>,
}

impl Publisher {
    pub fn publish(&mut self, store: &Store, today: NaiveDate) {
        let agenda = build(store, today);
        if self.last.as_ref() == Some(&agenda) {
            return;
        }
        let sent = sola_kit::app::bus()
            .lock()
            .map(|mut bus| bus.emit(Topic::CalendarAgenda(agenda.clone())).is_ok())
            .unwrap_or(false);
        if sent {
            tracing::debug!(today = %today, "calendar agenda published");
            self.last = Some(agenda);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CalEvent, Calendar};
    use chrono::{TimeZone, Utc};

    fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn event(id: &str, calendar: &str, start: NaiveDate, end: NaiveDate) -> CalEvent {
        CalEvent {
            id: id.into(),
            calendar_id: calendar.into(),
            title: id.into(),
            notes: String::new(),
            location: "Room 4\nsecond line".into(),
            all_day: true,
            start: Utc.from_utc_datetime(&start.and_hms_opt(0, 0, 0).unwrap()),
            end: Utc.from_utc_datetime(&end.and_hms_opt(0, 0, 0).unwrap()),
            start_date: Some(start),
            end_date: Some(end),
            remote_id: None,
            href: None,
            etag: None,
            read_only: false,
            rrule: None,
            master_id: None,
        }
    }

    fn store(events: Vec<CalEvent>) -> Store {
        let local = Calendar::local();
        let mut off = Calendar::local();
        off.id = "off".into();
        off.visible = false;
        Store {
            calendars: vec![local, off],
            events,
            accounts: Vec::new(),
        }
    }

    #[test]
    fn a_day_at_a_time_from_today() {
        let local = Calendar::local().id;
        let s = store(vec![event("trip", &local, ymd(2026, 10, 26), ymd(2026, 10, 28))]);
        let a = build(&s, ymd(2026, 10, 26));
        assert_eq!(a.days.len(), DAYS as usize);
        assert_eq!(a.days[0].date, "2026-10-26");
        let ids = |i: usize| a.days[i].events.iter().map(|e| e.id.as_str()).collect::<Vec<_>>();
        // Two days long (the end is exclusive), so on the 26th and 27th.
        assert_eq!(ids(0), ["trip"]);
        assert_eq!(ids(1), ["trip"]);
        assert!(ids(2).is_empty());
        assert_eq!(a.days[0].events[0].color, Calendar::local().color);
        assert_eq!(a.days[0].events[0].location, "Room 4");
    }

    #[test]
    fn a_hidden_calendar_stays_off_the_clock() {
        let s = store(vec![event("secret", "off", ymd(2026, 10, 26), ymd(2026, 10, 27))]);
        assert!(build(&s, ymd(2026, 10, 26)).days[0].events.is_empty());
    }
}
