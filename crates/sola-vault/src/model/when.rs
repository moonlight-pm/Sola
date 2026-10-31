//! Times as the record and the foot say them: "Edited Oct 30",
//! "Synced 2 min ago", "Last synced 10:42".

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeZone};

/// An RFC 3339 stamp in local time.
pub fn parse(rfc3339: &str) -> Option<NaiveDateTime> {
    DateTime::parse_from_rfc3339(rfc3339.trim())
        .ok()
        .map(|d| d.with_timezone(&Local).naive_local())
}

/// Unix seconds in local time.
pub fn from_unix(secs: u64) -> Option<NaiveDateTime> {
    Local
        .timestamp_opt(i64::try_from(secs).ok()?, 0)
        .single()
        .map(|d| d.naive_local())
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// "today", "yesterday", "Oct 30", or "Mar 2, 2024" in another year.
pub fn day(date: NaiveDate, today: NaiveDate) -> String {
    if date == today {
        return "today".into();
    }
    if today.pred_opt() == Some(date) {
        return "yesterday".into();
    }
    if date.year() == today.year() {
        date.format("%b %-d").to_string()
    } else {
        date.format("%b %-d, %Y").to_string()
    }
}

/// The record's foot: "Edited Oct 30 · Created Mar 2, 2024".
pub fn edited_created(revised: Option<&str>, created: Option<&str>, today: NaiveDate) -> String {
    let mut parts = Vec::new();
    if let Some(d) = revised.and_then(parse) {
        parts.push(format!("Edited {}", day(d.date(), today)));
    }
    if let Some(d) = created.and_then(parse) {
        parts.push(format!("Created {}", day(d.date(), today)));
    }
    parts.join(" · ")
}

/// The foot's sync line, `age` seconds after the sync at `at`.
pub fn synced(age: u64, at: NaiveDateTime, today: NaiveDate) -> String {
    match age {
        0..60 => "Synced just now".into(),
        60..3600 => format!("Synced {} min ago", age / 60),
        _ if at.date() == today => format!("Synced at {}", at.format("%H:%M")),
        _ => format!("Synced {}", day(at.date(), today)),
    }
}

/// When a sync failed: the items are from this time.
pub fn last_synced(at: NaiveDateTime, today: NaiveDate) -> String {
    if at.date() == today {
        at.format("%H:%M").to_string()
    } else {
        day(at.date(), today)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn days_read_short() {
        let today = d(2026, 10, 31);
        assert_eq!(day(d(2026, 10, 31), today), "today");
        assert_eq!(day(d(2026, 10, 30), today), "yesterday");
        assert_eq!(day(d(2026, 10, 3), today), "Oct 3");
        assert_eq!(day(d(2024, 3, 2), today), "Mar 2, 2024");
    }

    #[test]
    fn synced_counts_up_then_names_the_time() {
        let today = d(2026, 10, 31);
        let at = today.and_hms_opt(10, 42, 0).unwrap();
        assert_eq!(synced(5, at, today), "Synced just now");
        assert_eq!(synced(150, at, today), "Synced 2 min ago");
        assert_eq!(synced(7200, at, today), "Synced at 10:42");
        let older = d(2026, 10, 20).and_hms_opt(9, 0, 0).unwrap();
        assert_eq!(synced(900_000, older, today), "Synced Oct 20");
        assert_eq!(last_synced(at, today), "10:42");
    }

    #[test]
    fn the_record_foot_skips_what_it_lacks() {
        let today = d(2026, 10, 31);
        assert_eq!(edited_created(None, None, today), "");
        let s = edited_created(Some("2024-03-02T12:00:00Z"), Some("bad"), today);
        assert!(s.starts_with("Edited Mar"), "{s}");
        assert!(!s.contains("Created"));
    }
}
