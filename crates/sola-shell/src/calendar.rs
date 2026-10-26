//! Calendar panel — the dropdown shown when the menubar clock is clicked.
//!
//! The month on six fixed Sunday-first weeks (the days either side in a
//! faint ink, so the card never changes height), today in an accent
//! circle, and previous / Today / next at the right of the month's name.
//! Type and inks are the stat panels' (`crate::stats::view`): 10 px caps
//! for the weekdays, the theme's UI faces, never a family by name.
//! Under the month, today's events from Calendar's agenda
//! (`Topic::CalendarAgenda`): a row each, its calendar's bar, the time
//! and the title; one that has ended dims; a click shows it in Calendar.
//! Left out until Calendar has published.
//! Rendered inside the shared Menu window (see `menu::view`),
//! right-anchored under the clock. The displayed month is held in
//! `Shell::calendar_month` (always the 1st of the month).

use chrono::{DateTime, Datelike, Duration, Local, Months, NaiveDate};
use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Color, Element, Length, Padding, Theme};

use crate::app::Msg;
use sola_bus::topics::AgendaEvent;
use sola_kit::components::icon::{icon_handle, icon_svg_colored};
use sola_kit::components::style::{HAIRLINE_A, mix, mix_white};
use sola_kit::components::{button as kit_btn, popover};
use sola_kit::fonts;

/// Fixed width of the calendar card: seven 32 px days and the pad.
pub const CARD_WIDTH: f32 = 248.0;
/// The month alone: its card is about 252 tall.
const MONTH_H: f32 = 252.0;
/// Today's section without its rows: the gap, the rule, the caption.
const TODAY_HEAD_H: f32 = 37.0;
/// A row of today's events, with the gap under it.
const EVENT_ROW_H: f32 = 24.0;
/// Today's events listed; past that, a count opens Calendar.
const TODAY_ROWS: usize = 5;
/// The live overlay's room past the card.
const SPARE: f32 = 24.0;

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
    rule: Color,
}

fn inks(theme: &Theme) -> Inks {
    let p = theme.extended_palette();
    let text = p.background.base.text;
    Inks {
        text,
        figure: Color { a: 0.78, ..text },
        dim: Color { a: 0.55, ..text },
        faint: mix(text, p.background.weaker.color, FAINT),
        rule: mix_white(p.background.weaker.color, HAIRLINE_A),
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

/// Live overlay height for a card listing `today` events (`None`: no
/// agenda, the month alone).
pub fn card_height(today: Option<usize>) -> f32 {
    let rows = match today {
        None => return MONTH_H + SPARE,
        Some(0) => 1,
        Some(n) if n > TODAY_ROWS => TODAY_ROWS + 1,
        Some(n) => n,
    };
    MONTH_H + TODAY_HEAD_H + rows as f32 * EVENT_ROW_H + SPARE
}

/// A row of today's: its calendar's bar, the time (or "all day"), the
/// title. An event that has ended dims; one under way is full ink.
fn event_row(e: &AgendaEvent, now: i64, ink: &Inks) -> Element<'static, Msg> {
    let color = sola_kit::theme::try_parse(&e.color).unwrap_or(ink.dim);
    let time = if e.all_day {
        "all day".to_string()
    } else {
        DateTime::from_timestamp(e.start, 0)
            .map(|t| t.with_timezone(&Local).format("%H:%M").to_string())
            .unwrap_or_default()
    };
    let title = match (e.all_day, now) {
        (false, now) if e.end <= now => ink.dim,
        (false, now) if e.start <= now => ink.text,
        _ => ink.figure,
    };
    let bar = container(Space::new())
        .width(Length::Fixed(3.0))
        .height(Length::Fixed(14.0))
        .style(move |_: &Theme| container::Style {
            background: Some(color.into()),
            border: iced::Border {
                radius: 1.5.into(),
                ..Default::default()
            },
            ..Default::default()
        });
    button(
        row![
            bar,
            text(time)
                .font(fonts::MONO)
                .size(11)
                .width(Length::Fixed(44.0))
                .style(tint(ink.dim)),
            text(e.title.clone())
                .font(fonts::chrome())
                .size(12.5)
                .wrapping(iced::widget::text::Wrapping::None)
                .style(tint(title)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .style(kit_btn::ghost)
    .padding([3, 6])
    .width(Length::Fill)
    .clip(true)
    .on_press(Msg::CalendarOpenEvent(e.id.clone()))
    .into()
}

/// Today's events under a rule: "Today" in the caption, how many at the
/// right, a row each (at most [`TODAY_ROWS`], then a count that opens
/// Calendar), or "Nothing today".
fn today_section(events: &[AgendaEvent], now: i64, ink: &Inks) -> Element<'static, Msg> {
    let count = match events.len() {
        0 => String::new(),
        1 => "1 event".to_string(),
        n => format!("{n} events"),
    };
    let head = row![
        text("TODAY")
            .font(fonts::ui_medium())
            .size(10)
            .style(tint(ink.dim)),
        Space::new().width(Length::Fill),
        text(count).font(fonts::MONO).size(11).style(tint(ink.dim)),
    ]
    .padding(Padding::ZERO.left(6).right(6))
    .align_y(Alignment::Center);
    let body: Element<'static, Msg> = if events.is_empty() {
        text("Nothing today")
            .font(fonts::chrome())
            .size(12.5)
            .style(tint(ink.dim))
            .width(Length::Fill)
            .into()
    } else {
        let mut rows = column(events.iter().take(TODAY_ROWS).map(|e| event_row(e, now, ink)));
        if events.len() > TODAY_ROWS {
            rows = rows.push(
                button(
                    text(format!("{} more in Calendar", events.len() - TODAY_ROWS))
                        .font(fonts::chrome())
                        .size(12)
                        .style(tint(ink.dim)),
                )
                .style(kit_btn::ghost)
                .padding([3, 6])
                .width(Length::Fill)
                .on_press(Msg::CalendarOpenEvent(String::new())),
            );
        }
        rows.spacing(2).into()
    };
    let body = container(body).padding(Padding::ZERO.left(if events.is_empty() { 6 } else { 0 }));
    column![rule(ink), column![head, body].spacing(6)]
        .spacing(8)
        .into()
}

fn rule(ink: &Inks) -> Element<'static, Msg> {
    let c = ink.rule;
    container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(move |_: &Theme| container::Style {
            background: Some(c.into()),
            ..Default::default()
        })
        .into()
}

/// Build the calendar card for `month` (1st of the displayed month) with
/// `today` marked, and today's events under it when Calendar has said
/// (`Some`, possibly empty).
pub fn view(
    month: NaiveDate,
    today: NaiveDate,
    now: DateTime<Local>,
    events: Option<&[AgendaEvent]>,
    theme: &Theme,
) -> Element<'static, Msg> {
    let ink = inks(theme);
    let grid = column(
        month_grid(month)
            .into_iter()
            .map(|week| row(week.into_iter().map(|d| day(d, month, today, &ink))).into()),
    );
    let mut parts = column![
        header(month, today, &ink),
        column![weekdays(&ink), grid].spacing(4),
    ]
    .spacing(10)
    .width(Length::Fill);
    if let Some(events) = events {
        parts = parts.push(today_section(events, now.timestamp(), &ink));
    }
    popover(parts)
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
