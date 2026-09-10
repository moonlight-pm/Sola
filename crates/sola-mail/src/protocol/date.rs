//! Envelope / IMAP INTERNALDATE parsing for sort keys and list labels.

use chrono::{DateTime as ChronoDateTime, Datelike, Local, Timelike};
use mail_parser::DateTime;

/// IMAP envelope Date → unix seconds. `0` if the field is missing or junk.
pub fn date_sort_key(raw: &str) -> i64 {
    parse_mail_datetime(raw)
        .map(|d| d.to_timestamp())
        .unwrap_or(0)
}

/// List column: `4 Sep`. Never includes a zone (`-0700`).
pub fn format_short_date(raw: &str) -> String {
    match parse_mail_datetime(raw) {
        Some(dt) => format!("{} {}", dt.day, month_abbr(dt.month)),
        None => String::new(),
    }
}

/// Letter header date: `4 September 2026` in the local timezone.
pub fn format_letter_date(raw: &str) -> String {
    match local_from_mail(raw) {
        Some(dt) => format!(
            "{} {} {}",
            dt.day(),
            month_name(dt.month() as u8),
            dt.year()
        ),
        None => String::new(),
    }
}

/// Letter header time: `16:00` in the local timezone. Empty when the
/// envelope has no clock (date-only) or does not parse.
pub fn format_letter_time(raw: &str) -> String {
    if !raw.contains(':') {
        return String::new();
    }
    match local_from_mail(raw) {
        Some(dt) => format!("{:02}:{:02}", dt.hour(), dt.minute()),
        None => String::new(),
    }
}

fn local_from_mail(raw: &str) -> Option<ChronoDateTime<Local>> {
    let parsed = parse_mail_datetime(raw)?;
    ChronoDateTime::from_timestamp(parsed.to_timestamp(), 0)
        .map(|utc| utc.with_timezone(&Local))
}

fn parse_mail_datetime(raw: &str) -> Option<DateTime> {
    let t = raw.trim().trim_matches('"').trim();
    if t.is_empty() {
        return None;
    }
    if looks_like_imap_datetime(t) {
        if let Some(dt) = parse_imap_datetime(t).filter(DateTime::is_valid) {
            return Some(dt);
        }
    }
    if let Some(dt) = parse_iso_date(t).filter(DateTime::is_valid) {
        return Some(dt);
    }
    DateTime::parse_rfc822(t).filter(DateTime::is_valid)
}

fn looks_like_imap_datetime(t: &str) -> bool {
    let first = t.split_whitespace().next().unwrap_or("");
    let mut segs = first.split('-');
    let (Some(day), Some(mon), Some(_year), None) =
        (segs.next(), segs.next(), segs.next(), segs.next())
    else {
        return false;
    };
    !day.is_empty() && day.bytes().all(|b| b.is_ascii_digit()) && month_from_abbr(mon).is_some()
}

/// RFC 3501 `date-time`: `4-Sep-2026 16:00:00 -0700` (day may be one digit).
fn parse_imap_datetime(t: &str) -> Option<DateTime> {
    let mut toks = t.split_whitespace();
    let dmy = toks.next()?;
    let mut segs = dmy.split('-');
    let day: u8 = segs.next()?.parse().ok()?;
    let month = month_from_abbr(segs.next()?)?;
    let year: u16 = segs.next()?.parse().ok()?;
    if segs.next().is_some() {
        return None;
    }
    let (hour, minute, second) = match toks.next() {
        Some(hms) => parse_hms(hms)?,
        None => (0, 0, 0),
    };
    let (tz_before_gmt, tz_hour, tz_minute) = match toks.next() {
        Some(z) => parse_zone(z)?,
        None => (false, 0, 0),
    };
    Some(DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
        tz_before_gmt,
        tz_hour,
        tz_minute,
    })
}

fn parse_iso_date(t: &str) -> Option<DateTime> {
    if t.len() < 10 {
        return None;
    }
    let b = t.as_bytes();
    if b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    if !t[..4].bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let year: u16 = t[..4].parse().ok()?;
    let month: u8 = t[5..7].parse().ok()?;
    let day: u8 = t[8..10].parse().ok()?;
    let rest = t.get(10..).unwrap_or("").trim_start_matches('T').trim();
    let (hour, minute, second) = if rest.len() >= 8 && rest.as_bytes().get(2) == Some(&b':') {
        parse_hms(&rest[..8]).unwrap_or((0, 0, 0))
    } else {
        (0, 0, 0)
    };
    Some(DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
        tz_before_gmt: false,
        tz_hour: 0,
        tz_minute: 0,
    })
}

fn parse_hms(s: &str) -> Option<(u8, u8, u8)> {
    let mut p = s.split(':');
    let hour = p.next()?.parse().ok()?;
    let minute = p.next()?.parse().ok()?;
    let second = p.next().unwrap_or("0").parse().ok()?;
    Some((hour, minute, second))
}

fn parse_zone(z: &str) -> Option<(bool, u8, u8)> {
    let z = z.trim();
    let (before, digits) = if let Some(rest) = z.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = z.strip_prefix('+') {
        (false, rest)
    } else {
        return None;
    };
    let digits: String = digits.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 4 {
        return None;
    }
    let hour: u8 = digits[..2].parse().ok()?;
    let minute: u8 = digits[2..4].parse().ok()?;
    Some((before, hour, minute))
}

fn month_from_abbr(s: &str) -> Option<u8> {
    let head: String = s.chars().take(3).collect::<String>().to_ascii_lowercase();
    Some(match head.as_str() {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    })
}

fn month_abbr(mo: u8) -> &'static str {
    match mo {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "???",
    }
}

fn month_name(mo: u8) -> &'static str {
    match mo {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => month_abbr(mo),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc2822_sort_ignores_weekday_prefix() {
        let tue = date_sort_key("Tue, 12 Aug 2026 10:00:00 +0000");
        let wed = date_sort_key("13 Aug 2026 09:00:00 +0000");
        let sun = date_sort_key("Sun, 23 Aug 2026 08:00:00 -0700");
        assert!(sun > wed && wed > tue, "{tue} {wed} {sun}");
    }

    #[test]
    fn imap_internaldate_sorts_and_hides_zone() {
        let aug = date_sort_key("25-Aug-2026 10:00:00 +0000");
        let sep_pt = date_sort_key("04-Sep-2026 16:00:00 -0700");
        let sep_east = date_sort_key("03-Sep-2026 12:00:00 -0500");
        assert!(
            sep_pt > sep_east && sep_east > aug,
            "{aug} {sep_east} {sep_pt}"
        );
        assert_eq!(format_short_date("04-Sep-2026 16:00:00 -0700"), "4 Sep");
        assert_eq!(format_short_date("25-Aug-2026 10:00:00 +0000"), "25 Aug");
        assert!(!format_short_date("04-Sep-2026 16:00:00 -0700").contains("0700"));
        assert_letter_stamp_is_local("04-Sep-2026 16:00:00 -0700");
    }

    #[test]
    fn rfc2822_short_date_drops_year_and_zone() {
        assert_eq!(
            format_short_date("Tue, 25 Aug 2026 10:00:00 +0000"),
            "25 Aug"
        );
        assert_letter_stamp_is_local("Tue, 25 Aug 2026 10:00:00 +0000");
    }

    #[test]
    fn letter_time_empty_without_clock() {
        assert_eq!(format_letter_time("2026-09-04"), "");
        assert_eq!(format_letter_time(""), "");
        assert_eq!(format_letter_time("not a date -0700"), "");
    }

    #[test]
    fn letter_time_honors_offset() {
        let utc = format_letter_time("04-Sep-2026 16:00:00 +0000");
        let west = format_letter_time("04-Sep-2026 16:00:00 -0700");
        assert_ne!(utc, west);
        assert_eq!(
            date_sort_key("04-Sep-2026 16:00:00 -0700")
                - date_sort_key("04-Sep-2026 16:00:00 +0000"),
            7 * 3600
        );
    }

    fn assert_letter_stamp_is_local(raw: &str) {
        let local = local_from_mail(raw).expect(raw);
        assert_eq!(
            format_letter_date(raw),
            format!(
                "{} {} {}",
                local.day(),
                month_name(local.month() as u8),
                local.year()
            )
        );
        let time = format_letter_time(raw);
        assert_eq!(time, format!("{:02}:{:02}", local.hour(), local.minute()));
        assert!(!time.contains("0700"));
        assert!(!time.contains('+'));
    }

    #[test]
    fn iso_date() {
        assert_eq!(format_short_date("2026-09-04T16:00:00Z"), "4 Sep");
        assert!(date_sort_key("2026-09-04") > date_sort_key("2026-08-25"));
    }

    #[test]
    fn junk_is_empty_not_zone_leak() {
        assert_eq!(format_short_date(""), "");
        assert_eq!(format_short_date("not a date -0700"), "");
    }
}
