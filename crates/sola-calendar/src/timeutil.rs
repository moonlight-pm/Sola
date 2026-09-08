//! Sunday-start month/week math (same convention as the menubar clock).

use chrono::{Datelike, Duration, Months, NaiveDate};

pub const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

pub fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

pub fn first_of_month(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), 1).expect("day 1 is always valid")
}

pub fn next_month(month: NaiveDate) -> NaiveDate {
    first_of_month(month)
        .checked_add_months(Months::new(1))
        .unwrap_or(month)
}

pub fn prev_month(month: NaiveDate) -> NaiveDate {
    first_of_month(month)
        .checked_sub_months(Months::new(1))
        .unwrap_or(month)
}

pub fn month_name(month: u32) -> &'static str {
    [
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
    ]
    .get((month.max(1) - 1) as usize)
    .copied()
    .unwrap_or("")
}

pub fn month_title(date: NaiveDate) -> String {
    format!("{} {}", month_name(date.month()), date.year())
}

pub fn weekday_long(date: NaiveDate) -> &'static str {
    [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ][date.weekday().num_days_from_monday() as usize]
}

pub fn day_title(date: NaiveDate) -> String {
    format!(
        "{}, {} {}",
        weekday_long(date),
        month_name(date.month()),
        date.day()
    )
}

pub fn pretty_date(date: NaiveDate) -> String {
    format!("{} {}, {}", month_name(date.month()), date.day(), date.year())
}

pub fn parse_pretty_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(d);
    }
    let (left, year) = s.rsplit_once(',')?;
    let year: i32 = year.trim().parse().ok()?;
    let left = left.trim();
    let (month, day) = left.rsplit_once(' ')?;
    let day: u32 = day.parse().ok()?;
    let month = (1u32..=12).find(|&m| month_name(m).eq_ignore_ascii_case(month))?;
    NaiveDate::from_ymd_opt(year, month, day)
}

pub fn week_title(week: [NaiveDate; 7]) -> String {
    let a = week[0];
    let b = week[6];
    if a.month() == b.month() && a.year() == b.year() {
        format!("{} {} – {}, {}", month_name(a.month()), a.day(), b.day(), a.year())
    } else if a.year() == b.year() {
        format!(
            "{} {} – {} {}, {}",
            month_name(a.month()),
            a.day(),
            month_name(b.month()),
            b.day(),
            a.year()
        )
    } else {
        format!(
            "{} {}, {} – {} {}, {}",
            month_name(a.month()),
            a.day(),
            a.year(),
            month_name(b.month()),
            b.day(),
            b.year()
        )
    }
}

/// Sunday-start week containing `day`.
pub fn week_of(day: NaiveDate) -> [NaiveDate; 7] {
    let lead = day.weekday().num_days_from_sunday() as i64;
    let start = day - Duration::days(lead);
    std::array::from_fn(|i| start + Duration::days(i as i64))
}

pub fn next_week(day: NaiveDate) -> NaiveDate {
    day + Duration::days(7)
}

pub fn prev_week(day: NaiveDate) -> NaiveDate {
    day - Duration::days(7)
}

/// Six Sunday-start weeks covering the month, filled with adjacent-month days.
pub fn month_weeks_filled(anchor: NaiveDate) -> Vec<[NaiveDate; 7]> {
    let first = first_of_month(anchor);
    let start_week = week_of(first);
    let mut weeks = Vec::with_capacity(6);
    let mut cursor = start_week[0];
    for _ in 0..6 {
        weeks.push(std::array::from_fn(|i| cursor + Duration::days(i as i64)));
        cursor += Duration::days(7);
        if cursor.month() != first.month() && cursor.weekday().num_days_from_sunday() == 0 {
            // Keep a 6-week board so the grid height is stable (Calendar.app).
        }
    }
    weeks
}

/// Inclusive date window covering the visible month board (leading/trailing days).
pub fn month_span(anchor: NaiveDate) -> (NaiveDate, NaiveDate) {
    let weeks = month_weeks_filled(anchor);
    let start = weeks[0][0];
    let end = weeks[weeks.len() - 1][6];
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn june_2026_starts_monday() {
        let weeks = month_weeks_filled(ymd(2026, 6, 1));
        assert_eq!(weeks.len(), 6);
        assert_eq!(weeks[0][0], ymd(2026, 5, 31)); // Sunday
        assert_eq!(weeks[0][1], ymd(2026, 6, 1));
        assert_eq!(weeks[4][2], ymd(2026, 6, 30));
    }

    #[test]
    fn week_of_sunday_is_self() {
        let d = ymd(2026, 9, 6); // Sunday
        assert_eq!(week_of(d)[0], d);
        assert_eq!(week_of(ymd(2026, 9, 8))[0], d);
    }
}
