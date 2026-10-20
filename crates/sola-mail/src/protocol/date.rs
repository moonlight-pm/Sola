//! Local-time labels for message dates (unix seconds in, text out).
//!
//! Parsing lives in mail-parser (Date header) and the imap crate
//! (INTERNALDATE); everything here formats an already-resolved instant
//! in the local timezone.

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};

fn local(ts: i64) -> Option<DateTime<Local>> {
    if ts == 0 {
        return None;
    }
    Local.timestamp_opt(ts, 0).single()
}

/// List column, relative to `now`: `16:04` today, `Yesterday`, a weekday
/// within the last week, `4 Sep` this year, `4 Sep 2024` before that.
pub fn list_stamp(ts: i64, now: DateTime<Local>) -> String {
    let Some(dt) = local(ts) else {
        return String::new();
    };
    let days = now
        .date_naive()
        .signed_duration_since(dt.date_naive())
        .num_days();
    match days {
        0 => format!("{:02}:{:02}", dt.hour(), dt.minute()),
        1 => "Yesterday".into(),
        2..=6 => weekday_abbr(dt.weekday()).into(),
        _ if dt.year() == now.year() => format!("{} {}", dt.day(), month_abbr(dt.month())),
        _ => format!("{} {} {}", dt.day(), month_abbr(dt.month()), dt.year()),
    }
}

/// Letter header date: `4 September 2026`.
pub fn letter_date(ts: i64) -> String {
    local(ts)
        .map(|dt| format!("{} {} {}", dt.day(), month_name(dt.month()), dt.year()))
        .unwrap_or_default()
}

/// Letter header time: `16:00`.
pub fn letter_time(ts: i64) -> String {
    local(ts)
        .map(|dt| format!("{:02}:{:02}", dt.hour(), dt.minute()))
        .unwrap_or_default()
}

/// Reply / forward attribution: `4 Sep 2026 at 16:00`.
pub fn attribution(ts: i64) -> String {
    local(ts)
        .map(|dt| {
            format!(
                "{} {} {} at {:02}:{:02}",
                dt.day(),
                month_abbr(dt.month()),
                dt.year(),
                dt.hour(),
                dt.minute()
            )
        })
        .unwrap_or_default()
}

fn weekday_abbr(w: chrono::Weekday) -> &'static str {
    use chrono::Weekday::*;
    match w {
        Mon => "Mon",
        Tue => "Tue",
        Wed => "Wed",
        Thu => "Thu",
        Fri => "Fri",
        Sat => "Sat",
        Sun => "Sun",
    }
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn month_name(mo: u32) -> &'static str {
    MONTHS
        .get(mo.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("")
}

fn month_abbr(mo: u32) -> &'static str {
    let name = month_name(mo);
    &name[..name.len().min(3)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(y, mo, d, h, mi, 0)
            .single()
            .expect("valid")
    }

    #[test]
    fn list_stamp_steps() {
        let now = at(2026, 9, 10, 18, 0);
        assert_eq!(list_stamp(at(2026, 9, 10, 9, 5).timestamp(), now), "09:05");
        assert_eq!(
            list_stamp(at(2026, 9, 9, 23, 0).timestamp(), now),
            "Yesterday"
        );
        let wed = at(2026, 9, 7, 12, 0);
        assert_eq!(
            list_stamp(wed.timestamp(), now),
            weekday_abbr(wed.weekday())
        );
        assert_eq!(
            list_stamp(at(2026, 8, 25, 10, 0).timestamp(), now),
            "25 Aug"
        );
        assert_eq!(
            list_stamp(at(2024, 3, 4, 10, 0).timestamp(), now),
            "4 Mar 2024"
        );
        assert_eq!(list_stamp(0, now), "");
    }

    #[test]
    fn letter_stamp_is_local() {
        let dt = at(2026, 9, 4, 16, 0);
        assert_eq!(letter_date(dt.timestamp()), "4 September 2026");
        assert_eq!(letter_time(dt.timestamp()), "16:00");
        assert_eq!(attribution(dt.timestamp()), "4 Sep 2026 at 16:00");
        assert_eq!(letter_time(0), "");
    }

    #[test]
    fn sender_offset_does_not_leak() {
        // 16:00 at -0700 is 23:00 UTC; the label must follow the instant,
        // not the sender's wall clock.
        let west = mail_parser::DateTime::parse_rfc822("Fri, 4 Sep 2026 16:00:00 -0700")
            .expect("parse")
            .to_timestamp();
        let utc = mail_parser::DateTime::parse_rfc822("Fri, 4 Sep 2026 23:00:00 +0000")
            .expect("parse")
            .to_timestamp();
        assert_eq!(west, utc);
        assert_eq!(letter_time(west), letter_time(utc));
    }
}
