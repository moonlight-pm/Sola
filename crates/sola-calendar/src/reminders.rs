//! Local reminders. Offsets live beside the event store so a provider
//! sync cannot wipe them, and they apply to read-only events too.
//!
//! A due reminder is an [`sola_bus::topics::AppNotification`] whose tag
//! starts with [`sola_bus::topics::NOTIFICATION_PERSIST_PREFIX`]. The
//! shell keeps that desk card up until it is dismissed.

use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::model::{CalEvent, Store, format_hm};

/// Deliver a reminder that came due while Calendar was quit, until this
/// long after the event ends.
const CATCH_UP: Duration = Duration::hours(12);

const ALL_DAY_HOUR: u32 = 9;

/// Timed-event quick sets, minutes before the start.
const TIMED_PRESETS: &[i64] = &[0, 5, 10, 15, 30, 60, 120, 1440, 10_080];

/// All-day quick sets, minutes before 9:00 local on the start date.
const ALL_DAY_PRESETS: &[i64] = &[0, 1440, 2880, 10_080];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    event_id: String,
    calendar_id: String,
    offsets_min: Vec<i64>,
    #[serde(default)]
    fired: Vec<String>,
    anchor: DateTime<Utc>,
    end: DateTime<Utc>,
    #[serde(default)]
    all_day: bool,
    #[serde(default)]
    title: String,
}

#[derive(Debug, Clone)]
pub struct ReminderBook {
    events: Vec<Entry>,
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    pub event_id: String,
    pub offset_min: i64,
    pub fire_unix: i64,
    pub title: String,
    pub body: String,
}

impl Default for ReminderBook {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            path: PathBuf::new(),
        }
    }
}

impl ReminderBook {
    pub fn load(path: &Path) -> Self {
        let events = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<File>(&bytes).ok())
            .map(|file| file.events)
            .unwrap_or_default();
        Self {
            events,
            path: path.to_path_buf(),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(&File {
            events: self.events.clone(),
        })?;
        let tmp = self.path.with_extension("json.tmp");
        {
            let mut f = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    pub fn offsets(&self, event_id: &str) -> Vec<i64> {
        self.events
            .iter()
            .find(|e| e.event_id == event_id)
            .map(|e| e.offsets_min.clone())
            .unwrap_or_default()
    }

    pub fn set_from_event(&mut self, ev: &CalEvent, mut offsets: Vec<i64>) {
        offsets.retain(|m| *m >= 0);
        offsets.sort_unstable();
        offsets.dedup();
        if offsets.is_empty() {
            self.remove(&ev.id);
            return;
        }
        let anchor = anchor(ev);
        let title = ev.display_title().to_string();
        if let Some(entry) = self.events.iter_mut().find(|e| e.event_id == ev.id) {
            entry.calendar_id = ev.calendar_id.clone();
            entry.anchor = anchor;
            entry.end = ev.end;
            entry.all_day = ev.all_day;
            entry.title = title;
            entry
                .fired
                .retain(|key| fired_offset(key).is_some_and(|mins| offsets.contains(&mins)));
            entry.offsets_min = offsets;
        } else {
            self.events.push(Entry {
                event_id: ev.id.clone(),
                calendar_id: ev.calendar_id.clone(),
                offsets_min: offsets,
                fired: Vec::new(),
                anchor,
                end: ev.end,
                all_day: ev.all_day,
                title,
            });
        }
    }

    pub fn remove(&mut self, event_id: &str) {
        self.events.retain(|e| e.event_id != event_id);
    }

    /// Copy start, end, and title off events that are currently loaded
    /// so a later windowed sync still knows when to fire.
    pub fn note_events(&mut self, events: &[CalEvent]) -> bool {
        let mut changed = false;
        for entry in &mut self.events {
            let Some(ev) = events.iter().find(|e| e.id == entry.event_id) else {
                continue;
            };
            let anchor = anchor(ev);
            let title = ev.display_title().to_string();
            if entry.anchor != anchor
                || entry.end != ev.end
                || entry.all_day != ev.all_day
                || entry.title != title
                || entry.calendar_id != ev.calendar_id
            {
                entry.anchor = anchor;
                entry.end = ev.end;
                entry.all_day = ev.all_day;
                entry.title = title;
                entry.calendar_id = ev.calendar_id.clone();
                changed = true;
            }
        }
        changed
    }

    pub fn pending(&self, store: &Store, now: DateTime<Utc>) -> Vec<Due> {
        let mut due = Vec::new();
        for entry in &self.events {
            let live = store.events.iter().find(|e| e.id == entry.event_id);
            let (anchor, end, all_day, title, calendar_id) = if let Some(ev) = live {
                (
                    anchor(ev),
                    ev.end,
                    ev.all_day,
                    ev.display_title().to_string(),
                    ev.calendar_id.as_str(),
                )
            } else {
                (
                    entry.anchor,
                    entry.end,
                    entry.all_day,
                    entry.title.clone(),
                    entry.calendar_id.as_str(),
                )
            };
            if !store.calendar(calendar_id).is_some_and(|c| c.active()) {
                continue;
            }
            let Some(horizon) = end.checked_add_signed(CATCH_UP) else {
                continue;
            };
            if now > horizon {
                continue;
            }
            for offset in &entry.offsets_min {
                let Some(fire) = anchor.checked_sub_signed(Duration::minutes(*offset)) else {
                    continue;
                };
                if fire > now {
                    continue;
                }
                let fire_unix = fire.timestamp();
                let key = fired_key(*offset, fire_unix);
                if entry.fired.iter().any(|k| k == &key) {
                    continue;
                }
                due.push(Due {
                    event_id: entry.event_id.clone(),
                    offset_min: *offset,
                    fire_unix,
                    title: title.clone(),
                    body: reminder_body(all_day, anchor, *offset, now),
                });
            }
        }
        due
    }

    pub fn mark_fired(&mut self, event_id: &str, offset_min: i64, fire_unix: i64) {
        let key = fired_key(offset_min, fire_unix);
        if let Some(entry) = self.events.iter_mut().find(|e| e.event_id == event_id) {
            if !entry.fired.iter().any(|k| k == &key) {
                entry.fired.push(key);
            }
        }
    }
}

pub fn preset_minutes(all_day: bool) -> &'static [i64] {
    if all_day {
        ALL_DAY_PRESETS
    } else {
        TIMED_PRESETS
    }
}

pub fn default_minutes(all_day: bool) -> i64 {
    if all_day { 0 } else { 30 }
}

/// Furthest-before first, which is the order the reminders fire.
pub fn display_order(offsets: &[i64]) -> Vec<i64> {
    let mut v = offsets.to_vec();
    v.sort_unstable_by(|a, b| b.cmp(a));
    v.dedup();
    v
}

pub fn chip_label(minutes: i64, all_day: bool) -> String {
    match (all_day, minutes) {
        (true, 0) => "Morning of".into(),
        (false, 0) => "At time".into(),
        (_, 5) => "5 min".into(),
        (_, 10) => "10 min".into(),
        (_, 15) => "15 min".into(),
        (_, 30) => "30 min".into(),
        (_, 60) => "1 hour".into(),
        (_, 120) => "2 hours".into(),
        (_, 1440) => "1 day".into(),
        (_, 2880) => "2 days".into(),
        (_, 10080) => "1 week".into(),
        _ => offset_label(minutes, all_day),
    }
}

pub fn offset_label(minutes: i64, all_day: bool) -> String {
    if minutes <= 0 {
        return if all_day {
            "Morning of the event".into()
        } else {
            "At time of event".into()
        };
    }
    let (n, unit) = if minutes % 10_080 == 0 {
        let n = minutes / 10_080;
        (n, if n == 1 { "week" } else { "weeks" })
    } else if minutes % 1_440 == 0 {
        let n = minutes / 1_440;
        (n, if n == 1 { "day" } else { "days" })
    } else if minutes % 60 == 0 {
        let n = minutes / 60;
        (n, if n == 1 { "hour" } else { "hours" })
    } else {
        (minutes, if minutes == 1 { "minute" } else { "minutes" })
    };
    format!("{n} {unit} before")
}

/// When the reminder is measured from. Timed events use the start.
/// All-day events use 9:00 local on the start date.
pub fn anchor(ev: &CalEvent) -> DateTime<Utc> {
    if !ev.all_day {
        return ev.start;
    }
    let day = ev
        .start_date
        .unwrap_or_else(|| ev.start.with_timezone(&chrono::Local).date_naive());
    let Some(naive) = day.and_hms_opt(ALL_DAY_HOUR, 0, 0) else {
        return ev.start;
    };
    match chrono::Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(d) | chrono::LocalResult::Ambiguous(d, _) => {
            d.with_timezone(&Utc)
        }
        chrono::LocalResult::None => ev.start,
    }
}

fn reminder_body(all_day: bool, anchor: DateTime<Utc>, offset: i64, now: DateTime<Utc>) -> String {
    format!(
        "{} · {}",
        offset_label(offset, all_day),
        short_when(all_day, anchor, now)
    )
}

fn short_when(all_day: bool, anchor: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let local = anchor.with_timezone(&chrono::Local);
    let now_l = now.with_timezone(&chrono::Local);
    let same_day = local.date_naive() == now_l.date_naive();
    if all_day {
        if same_day {
            "Today".into()
        } else {
            format!("{} {}", month_abbrev(local.month()), local.day())
        }
    } else if same_day {
        format_hm(local.hour(), local.minute())
    } else {
        format!(
            "{} {} · {}",
            month_abbrev(local.month()),
            local.day(),
            format_hm(local.hour(), local.minute())
        )
    }
}

fn month_abbrev(month: u32) -> &'static str {
    [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .get(month.wrapping_sub(1) as usize)
    .copied()
    .unwrap_or("—")
}

fn fired_key(offset_min: i64, fire_unix: i64) -> String {
    format!("{offset_min}:{fire_unix}")
}

fn fired_offset(key: &str) -> Option<i64> {
    key.split_once(':')?.0.parse().ok()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct File {
    events: Vec<Entry>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Calendar, LOCAL_CAL_ID};
    use chrono::TimeZone;

    fn event(start: DateTime<Utc>, end: DateTime<Utc>) -> CalEvent {
        CalEvent {
            id: "ev-1".into(),
            calendar_id: LOCAL_CAL_ID.into(),
            title: "Standup".into(),
            notes: String::new(),
            location: String::new(),
            all_day: false,
            start,
            end,
            start_date: None,
            end_date: None,
            remote_id: None,
            href: None,
            etag: None,
            read_only: false,
            rrule: None,
            master_id: None,
        }
    }

    fn store_with(ev: CalEvent) -> Store {
        Store {
            calendars: vec![Calendar::local()],
            events: vec![ev],
            accounts: Vec::new(),
        }
    }

    #[test]
    fn timed_default_and_labels() {
        assert_eq!(default_minutes(false), 30);
        assert_eq!(default_minutes(true), 0);
        assert_eq!(offset_label(0, false), "At time of event");
        assert_eq!(offset_label(30, false), "30 minutes before");
        assert_eq!(offset_label(60, false), "1 hour before");
        assert_eq!(offset_label(120, false), "2 hours before");
        assert_eq!(offset_label(1440, false), "1 day before");
        assert_eq!(offset_label(10_080, true), "1 week before");
        assert_eq!(chip_label(0, false), "At time");
        assert_eq!(chip_label(0, true), "Morning of");
        assert_eq!(display_order(&[0, 30, 1440]), vec![1440, 30, 0]);
    }

    #[test]
    fn fires_once_when_due_and_skips_hidden() {
        let start = Utc.with_ymd_and_hms(2026, 10, 12, 15, 0, 0).unwrap();
        let end = start + Duration::hours(1);
        let ev = event(start, end);
        let mut book = ReminderBook::default();
        book.set_from_event(&ev, vec![30, 0]);
        let store = store_with(ev.clone());

        let early = start - Duration::minutes(45);
        assert!(book.pending(&store, early).is_empty());

        let at_half_hour = start - Duration::minutes(30);
        let due = book.pending(&store, at_half_hour);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].offset_min, 30);
        assert!(due[0].body.contains("30 minutes before"));

        book.mark_fired(&due[0].event_id, due[0].offset_min, due[0].fire_unix);
        assert!(book.pending(&store, at_half_hour).is_empty());

        let at_start = book.pending(&store, start);
        assert_eq!(at_start.len(), 1);
        assert_eq!(at_start[0].offset_min, 0);

        let mut hidden = store.clone();
        hidden.calendars[0].hidden = true;
        hidden.calendars[0].visible = false;
        assert!(book.pending(&hidden, start).is_empty());
    }

    #[test]
    fn absent_event_uses_snapshot_until_catch_up_ends() {
        let start = Utc.with_ymd_and_hms(2026, 10, 12, 15, 0, 0).unwrap();
        let end = start + Duration::minutes(30);
        let ev = event(start, end);
        let mut book = ReminderBook::default();
        book.set_from_event(&ev, vec![0]);
        let store = Store {
            calendars: vec![Calendar::local()],
            events: Vec::new(),
            accounts: Vec::new(),
        };
        assert_eq!(book.pending(&store, start).len(), 1);
        let late = end + Duration::hours(13);
        assert!(book.pending(&store, late).is_empty());
    }

    #[test]
    fn moved_event_rearms() {
        let start = Utc.with_ymd_and_hms(2026, 10, 12, 15, 0, 0).unwrap();
        let ev = event(start, start + Duration::hours(1));
        let mut book = ReminderBook::default();
        book.set_from_event(&ev, vec![0]);
        let mut store = store_with(ev);
        let due = book.pending(&store, start);
        book.mark_fired(&due[0].event_id, due[0].offset_min, due[0].fire_unix);

        let later = start + Duration::hours(2);
        store.events[0].start = later;
        store.events[0].end = later + Duration::hours(1);
        assert!(book.note_events(&store.events));
        assert_eq!(book.pending(&store, later).len(), 1);
    }

    #[test]
    fn roundtrip_drops_duplicates_and_negative_offsets() {
        let start = Utc.with_ymd_and_hms(2026, 10, 12, 15, 0, 0).unwrap();
        let ev = event(start, start + Duration::hours(1));
        let dir = std::env::temp_dir().join(format!("sola-cal-remind-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("reminders.json");
        let mut book = ReminderBook::load(&path);
        book.set_from_event(&ev, vec![15, 15, -1]);
        book.save().unwrap();
        let loaded = ReminderBook::load(&path);
        assert_eq!(loaded.offsets("ev-1"), vec![15]);
        let _ = fs::remove_dir_all(&dir);
    }
}
