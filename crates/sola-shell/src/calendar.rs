//! Calendar panel — the dropdown shown when the menubar clock is clicked.
//!
//! A Sunday-start month grid with today highlighted, plus prev/next month
//! navigation and a "Today" button. Rendered inside the shared Menu window
//! (see `menu::view`), right-anchored under the clock. The displayed month is
//! held in `Shell::calendar_month` (always the 1st of the month).

use chrono::{Datelike, Months, NaiveDate};
use iced::alignment::{Horizontal, Vertical};
use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Padding};

use crate::app::Msg;
use sola_kit::components::{button as kit_btn, popover};
use sola_kit::fonts::INTER_MEDIUM;

/// Fixed width of the calendar card.
pub const CARD_WIDTH: f32 = 248.0;

const WEEKDAYS: [&str; 7] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

/// The 1st of the month containing `date`.
pub fn first_of_month(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), 1).expect("day 1 is always valid")
}

/// The 1st of the month after `month` (which must itself be a 1st).
pub fn next_month(month: NaiveDate) -> NaiveDate {
    month.checked_add_months(Months::new(1)).unwrap_or(month)
}

/// The 1st of the month before `month`.
pub fn prev_month(month: NaiveDate) -> NaiveDate {
    month.checked_sub_months(Months::new(1)).unwrap_or(month)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(28)
}

/// Sunday-start weeks for the month containing `anchor`. Each row is 7 slots;
/// slots before the 1st and after the last day of the month are `None`.
pub fn month_weeks(anchor: NaiveDate) -> Vec<[Option<NaiveDate>; 7]> {
    let (year, month) = (anchor.year(), anchor.month());
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("day 1 valid");
    let lead = first.weekday().num_days_from_sunday() as usize;
    let total = days_in_month(year, month);

    let mut weeks = Vec::new();
    let mut week: [Option<NaiveDate>; 7] = [None; 7];
    let mut col = lead;
    for day in 1..=total {
        week[col] = NaiveDate::from_ymd_opt(year, month, day);
        col += 1;
        if col == 7 {
            weeks.push(week);
            week = [None; 7];
            col = 0;
        }
    }
    if col != 0 {
        weeks.push(week);
    }
    weeks
}

fn month_name(month: u32) -> &'static str {
    [
        "January", "February", "March", "April", "May", "June", "July", "August", "September",
        "October", "November", "December",
    ]
    .get((month.max(1) - 1) as usize)
    .copied()
    .unwrap_or("")
}

/// Lower-contrast text (weekday headers) — a dimmed copy of the base text
/// colour, which is reliably visible on the card (unlike `kit_text::muted`).
fn dim(theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(iced::Color {
            a: 0.5,
            ..theme.palette().text
        }),
    }
}

fn nav_button(glyph: &str, msg: Msg) -> Element<'static, Msg> {
    button(text(glyph.to_string()).size(16))
        .style(kit_btn::ghost)
        .padding([2, 8])
        .on_press(msg)
        .into()
}

fn weekday_cell(label: &'static str) -> Element<'static, Msg> {
    container(text(label).size(11).style(dim))
        .width(Length::Fill)
        .align_x(Horizontal::Center)
        .into()
}

fn day_cell(date: Option<NaiveDate>, is_today: bool) -> Element<'static, Msg> {
    let inner: Element<'static, Msg> = match date {
        Some(d) => text(format!("{}", d.day())).size(13).into(),
        None => text("").into(),
    };
    let cell = container(inner)
        .width(Length::Fill)
        .height(Length::Fixed(26.0))
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center);
    if is_today {
        cell.style(|theme: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(
                theme.extended_palette().primary.base.color,
            )),
            text_color: Some(theme.palette().background),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
    } else {
        cell.into()
    }
}

/// Build the calendar card for `month` (1st of the displayed month) with
/// `today` highlighted.
pub fn view(month: NaiveDate, today: NaiveDate) -> Element<'static, Msg> {
    let header = row![
        nav_button("\u{2039}", Msg::CalendarPrevMonth), // ‹
        container(
            text(format!("{} {}", month_name(month.month()), month.year()))
                .font(INTER_MEDIUM)
                .size(14)
        )
        .width(Length::Fill)
        .align_x(Horizontal::Center),
        nav_button("\u{203A}", Msg::CalendarNextMonth), // ›
        button(text("Today").size(12))
            .style(kit_btn::ghost)
            .padding([2, 8])
            .on_press(Msg::CalendarToday),
    ]
    .align_y(Vertical::Center)
    .spacing(2);

    let weekday_row = row(WEEKDAYS.into_iter().map(weekday_cell).collect::<Vec<_>>());

    let mut rows: Vec<Element<'static, Msg>> = vec![header.into(), weekday_row.into()];
    for week in month_weeks(month) {
        let cells: Vec<Element<'static, Msg>> = week
            .iter()
            .map(|d| day_cell(*d, *d == Some(today)))
            .collect();
        rows.push(row(cells).into());
    }

    let content = column(rows).spacing(2).width(Length::Fill);

    popover(content)
        .padding(Padding::new(8.0))
        .width(Length::Fixed(CARD_WIDTH))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn first_of_month_normalises() {
        assert_eq!(first_of_month(ymd(2026, 6, 16)), ymd(2026, 6, 1));
    }

    #[test]
    fn month_stepping_wraps_year() {
        assert_eq!(next_month(ymd(2026, 12, 1)), ymd(2027, 1, 1));
        assert_eq!(prev_month(ymd(2026, 1, 1)), ymd(2025, 12, 1));
    }

    #[test]
    fn june_2026_grid_layout() {
        // June 1 2026 is a Monday → one leading blank (Sunday), 30 days.
        let weeks = month_weeks(ymd(2026, 6, 1));
        assert_eq!(weeks[0][0], None); // Sunday blank
        assert_eq!(weeks[0][1], Some(ymd(2026, 6, 1))); // Monday = 1st
        // 1 lead + 30 days = 31 slots → 5 weeks (35 slots).
        assert_eq!(weeks.len(), 5);
        assert_eq!(weeks[4][2], Some(ymd(2026, 6, 30))); // last day
        assert_eq!(weeks[4][3], None); // trailing blank
    }

    #[test]
    fn february_leap_and_common() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2026, 12), 31);
    }

    #[test]
    fn every_day_appears_exactly_once() {
        let weeks = month_weeks(ymd(2026, 6, 1));
        let mut days: Vec<u32> = weeks
            .iter()
            .flatten()
            .filter_map(|d| d.map(|d| d.day()))
            .collect();
        days.sort_unstable();
        assert_eq!(days, (1..=30).collect::<Vec<_>>());
    }
}
