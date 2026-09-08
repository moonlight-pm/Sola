//! Minimal VEVENT parse / emit and RRULE expansion for a date window.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Utc, Weekday};

use crate::model::CalEvent;

pub fn parse_vevents(ics: &str) -> Vec<RawEvent> {
    let unfolded = unfold(ics);
    let mut out = Vec::new();
    let mut in_event = false;
    let mut cur = RawEvent::default();
    for line in unfolded.lines() {
        let line = line.trim_end();
        if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
            in_event = true;
            cur = RawEvent::default();
            continue;
        }
        if line.eq_ignore_ascii_case("END:VEVENT") {
            if in_event && (cur.uid.is_some() || cur.dtstart.is_some()) {
                if cur.uid.is_none() {
                    cur.uid = Some(crate::model::new_id("ics"));
                }
                out.push(std::mem::take(&mut cur));
            }
            in_event = false;
            continue;
        }
        if !in_event {
            continue;
        }
        if let Some((name, params, value)) = split_prop(line) {
            match name.to_ascii_uppercase().as_str() {
                "UID" => cur.uid = Some(unescape(value)),
                "SUMMARY" => cur.summary = unescape(value),
                "DESCRIPTION" => cur.description = unescape(value),
                "LOCATION" => cur.location = unescape(value),
                "RRULE" => cur.rrule = Some(value.to_string()),
                "DTSTART" => cur.dtstart = parse_dt(params, value),
                "DTEND" => cur.dtend = parse_dt(params, value),
                "EXDATE" => {
                    if let Some(dt) = parse_dt(params, value.split(',').next().unwrap_or(value)) {
                        cur.exdates.push(dt.date);
                    }
                    for part in value.split(',').skip(1) {
                        if let Some(dt) = parse_dt(params, part) {
                            cur.exdates.push(dt.date);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

pub fn emit_vevent(event: &CalEvent) -> String {
    let uid = event
        .remote_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&event.id);
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let mut lines = vec![
        "BEGIN:VCALENDAR".into(),
        "VERSION:2.0".into(),
        "PRODID:-//Sola//Calendar//EN".into(),
        "CALSCALE:GREGORIAN".into(),
        "BEGIN:VEVENT".into(),
        format!("UID:{uid}"),
        format!("DTSTAMP:{stamp}"),
    ];
    if event.all_day {
        let start = event
            .start_date
            .unwrap_or_else(|| event.start.with_timezone(&chrono::Local).date_naive());
        let end = event
            .end_date
            .unwrap_or_else(|| start + Duration::days(1));
        lines.push(format!("DTSTART;VALUE=DATE:{}", ymd(start)));
        lines.push(format!("DTEND;VALUE=DATE:{}", ymd(end)));
    } else {
        lines.push(format!(
            "DTSTART:{}",
            event.start.format("%Y%m%dT%H%M%SZ")
        ));
        lines.push(format!("DTEND:{}", event.end.format("%Y%m%dT%H%M%SZ")));
    }
    lines.push(format!("SUMMARY:{}", escape(&event.title)));
    if !event.notes.trim().is_empty() {
        lines.push(format!("DESCRIPTION:{}", escape(&event.notes)));
    }
    if !event.location.trim().is_empty() {
        lines.push(format!("LOCATION:{}", escape(&event.location)));
    }
    lines.push("END:VEVENT".into());
    lines.push("END:VCALENDAR".into());
    lines.join("\r\n") + "\r\n"
}

pub fn to_cal_events(
    raw: RawEvent,
    calendar_id: &str,
    href: Option<String>,
    etag: Option<String>,
    window: (NaiveDate, NaiveDate),
) -> Vec<CalEvent> {
    let Some(start_dt) = raw.dtstart else {
        return Vec::new();
    };
    let end_dt = raw.dtend.unwrap_or_else(|| {
        if start_dt.all_day {
            Dt {
                all_day: true,
                date: start_dt.date + Duration::days(1),
                utc: start_dt.utc + Duration::days(1),
            }
        } else {
            Dt {
                all_day: false,
                date: start_dt.date,
                utc: start_dt.utc + Duration::hours(1),
            }
        }
    });
    let duration = end_dt.utc - start_dt.utc;
    let master_id = raw.uid.clone().unwrap_or_else(|| crate::model::new_id("ics"));
    let dates = if let Some(rrule) = &raw.rrule {
        expand_rrule(start_dt.date, rrule, window, &raw.exdates)
    } else if start_dt.date >= window.0 && start_dt.date <= window.1 {
        vec![start_dt.date]
    } else if start_dt.all_day {
        let mut d = start_dt.date;
        let mut hit = Vec::new();
        while d < end_dt.date && d <= window.1 {
            if d >= window.0 {
                hit.push(d);
            }
            d += Duration::days(1);
        }
        if hit.is_empty() && start_dt.date <= window.1 && end_dt.date > window.0 {
            vec![start_dt.date]
        } else {
            hit.into_iter().take(1).collect()
        }
    } else {
        Vec::new()
    };

    let repeating = raw.rrule.is_some();
    dates
        .into_iter()
        .filter(|d| *d >= window.0 && *d <= window.1)
        .map(|date| {
            let offset = date - start_dt.date;
            let start = start_dt.utc + offset;
            let end = start + duration;
            let id = if repeating {
                format!("{}:{}", master_id, ymd(date))
            } else {
                master_id.clone()
            };
            CalEvent {
                id,
                calendar_id: calendar_id.to_string(),
                title: raw.summary.clone(),
                notes: raw.description.clone(),
                location: raw.location.clone(),
                all_day: start_dt.all_day,
                start,
                end,
                start_date: start_dt.all_day.then_some(date),
                end_date: start_dt.all_day.then_some(date + (end_dt.date - start_dt.date)),
                remote_id: Some(master_id.clone()),
                href: href.clone(),
                etag: etag.clone(),
                read_only: repeating,
                rrule: raw.rrule.clone(),
                master_id: repeating.then(|| master_id.clone()),
            }
        })
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct RawEvent {
    pub uid: Option<String>,
    pub summary: String,
    pub description: String,
    pub location: String,
    pub rrule: Option<String>,
    pub dtstart: Option<Dt>,
    pub dtend: Option<Dt>,
    pub exdates: Vec<NaiveDate>,
}

#[derive(Debug, Clone, Copy)]
pub struct Dt {
    pub all_day: bool,
    pub date: NaiveDate,
    pub utc: DateTime<Utc>,
}

fn unfold(ics: &str) -> String {
    let mut out = String::with_capacity(ics.len());
    for line in ics.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with(' ') || line.starts_with('\t') {
            out.push_str(&line[1..]);
        } else {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(line);
        }
    }
    out
}

fn split_prop(line: &str) -> Option<(&str, &str, &str)> {
    let (left, value) = line.split_once(':')?;
    if let Some((name, params)) = left.split_once(';') {
        Some((name, params, value))
    } else {
        Some((left, "", value))
    }
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(',') => out.push(','),
                Some(';') => out.push(';'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}

fn ymd(d: NaiveDate) -> String {
    format!("{:04}{:02}{:02}", d.year(), d.month(), d.day())
}

fn parse_dt(params: &str, value: &str) -> Option<Dt> {
    let value = value.trim();
    let mut tzid = None;
    let mut value_date = false;
    for p in params.split(';') {
        let p = p.trim();
        if let Some(v) = p.strip_prefix("TZID=") {
            tzid = Some(v.trim_matches('"'));
        } else if p.eq_ignore_ascii_case("VALUE=DATE") {
            value_date = true;
        }
    }
    if value_date || (value.len() == 8 && value.chars().all(|c| c.is_ascii_digit())) {
        let date = parse_ymd(value)?;
        let utc = chrono::Local
            .from_local_datetime(&date.and_time(NaiveTime::MIN))
            .single()
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|| DateTime::<Utc>::from_naive_utc_and_offset(date.and_time(NaiveTime::MIN), Utc));
        return Some(Dt {
            all_day: true,
            date,
            utc,
        });
    }
    let utc = parse_datetime(value, tzid)?;
    Some(Dt {
        all_day: false,
        date: utc.with_timezone(&chrono::Local).date_naive(),
        utc,
    })
}

fn parse_ymd(s: &str) -> Option<NaiveDate> {
    if s.len() < 8 {
        return None;
    }
    let y: i32 = s[0..4].parse().ok()?;
    let m: u32 = s[4..6].parse().ok()?;
    let d: u32 = s[6..8].parse().ok()?;
    NaiveDate::from_ymd_opt(y, m, d)
}

fn parse_datetime(s: &str, tzid: Option<&str>) -> Option<DateTime<Utc>> {
    let compact: String = s.chars().filter(|c| *c != '-' && *c != ':').collect();
    let zulu = compact.ends_with('Z');
    let body = compact.trim_end_matches('Z');
    if body.len() < 15 {
        return None;
    }
    let date = parse_ymd(&body[0..8])?;
    let h: u32 = body[9..11].parse().ok()?;
    let min: u32 = body[11..13].parse().ok()?;
    let sec: u32 = body[13..15].parse().ok()?;
    let naive = date.and_time(NaiveTime::from_hms_opt(h, min, sec)?);
    if zulu {
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    if let Some(tzid) = tzid {
        if let Ok(tz) = tzid.parse::<chrono_tz::Tz>() {
            return tz.from_local_datetime(&naive).single().map(|d| d.with_timezone(&Utc));
        }
    }
    chrono::Local
        .from_local_datetime(&naive)
        .single()
        .map(|d| d.with_timezone(&Utc))
}

fn expand_rrule(
    start: NaiveDate,
    rrule: &str,
    window: (NaiveDate, NaiveDate),
    exdates: &[NaiveDate],
) -> Vec<NaiveDate> {
    let mut freq = String::from("DAILY");
    let mut interval: i64 = 1;
    let mut count: Option<u32> = None;
    let mut until: Option<NaiveDate> = None;
    let mut byday: Vec<Weekday> = Vec::new();
    for part in rrule.split(';') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        match k.trim().to_ascii_uppercase().as_str() {
            "FREQ" => freq = v.trim().to_ascii_uppercase(),
            "INTERVAL" => interval = v.parse().unwrap_or(1).max(1),
            "COUNT" => count = v.parse().ok(),
            "UNTIL" => {
                let digits: String = v.chars().filter(|c| c.is_ascii_digit()).take(8).collect();
                until = parse_ymd(&digits);
            }
            "BYDAY" => {
                byday = v.split(',').filter_map(|d| weekday_token(d.trim())).collect();
            }
            _ => {}
        }
    }
    let until = until.unwrap_or(window.1);
    let mut out = Vec::new();
    let mut n = 0u32;
    match freq.as_str() {
        "WEEKLY" if !byday.is_empty() => {
            let mut cursor = start;
            // Walk days; accept BYDAY matches.
            while cursor <= until && cursor <= window.1 && count.map(|c| n < c).unwrap_or(true) {
                if cursor >= start {
                    let week_index = ((cursor - start).num_days() / 7) as i64;
                    if week_index % interval == 0
                        && byday.contains(&cursor.weekday())
                        && !exdates.contains(&cursor)
                        && cursor >= window.0
                    {
                        out.push(cursor);
                    }
                    if byday.contains(&cursor.weekday()) {
                        n += 1;
                    }
                }
                cursor += Duration::days(1);
                if out.len() > 400 {
                    break;
                }
            }
        }
        _ => {
            let mut cursor = start;
            while cursor <= until && n < count.unwrap_or(u32::MAX) {
                if cursor >= window.0 && cursor <= window.1 && !exdates.contains(&cursor) {
                    out.push(cursor);
                }
                n += 1;
                cursor = match freq.as_str() {
                    "DAILY" => cursor + Duration::days(interval),
                    "WEEKLY" => cursor + Duration::weeks(interval),
                    "MONTHLY" => add_months(cursor, interval as u32),
                    "YEARLY" => add_months(cursor, interval as u32 * 12),
                    _ => cursor + Duration::days(interval),
                };
                if cursor < start || out.len() > 400 {
                    break;
                }
            }
        }
    }
    out
}

fn weekday_token(tok: &str) -> Option<Weekday> {
    let t = tok
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_ascii_uppercase();
    match t.as_str() {
        "MO" => Some(Weekday::Mon),
        "TU" => Some(Weekday::Tue),
        "WE" => Some(Weekday::Wed),
        "TH" => Some(Weekday::Thu),
        "FR" => Some(Weekday::Fri),
        "SA" => Some(Weekday::Sat),
        "SU" => Some(Weekday::Sun),
        _ => None,
    }
}

fn add_months(d: NaiveDate, months: u32) -> NaiveDate {
    d.checked_add_months(chrono::Months::new(months))
        .unwrap_or(d + Duration::days(30 * months as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_day_and_timed() {
        let ics = "BEGIN:VEVENT\r\nUID:a@b\r\nSUMMARY:Off\r\nDTSTART;VALUE=DATE:20260908\r\nDTEND;VALUE=DATE:20260909\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:t@b\r\nSUMMARY:Standup\r\nDTSTART:20260908T150000Z\r\nDTEND:20260908T153000Z\r\nEND:VEVENT\r\n";
        let ev = parse_vevents(ics);
        assert_eq!(ev.len(), 2);
        assert!(ev[0].dtstart.unwrap().all_day);
        assert_eq!(ev[0].summary, "Off");
        assert!(!ev[1].dtstart.unwrap().all_day);
        assert_eq!(ev[1].summary, "Standup");
    }

    #[test]
    fn weekly_byday_expands() {
        let start = NaiveDate::from_ymd_opt(2026, 9, 7).unwrap(); // Monday
        let window = (
            NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 9, 30).unwrap(),
        );
        let dates = expand_rrule(start, "FREQ=WEEKLY;BYDAY=MO,WE", window, &[]);
        assert!(dates.contains(&start));
        assert!(dates.contains(&NaiveDate::from_ymd_opt(2026, 9, 9).unwrap()));
        assert!(dates.contains(&NaiveDate::from_ymd_opt(2026, 9, 14).unwrap()));
        assert!(!dates.contains(&NaiveDate::from_ymd_opt(2026, 9, 8).unwrap()));
    }

    #[test]
    fn emit_roundtrip_title() {
        let e = CalEvent {
            id: "x".into(),
            calendar_id: "local".into(),
            title: "A, B".into(),
            notes: "line1\nline2".into(),
            location: String::new(),
            all_day: true,
            start: Utc::now(),
            end: Utc::now(),
            start_date: NaiveDate::from_ymd_opt(2026, 9, 8),
            end_date: NaiveDate::from_ymd_opt(2026, 9, 9),
            remote_id: Some("x".into()),
            href: None,
            etag: None,
            read_only: false,
            rrule: None,
            master_id: None,
        };
        let ics = emit_vevent(&e);
        let parsed = parse_vevents(&ics);
        assert_eq!(parsed[0].summary, "A, B");
        assert_eq!(parsed[0].description, "line1\nline2");
        assert!(parsed[0].dtstart.unwrap().all_day);
    }
}
