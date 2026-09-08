//! Calendars, events, and connected accounts.

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use sola_bus::topics::{google_calendar_client_id, CalendarShelf};
use sola_core::Encrypted;

pub const LOCAL_CAL_ID: &str = "local";
pub const LOCAL_CAL_NAME: &str = "On This Computer";

/// Enamel discs for calendars that do not ship a provider color.
pub const PALETTE: [&str; 8] = [
    "#7aa2f7", "#9ece6a", "#e0af68", "#f7768e", "#bb9af7", "#7dcfff", "#cfc9c2", "#3dd6f5",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum View {
    #[default]
    Month,
    Week,
    Day,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CalKind {
    Local,
    Google,
    Apple,
    Url,
    Caldav,
}

impl CalKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "On This Computer",
            Self::Google => "Google",
            Self::Apple => "iCloud",
            Self::Url => "URL",
            Self::Caldav => "CalDAV",
        }
    }

    pub fn wire(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Google => "google",
            Self::Apple => "apple",
            Self::Url => "url",
            Self::Caldav => "caldav",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calendar {
    pub id: String,
    pub name: String,
    pub color: String,
    pub kind: CalKind,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub visible: bool,
    #[serde(default)]
    pub read_only: bool,
    /// Provider calendar id (Google calendarList id, CalDAV href).
    #[serde(default)]
    pub remote_id: Option<String>,
    #[serde(default)]
    pub href: Option<String>,
    /// Local display label. Empty/None uses [`Self::name`].
    #[serde(default)]
    pub alias: Option<String>,
    /// Local colour. Empty/None uses provider [`Self::color`].
    #[serde(default)]
    pub color_override: Option<String>,
    /// Hidden from the Calendar sidebar (events stay off).
    #[serde(default)]
    pub hidden: bool,
}

impl Calendar {
    pub fn local() -> Self {
        Self {
            id: LOCAL_CAL_ID.into(),
            name: LOCAL_CAL_NAME.into(),
            color: PALETTE[0].into(),
            kind: CalKind::Local,
            account_id: None,
            visible: true,
            read_only: false,
            remote_id: None,
            href: None,
            alias: None,
            color_override: None,
            hidden: false,
        }
    }

    pub fn active(&self) -> bool {
        self.visible && !self.hidden
    }

    pub fn display_name(&self) -> &str {
        match self.alias.as_deref().map(str::trim) {
            Some(a) if !a.is_empty() => a,
            _ => self.name.as_str(),
        }
    }

    pub fn display_color(&self) -> &str {
        match self.color_override.as_deref().map(str::trim) {
            Some(c) if !c.is_empty() => c,
            _ => self.color.as_str(),
        }
    }

    pub fn to_shelf(&self) -> CalendarShelf {
        CalendarShelf {
            id: self.id.clone(),
            name: self.name.clone(),
            alias: self.alias.clone(),
            color: self.color.clone(),
            color_override: self.color_override.clone(),
            hidden: self.hidden,
            visible: self.visible,
            kind: self.kind.wire().into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalEvent {
    pub id: String,
    pub calendar_id: String,
    pub title: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub location: String,
    pub all_day: bool,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// Inclusive local date for all-day events.
    #[serde(default)]
    pub start_date: Option<NaiveDate>,
    /// Exclusive local date for all-day events (iCal).
    #[serde(default)]
    pub end_date: Option<NaiveDate>,
    #[serde(default)]
    pub remote_id: Option<String>,
    #[serde(default)]
    pub href: Option<String>,
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub rrule: Option<String>,
    #[serde(default)]
    pub master_id: Option<String>,
}

impl CalEvent {
    pub fn new_local(calendar_id: &str, day: NaiveDate, now: DateTime<Utc>) -> Self {
        let local = now.with_timezone(&chrono::Local);
        let start_naive = if local.date_naive() == day {
            let hour = (local.hour() + 1).min(22);
            day.and_time(NaiveTime::from_hms_opt(hour, 0, 0).unwrap())
        } else {
            day.and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap())
        };
        let start = chrono::Local
            .from_local_datetime(&start_naive)
            .single()
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|| DateTime::<Utc>::from_naive_utc_and_offset(start_naive, Utc));
        Self {
            id: new_id("ev"),
            calendar_id: calendar_id.to_string(),
            title: String::new(),
            notes: String::new(),
            location: String::new(),
            all_day: false,
            start,
            end: start + Duration::hours(1),
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

    pub fn display_title(&self) -> &str {
        let t = self.title.trim();
        if t.is_empty() { "(No title)" } else { t }
    }

    pub fn occurs_on(&self, day: NaiveDate) -> bool {
        if self.all_day {
            let start = self.start_date.unwrap_or_else(|| self.start.date_naive());
            let end = self
                .end_date
                .unwrap_or_else(|| start + Duration::days(1));
            return day >= start && day < end;
        }
        let start = self.start.with_timezone(&chrono::Local).date_naive();
        let end_dt = self.end.with_timezone(&chrono::Local);
        let end = if end_dt.time() == NaiveTime::from_hms_opt(0, 0, 0).unwrap()
            && self.end > self.start
        {
            (end_dt.date_naive() - Duration::days(1)).max(start)
        } else {
            end_dt.date_naive()
        };
        day >= start && day <= end
    }

    pub fn timed_label(&self) -> String {
        if self.all_day {
            return "All day".into();
        }
        let start = self.start.with_timezone(&chrono::Local);
        format_hm(start.hour(), start.minute())
    }
}

pub fn format_hm(hour: u32, minute: u32) -> String {
    let (h12, am) = match hour {
        0 => (12, true),
        1..=11 => (hour, true),
        12 => (12, false),
        _ => (hour - 12, false),
    };
    if minute == 0 {
        format!("{} {}", h12, if am { "AM" } else { "PM" })
    } else {
        format!("{}:{:02} {}", h12, minute, if am { "AM" } else { "PM" })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub kind: CalKind,
    pub label: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub apple_id: String,
    #[serde(default)]
    pub app_password: Option<Encrypted<String>>,
    #[serde(default)]
    pub google_client_id: String,
    #[serde(default)]
    pub access_token: Option<Encrypted<String>>,
    #[serde(default)]
    pub refresh_token: Option<Encrypted<String>>,
    #[serde(default)]
    pub expiry_unix: u64,
    #[serde(default)]
    pub principal_url: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub username: String,
}

impl Account {
    pub fn dav_user(&self) -> &str {
        if !self.username.trim().is_empty() {
            self.username.trim()
        } else {
            self.apple_id.trim()
        }
    }

    pub fn dav_base(&self) -> &str {
        let u = self.url.trim();
        if u.is_empty() {
            "https://caldav.icloud.com/"
        } else {
            u
        }
    }

    pub fn secret(&self) -> String {
        self.app_password
            .as_ref()
            .map(|e| e.0.clone())
            .unwrap_or_default()
    }

    pub fn access(&self) -> String {
        self.access_token
            .as_ref()
            .map(|e| e.0.clone())
            .unwrap_or_default()
    }

    pub fn refresh(&self) -> String {
        self.refresh_token
            .as_ref()
            .map(|e| e.0.clone())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Store {
    #[serde(default)]
    pub calendars: Vec<Calendar>,
    #[serde(default)]
    pub events: Vec<CalEvent>,
    #[serde(default)]
    pub accounts: Vec<Account>,
}

impl Store {
    pub fn ensure_local(&mut self) {
        if !self.calendars.iter().any(|c| c.id == LOCAL_CAL_ID) {
            self.calendars.insert(0, Calendar::local());
        }
    }

    pub fn calendar(&self, id: &str) -> Option<&Calendar> {
        self.calendars.iter().find(|c| c.id == id)
    }

    pub fn visible_events_on<'a>(&'a self, day: NaiveDate) -> Vec<&'a CalEvent> {
        let hidden: std::collections::HashSet<&str> = self
            .calendars
            .iter()
            .filter(|c| !c.active())
            .map(|c| c.id.as_str())
            .collect();
        let mut out: Vec<&CalEvent> = self
            .events
            .iter()
            .filter(|e| !hidden.contains(e.calendar_id.as_str()) && e.occurs_on(day))
            .collect();
        out.sort_by(|a, b| match (a.all_day, b.all_day) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.start.cmp(&b.start).then_with(|| a.title.cmp(&b.title)),
        });
        out
    }

    pub fn writable_calendars(&self) -> Vec<&Calendar> {
        self.calendars
            .iter()
            .filter(|c| !c.read_only && !c.hidden)
            .collect()
    }

    pub fn pick_default(&self, current: &str) -> String {
        if self
            .calendars
            .iter()
            .any(|c| c.id == current && !c.hidden)
        {
            return current.to_string();
        }
        self.calendars
            .iter()
            .find(|c| !c.hidden && !c.read_only)
            .or_else(|| self.calendars.iter().find(|c| !c.hidden))
            .map(|c| c.id.clone())
            .unwrap_or_else(|| LOCAL_CAL_ID.into())
    }

    /// Apply Settings shelf prefs. Empty `shelves` means leave as-is.
    /// Hide also turns the calendar off; re-show turns it back on.
    /// Click-to-toggle `visible` is not overwritten except on that transition.
    pub fn apply_shelf(&mut self, shelves: &[CalendarShelf]) -> bool {
        if shelves.is_empty() {
            return false;
        }
        let mut changed = false;
        for s in shelves {
            let Some(cal) = self.calendars.iter_mut().find(|c| c.id == s.id) else {
                continue;
            };
            let alias = normalize_alias(&s.alias, &cal.name);
            let color_override = normalize_color_override(&s.color_override);
            if cal.alias != alias {
                cal.alias = alias;
                changed = true;
            }
            if cal.color_override != color_override {
                cal.color_override = color_override;
                changed = true;
            }
            if cal.hidden != s.hidden {
                cal.hidden = s.hidden;
                cal.visible = !s.hidden;
                changed = true;
            }
        }
        changed
    }
}

fn normalize_alias(alias: &Option<String>, name: &str) -> Option<String> {
    match alias.as_deref().map(str::trim) {
        Some(a) if !a.is_empty() && a != name => Some(a.to_string()),
        _ => None,
    }
}

fn normalize_color_override(color: &Option<String>) -> Option<String> {
    match color.as_deref().map(str::trim) {
        Some(c) if !c.is_empty() => Some(c.to_string()),
        _ => None,
    }
}

fn default_sidebar_w() -> f32 {
    220.0
}

fn default_inspector_w() -> f32 {
    320.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub view: View,
    #[serde(default)]
    pub default_calendar: String,
    #[serde(default)]
    pub google_client_id: String,
    #[serde(default = "default_sidebar_w")]
    pub sidebar_w: f32,
    #[serde(default = "default_inspector_w")]
    pub inspector_w: f32,
    #[serde(default)]
    pub accounts_collapsed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            view: View::Month,
            default_calendar: LOCAL_CAL_ID.into(),
            google_client_id: google_calendar_client_id(),
            sidebar_w: default_sidebar_w(),
            inspector_w: default_inspector_w(),
            accounts_collapsed: false,
        }
    }
}

pub fn new_id(prefix: &str) -> String {
    use rand::RngCore;
    let mut buf = [0u8; 8];
    rand::rng().fill_bytes(&mut buf);
    let n = u64::from_le_bytes(buf);
    format!("{prefix}-{n:016x}")
}

pub fn parse_hex(s: &str) -> Option<iced::Color> {
    let s = s.trim().trim_start_matches('#');
    let n = u32::from_str_radix(s, 16).ok()?;
    match s.len() {
        6 => Some(iced::Color::from_rgb8(
            ((n >> 16) & 0xff) as u8,
            ((n >> 8) & 0xff) as u8,
            (n & 0xff) as u8,
        )),
        8 => Some(iced::Color::from_rgba8(
            ((n >> 24) & 0xff) as u8,
            ((n >> 16) & 0xff) as u8,
            ((n >> 8) & 0xff) as u8,
            (n & 0xff) as f32 / 255.0,
        )),
        _ => None,
    }
}

pub fn next_palette(existing: &[Calendar]) -> String {
    let used: std::collections::HashSet<&str> =
        existing.iter().map(|c| c.color.as_str()).collect();
    PALETTE
        .iter()
        .find(|c| !used.contains(*c))
        .copied()
        .unwrap_or(PALETTE[existing.len() % PALETTE.len()])
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_day_exclusive_end() {
        let e = CalEvent {
            id: "1".into(),
            calendar_id: LOCAL_CAL_ID.into(),
            title: "Off".into(),
            notes: String::new(),
            location: String::new(),
            all_day: true,
            start: Utc::now(),
            end: Utc::now(),
            start_date: NaiveDate::from_ymd_opt(2026, 9, 8),
            end_date: NaiveDate::from_ymd_opt(2026, 9, 9),
            remote_id: None,
            href: None,
            etag: None,
            read_only: false,
            rrule: None,
            master_id: None,
        };
        assert!(e.occurs_on(NaiveDate::from_ymd_opt(2026, 9, 8).unwrap()));
        assert!(!e.occurs_on(NaiveDate::from_ymd_opt(2026, 9, 9).unwrap()));
    }

    #[test]
    fn hidden_is_inactive_and_shelf_re_show_activates() {
        let mut store = Store {
            calendars: vec![Calendar::local()],
            events: Vec::new(),
            accounts: Vec::new(),
        };
        store.calendars[0].hidden = true;
        store.calendars[0].visible = false;
        assert!(!store.calendars[0].active());
        let day = NaiveDate::from_ymd_opt(2026, 9, 8).unwrap();
        store.events.push(CalEvent::new_local(LOCAL_CAL_ID, day, Utc::now()));
        assert!(store.visible_events_on(day).is_empty());

        let mut shelf = store.calendars[0].to_shelf();
        shelf.hidden = false;
        assert!(store.apply_shelf(&[shelf]));
        assert!(!store.calendars[0].hidden);
        assert!(store.calendars[0].visible);
        assert_eq!(store.visible_events_on(day).len(), 1);
    }
}
