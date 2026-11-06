//! Times as Oath writes them (`2026-11-06T01:19:14Z`, UTC), and how long
//! ago that was.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Seconds since 1970 for `YYYY-MM-DDTHH:MM:SSZ` (fractions ignored).
pub fn parse(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, se) = (n(11..13)?, n(14..16)?, n(17..19)?);
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se)
}

/// `YYYY-MM-DDTHH:MM:SSZ` for seconds since 1970.
pub fn format(t: i64) -> String {
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, secs / 60 % 60, secs % 60)
}

/// "just now", "12 min ago", "3 h ago", "2 days ago", or the date.
pub fn ago(s: &str) -> String {
    let Some(t) = parse(s) else {
        return s.to_string();
    };
    let d = now() - t;
    match d {
        _ if d < 60 => "just now".into(),
        _ if d < 3600 => format!("{} min ago", d / 60),
        _ if d < 86_400 => format!("{} h ago", d / 3600),
        _ if d < 86_400 * 14 => format!("{} days ago", d / 86_400),
        _ => s.get(0..10).unwrap_or(s).to_string(),
    }
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let t = parse("2026-11-06T01:19:14Z").unwrap();
        assert_eq!(format(t), "2026-11-06T01:19:14Z");
        assert_eq!(parse("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse("2024-02-29T12:00:00.5Z").map(format).as_deref(), Some("2024-02-29T12:00:00Z"));
    }

    #[test]
    fn junk_is_none() {
        assert_eq!(parse("soon"), None);
    }
}
