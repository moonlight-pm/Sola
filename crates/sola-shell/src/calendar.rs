//! Calendar panel — the dropdown shown when the menubar clock is clicked.
//!
//! The month on six fixed Sunday-first weeks (the days either side in a
//! faint ink, so the card never changes height), today in an accent
//! circle, and previous / Today / next at the right of the month's name.
//! Type and inks are the stat panels' (`crate::stats::view`): 10 px caps
//! for the weekdays, the theme's UI faces, never a family by name.
//! Rendered inside the shared Menu window (see `menu::view`),
//! right-anchored under the clock. The displayed month is held in
//! `Shell::calendar_month` (always the 1st of the month).

use chrono::{Datelike, Duration, Months, NaiveDate};
use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Color, Element, Length, Padding, Theme};

use crate::app::Msg;
use sola_kit::components::icon::{icon_handle, icon_svg_colored};
use sola_kit::components::style::mix;
use sola_kit::components::{button as kit_btn, popover};
use sola_kit::fonts;

/// Fixed width of the calendar card: seven 32 px days and the pad.
pub const CARD_WIDTH: f32 = 248.0;
/// Live overlay height: the card (about 256) with room to spare.
pub const CARD_HEIGHT: f32 = 280.0;

/// Inside the card, as the stat panels.
const PAD: f32 = 12.0;
/// A week's row.
const ROW_H: f32 = 30.0;
/// Today's circle.
const TODAY_D: f32 = 24.0;
/// How far the days either side are from the card toward the ink.
const FAINT: f32 = 0.38;
/// Weeks on the card, whatever the month.
const WEEKS: usize = 6;

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

/// Six Sunday-start weeks from the Sunday on or before the 1st of `month`:
/// the month, and the days either side to fill the grid.
pub fn month_grid(month: NaiveDate) -> [[NaiveDate; 7]; WEEKS] {
    let first = first_of_month(month);
    let start = first - Duration::days(first.weekday().num_days_from_sunday() as i64);
    std::array::from_fn(|w| std::array::from_fn(|d| start + Duration::days((w * 7 + d) as i64)))
}

fn month_name(month: u32) -> &'static str {
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

/// The card's inks: the theme's text at the stat panels' steps. The days
/// either side are opaque, the design's ink-4: a translucent white reads
/// brighter through iced's linear blending (the kit's hairline note).
struct Inks {
    text: Color,
    figure: Color,
    dim: Color,
    faint: Color,
}

fn inks(theme: &Theme) -> Inks {
    let p = theme.extended_palette();
    let text = p.background.base.text;
    Inks {
        text,
        figure: Color { a: 0.78, ..text },
        dim: Color { a: 0.55, ..text },
        faint: mix(text, p.background.weaker.color, FAINT),
    }
}

fn tint(c: Color) -> impl Fn(&Theme) -> iced::widget::text::Style {
    move |_| iced::widget::text::Style { color: Some(c) }
}

/// A chevron that steps the month.
fn step(icon: &str, msg: Msg, ink: &Inks) -> Element<'static, Msg> {
    button(icon_svg_colored(icon_handle(icon), 14, ink.dim))
        .style(kit_btn::ghost)
        .padding(5)
        .on_press(msg)
        .into()
}

/// The month and year, and previous / Today / next. Today rests dim while
/// the current month shows.
fn header(month: NaiveDate, today: NaiveDate, ink: &Inks) -> Element<'static, Msg> {
    let away = first_of_month(today) != month;
    let today_btn = button(text("Today").font(fonts::chrome()).size(12))
        .style(kit_btn::ghost)
        .padding([3, 7])
        .on_press_maybe(away.then_some(Msg::CalendarToday));
    row![
        text(month_name(month.month()))
            .font(fonts::ui_medium())
            .size(13)
            .style(tint(ink.text)),
        text(month.year().to_string())
            .font(fonts::ui())
            .size(13)
            .style(tint(ink.dim)),
        Space::new().width(Length::Fill),
        step("lucide/chevron-left", Msg::CalendarPrevMonth, ink),
        today_btn,
        step("lucide/chevron-right", Msg::CalendarNextMonth, ink),
    ]
    .spacing(5)
    .padding(Padding::ZERO.left(6))
    .align_y(Alignment::Center)
    .into()
}

/// The weekdays in the panels' caption: 10 px caps, dim.
fn weekdays(ink: &Inks) -> Element<'static, Msg> {
    row(WEEKDAYS.into_iter().map(|d| {
        container(
            text(d.to_uppercase())
                .font(fonts::ui_medium())
                .size(10)
                .style(tint(ink.dim)),
        )
        .center_x(Length::Fill)
        .into()
    }))
    .into()
}

/// A day: the month's in the figure ink, the days either side faint,
/// today medium on an accent circle, a dark label as on a filled button.
fn day(date: NaiveDate, month: NaiveDate, today: NaiveDate, ink: &Inks) -> Element<'static, Msg> {
    let n = text(date.day().to_string()).size(13);
    let inner: Element<'static, Msg> = if date == today {
        container(n.font(fonts::ui_medium()))
            .center(Length::Fixed(TODAY_D))
            .style(|theme: &Theme| {
                let p = theme.extended_palette();
                container::Style {
                    background: Some(p.primary.base.color.into()),
                    // The card's surface: the shell's base is see-through.
                    text_color: Some(p.background.weaker.color),
                    border: iced::Border {
                        radius: (TODAY_D / 2.0).into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .into()
    } else if date.month() == month.month() {
        n.font(fonts::chrome()).style(tint(ink.figure)).into()
    } else {
        n.font(fonts::chrome()).style(tint(ink.faint)).into()
    };
    container(inner)
        .center_x(Length::Fill)
        .center_y(Length::Fixed(ROW_H))
        .into()
}

/// Build the calendar card for `month` (1st of the displayed month) with
/// `today` marked.
pub fn view(month: NaiveDate, today: NaiveDate, theme: &Theme) -> Element<'static, Msg> {
    let ink = inks(theme);
    let grid = column(
        month_grid(month)
            .into_iter()
            .map(|week| row(week.into_iter().map(|d| day(d, month, today, &ink))).into()),
    );
    popover(
        column![
            header(month, today, &ink),
            column![weekdays(&ink), grid].spacing(4),
        ]
        .spacing(10)
        .width(Length::Fill),
    )
    .padding(Padding::new(PAD))
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
    fn june_2026_grid_starts_the_sunday_before() {
        // June 1 2026 is a Monday: the grid opens on Sunday May 31.
        let g = month_grid(ymd(2026, 6, 1));
        assert_eq!(g[0][0], ymd(2026, 5, 31));
        assert_eq!(g[0][1], ymd(2026, 6, 1));
        assert_eq!(g[4][2], ymd(2026, 6, 30));
        assert_eq!(g[5][6], ymd(2026, 7, 11));
    }

    #[test]
    fn a_month_starting_on_sunday_opens_on_its_first() {
        // November 1 2026 is a Sunday.
        assert_eq!(month_grid(ymd(2026, 11, 1))[0][0], ymd(2026, 11, 1));
    }

    #[test]
    fn every_day_of_the_month_appears_once_in_order() {
        for (y, m, days) in [(2026, 6, 30), (2026, 2, 28), (2024, 2, 29), (2026, 10, 31), (2026, 8, 31)] {
            let ours: Vec<u32> = month_grid(ymd(y, m, 1))
                .iter()
                .flatten()
                .filter(|d| d.month() == m)
                .map(|d| d.day())
                .collect();
            assert_eq!(ours, (1..=days).collect::<Vec<_>>(), "{y}-{m}");
        }
    }
}
