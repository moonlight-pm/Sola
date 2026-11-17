//! Times as the list and the thread show them.

use chrono::{DateTime, Datelike, Local, TimeZone};

fn local(at: u64) -> DateTime<Local> {
    Local.timestamp_millis_opt(at as i64).single().unwrap_or_else(Local::now)
}

/// The time of day: "3:42 PM".
pub fn time(at: u64) -> String {
    local(at).format("%-I:%M %p").to_string()
}

/// How long ago, for the list: the time today, the weekday this week, the
/// date before that.
pub fn brief(at: u64) -> String {
    let (then, now) = (local(at), Local::now());
    let days = now.date_naive().signed_duration_since(then.date_naive()).num_days();
    match days {
        0 => time(at),
        1..=6 => then.format("%a").to_string(),
        _ if then.year() == now.year() => then.format("%b %-d").to_string(),
        _ => then.format("%b %-d, %Y").to_string(),
    }
}

/// The day, for the line between two days of a thread.
pub fn day(at: u64) -> String {
    let (then, now) = (local(at), Local::now());
    match now.date_naive().signed_duration_since(then.date_naive()).num_days() {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        _ if then.year() == now.year() => then.format("%A, %B %-d").to_string(),
        _ => then.format("%B %-d, %Y").to_string(),
    }
}

/// Whether two times fall on the same day here.
pub fn same_day(a: u64, b: u64) -> bool {
    local(a).date_naive() == local(b).date_naive()
}
