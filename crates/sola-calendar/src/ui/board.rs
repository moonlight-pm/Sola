//! The board: the bar over it and the month, week and day views, drawn as
//! Next's calendar (`design/next/next.js` `calendarBody`): weeks a hairline
//! apart with no boxes, a day's date at its top-left, today on an accent
//! circle, events as chips tinted with their calendar's colour and marked
//! by a bar of it.
//!
//! Colors come from the live theme. A tile's theme clears
//! `background.base` (the frame paints the face), so the face here is
//! `palette().background`.

use chrono::{Datelike, NaiveDate, Timelike};
use iced::font::Weight;
use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, mouse_area, row, text};
use iced::{Alignment, Border, Color, Element, Font, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::icon::{icon_handle, icon_svg_colored};
use sola_kit::components::style::{
    HAIRLINE_A, ON_FILL_DARK, RADIUS_MD, RADIUS_SM, SPACE_LG, SPACE_SM, SPACE_XS, mix, mix_white,
};
use sola_kit::fonts;

use super::{App, Msg};
use crate::model::{CalEvent, Calendar, View, format_hm, parse_hex};
use crate::timeutil::{
    WEEKDAYS, day_title, first_of_month, month_title, month_weeks_filled, today, week_of,
    week_title,
};
use sola_kit::components::scroll;

/// The bar over the board.
pub(super) const BAR_H: f32 = sola_kit::components::style::BAR_H;
/// The weekday captions over the month.
const WEEKDAY_H: f32 = 24.0;
/// A day's date, and today's circle.
const BADGE: f32 = 22.0;
/// An event chip's type.
const CHIP_TEXT: f32 = 11.5;
/// A month day at least this wide shows its events' times.
const TIMES_FROM: f32 = 110.0;
/// The day view's time column.
const TIME_W: f32 = 112.0;
/// A calendar without a colour.
pub(super) const FALLBACK: Color = Color::from_rgb(0.48, 0.64, 0.97);

/// The board's inks: the theme's text at the stat panels' steps. The days
/// of the months either side are opaque (a translucent white reads
/// brighter through iced's linear blending, the kit's hairline note).
pub(super) struct Inks {
    pub text: Color,
    pub figure: Color,
    pub dim: Color,
    pub faint: Color,
    pub rule: Color,
    /// The selected day's lift.
    pub lift: Color,
}

pub(super) fn inks(theme: &Theme) -> Inks {
    let face = theme.palette().background;
    let text = theme.extended_palette().background.base.text;
    Inks {
        text,
        figure: Color { a: 0.82, ..text },
        dim: Color { a: 0.55, ..text },
        faint: mix(text, face, 0.38),
        rule: mix_white(face, HAIRLINE_A),
        lift: mix(text, face, 0.06),
    }
}

pub(super) fn tint(c: Color) -> impl Fn(&Theme) -> text::Style {
    move |_| text::Style { color: Some(c) }
}

/// A caption: 10 px caps, medium (the sidebar's section label).
pub(super) fn caps<'a>(label: &str, ink: Color) -> Element<'a, Msg> {
    text(label.to_uppercase())
        .font(fonts::ui_medium())
        .size(10)
        .style(tint(ink))
        .into()
}

/// A full-width hairline.
pub(super) fn rule<'a>(ink: &Inks) -> Element<'a, Msg> {
    let c = ink.rule;
    container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(move |_| container::Style {
            background: Some(c.into()),
            ..container::Style::default()
        })
        .into()
}

/// The calendar's colour, or the fallback.
pub(super) fn cal_color(cal: Option<&Calendar>) -> Color {
    cal.and_then(|c| parse_hex(c.display_color()))
        .unwrap_or(FALLBACK)
}

/// `9:00 AM – 10:30 AM`, or `All day`.
pub(super) fn time_range(ev: &CalEvent) -> String {
    if ev.all_day {
        return "All day".into();
    }
    let at = |t: chrono::DateTime<chrono::Utc>| {
        let t = t.with_timezone(&chrono::Local);
        format_hm(t.hour(), t.minute())
    };
    format!("{} – {}", at(ev.start), at(ev.end))
}

impl App {
    fn selected_day(&self) -> NaiveDate {
        match &self.pane {
            super::Pane::Day(d) => *d,
            super::Pane::Draft(_) => self.cursor,
        }
    }

    /// The range shown, for the bar and the window's title.
    pub(super) fn range_title(&self) -> String {
        match self.settings.view {
            View::Month => month_title(self.cursor),
            View::Week => week_title(week_of(self.cursor)),
            View::Day => day_title(self.cursor),
        }
    }

    /// The range in semibold at the left; the views, previous / Today /
    /// next and a new event at the right.
    pub(super) fn view_bar(&self) -> Element<'_, Msg> {
        let ink = inks(&self.theme);
        let title = text(self.range_title())
            .font(Font {
                weight: Weight::Semibold,
                ..fonts::ui()
            })
            .size(15)
            .wrapping(Wrapping::None)
            .style(tint(ink.text));
        let nav = row![
            icon_button("lucide/chevron-left", Msg::Prev, ink.dim),
            button(text("Today").font(fonts::chrome()).size(12.5))
                .style(kit_btn::ghost)
                .padding([3, 8])
                .on_press(Msg::Today),
            icon_button("lucide/chevron-right", Msg::Next, ink.dim),
        ]
        .spacing(SPACE_XS)
        .align_y(Alignment::Center);
        let bar = row![
            container(title).width(Length::Fill).clip(true),
            segmented(self.settings.view, &ink),
            nav,
            icon_button("lucide/plus", Msg::NewEvent, ink.figure),
        ]
        .spacing(SPACE_LG)
        .align_y(Alignment::Center);
        column![
            container(bar)
                .height(Length::Fixed(BAR_H))
                .padding(Padding::from([0.0, 14.0]))
                .center_y(Length::Fixed(BAR_H)),
            rule(&ink),
        ]
        .into()
    }

    pub(super) fn view_month(&self) -> Element<'_, Msg> {
        let ink = inks(&self.theme);
        let month = first_of_month(self.cursor).month();
        let today = today();
        let selected = self.selected_day();
        let times = (self.board_w() - 20.0) / 7.0 >= TIMES_FROM;
        let head = row(WEEKDAYS.iter().map(|d| {
            container(caps(&d[..3], ink.dim))
                .width(Length::Fill)
                .padding(Padding::ZERO.left(9))
                .into()
        }))
        .height(Length::Fixed(WEEKDAY_H))
        .align_y(Alignment::Center);
        let mut rows: Vec<Element<'_, Msg>> = vec![head.into()];
        for week in month_weeks_filled(self.cursor) {
            let cells = week.iter().map(|d| {
                self.month_cell(*d, *d == today, *d == selected, d.month() == month, times, &ink)
            });
            rows.push(
                column![rule(&ink), row(cells).spacing(2).height(Length::Fill)]
                    .height(Length::Fill)
                    .into(),
            );
        }
        container(column(rows))
            .padding(Padding {
                top: 2.0,
                right: 10.0,
                bottom: 10.0,
                left: 10.0,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn month_cell(
        &self,
        day: NaiveDate,
        is_today: bool,
        selected: bool,
        in_month: bool,
        times: bool,
        ink: &Inks,
    ) -> Element<'_, Msg> {
        let chips: Vec<Element<'_, Msg>> = self
            .store
            .visible_events_on(day)
            .into_iter()
            .map(|ev| event_chip(ev, self.store.calendar(&ev.calendar_id), times, ink))
            .collect();
        let events: Element<'_, Msg> = if chips.is_empty() {
            Space::new().width(Length::Fill).height(Length::Fill).into()
        } else {
            crate::fit::stack(chips)
        };
        let badge = date_badge(
            day.day(),
            is_today,
            if in_month { ink.figure } else { ink.faint },
        );
        mouse_area(
            container(column![badge, events].spacing(SPACE_XS))
                .padding(Padding::from([4.0, 3.0]))
                .width(Length::Fill)
                .height(Length::Fill)
                .clip(true)
                .style(lifted(selected, ink.lift)),
        )
        .on_press(Msg::SelectDay(day))
        .on_double_click(Msg::NewOn(day))
        .into()
    }

    pub(super) fn view_week(&self) -> Element<'_, Msg> {
        let ink = inks(&self.theme);
        let today = today();
        let selected = self.selected_day();
        let week = week_of(self.cursor);
        let heads = week.iter().map(|d| {
            let wd = WEEKDAYS[d.weekday().num_days_from_sunday() as usize];
            mouse_area(
                container(
                    column![caps(&wd[..3], ink.dim), date_badge(d.day(), *d == today, ink.figure)]
                        .spacing(SPACE_XS)
                        .align_x(Alignment::Center),
                )
                .center_x(Length::Fill)
                .padding(Padding::from([6.0, 0.0])),
            )
            .on_press(Msg::SelectDay(*d))
            .into()
        });
        let cols = week.iter().map(|d| self.week_column(*d, *d == selected, &ink));
        column![
            row(heads).padding(Padding::from([0.0, 10.0])),
            rule(&ink),
            row(cols)
                .spacing(2)
                .padding(Padding::from([6.0, 10.0]))
                .height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn week_column(&self, day: NaiveDate, selected: bool, ink: &Inks) -> Element<'_, Msg> {
        let cards: Vec<Element<'_, Msg>> = self
            .store
            .visible_events_on(day)
            .into_iter()
            .map(|ev| event_card(ev, self.store.calendar(&ev.calendar_id), ink, false))
            .collect();
        mouse_area(
            container(column(cards).spacing(3))
                .padding(3)
                .width(Length::Fill)
                .height(Length::Fill)
                .clip(true)
                .style(lifted(selected, ink.lift)),
        )
        .on_press(Msg::SelectDay(day))
        .on_double_click(Msg::NewOn(day))
        .into()
    }

    pub(super) fn view_day(&self) -> Element<'_, Msg> {
        let ink = inks(&self.theme);
        let day = self.cursor;
        let events = self.store.visible_events_on(day);
        let body: Element<'_, Msg> = if events.is_empty() {
            text("Nothing on this day.")
                .size(13)
                .style(tint(ink.dim))
                .into()
        } else {
            column(events.into_iter().map(|ev| {
                row![
                    text(time_range(ev))
                        .size(12)
                        .width(Length::Fixed(TIME_W))
                        .style(tint(ink.dim)),
                    event_card(ev, self.store.calendar(&ev.calendar_id), &ink, true),
                ]
                .spacing(SPACE_LG)
                .align_y(Alignment::Start)
                .into()
            }))
            .spacing(6)
            .into()
        };
        mouse_area(
            scroll::scrollable(container(body).padding(Padding::from([14.0, 18.0])).width(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_double_click(Msg::NewOn(day))
        .into()
    }
}

/// A day's date; today's on an accent circle with a dark label, as on a
/// filled button.
pub(super) fn date_badge<'a>(day: u32, is_today: bool, ink: Color) -> Element<'a, Msg> {
    let n = text(day.to_string()).font(fonts::ui_medium()).size(12);
    if is_today {
        return container(n)
            .center(Length::Fixed(BADGE))
            .style(|theme: &Theme| container::Style {
                background: Some(theme.extended_palette().primary.base.color.into()),
                text_color: Some(ON_FILL_DARK),
                border: Border {
                    radius: (BADGE / 2.0).into(),
                    ..Border::default()
                },
                ..container::Style::default()
            })
            .into();
    }
    container(n.style(tint(ink)))
        .center(Length::Fixed(BADGE))
        .into()
}

/// The selected day's lift, else nothing.
fn lifted(selected: bool, lift: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: selected.then_some(lift.into()),
        border: Border {
            radius: RADIUS_MD.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// The calendar's colour as a short bar at a chip's left edge.
fn bar<'a>(color: Color, height: f32) -> Element<'a, Msg> {
    container(Space::new())
        .width(Length::Fixed(3.0))
        .height(Length::Fixed(height))
        .style(move |_| container::Style {
            background: Some(color.into()),
            border: Border {
                radius: 1.5.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// A chip's fill: its calendar's colour a fifth of the way from the face,
/// opaque (as [`Inks`]); stronger under the pointer.
fn chip_style(color: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status| {
        let amount = match status {
            button::Status::Hovered | button::Status::Pressed => 0.32,
            _ => 0.2,
        };
        button::Style {
            background: Some(mix(color, theme.palette().background, amount).into()),
            text_color: theme.extended_palette().background.base.text,
            border: Border {
                radius: RADIUS_SM.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// A month cell's event: one line, the time (dim, when the day has room)
/// before the title.
fn event_chip<'a>(
    ev: &'a CalEvent,
    cal: Option<&'a Calendar>,
    times: bool,
    ink: &Inks,
) -> Element<'a, Msg> {
    let color = cal_color(cal);
    let mut line = row![bar(color, 11.0)]
        .spacing(5)
        .align_y(Alignment::Center);
    if times && !ev.all_day {
        line = line.push(
            text(ev.timed_label())
                .size(CHIP_TEXT)
                .wrapping(Wrapping::None)
                .style(tint(ink.dim)),
        );
    }
    line = line.push(
        text(ev.display_title())
            .size(CHIP_TEXT)
            .wrapping(Wrapping::None)
            .style(tint(ink.text)),
    );
    button(line)
        .padding(Padding::from([2.0, 4.0]))
        .width(Length::Fill)
        .clip(true)
        .style(chip_style(color))
        .on_press(Msg::OpenEvent(ev.id.clone()))
        .into()
}

/// A week or day event: the title over its time (and, wide, the place).
fn event_card<'a>(
    ev: &'a CalEvent,
    cal: Option<&'a Calendar>,
    ink: &Inks,
    wide: bool,
) -> Element<'a, Msg> {
    let color = cal_color(cal);
    let mut lines = column![
        text(ev.display_title())
            .size(12.5)
            .wrapping(Wrapping::WordOrGlyph)
            .style(tint(ink.text))
    ]
    .spacing(1)
    .width(Length::Fill);
    if !wide {
        lines = lines.push(text(ev.timed_label()).size(11).style(tint(ink.dim)));
    }
    let place = ev.location.lines().next().unwrap_or("").trim();
    if wide && !place.is_empty() {
        lines = lines.push(
            text(place.to_string())
                .size(11.5)
                .wrapping(Wrapping::WordOrGlyph)
                .style(tint(ink.dim)),
        );
    }
    button(
        row![bar(color, if wide { 16.0 } else { 26.0 }), lines]
            .spacing(7)
            .align_y(Alignment::Start),
    )
    .padding(Padding::from([5.0, SPACE_SM + 2.0]))
    .width(Length::Fill)
    .style(chip_style(color))
    .on_press(Msg::OpenEvent(ev.id.clone()))
    .into()
}

/// A ghost button showing a Lucide glyph.
fn icon_button<'a>(icon: &str, msg: Msg, ink: Color) -> Element<'a, Msg> {
    button(icon_svg_colored(icon_handle(icon), 15, ink))
        .style(kit_btn::ghost)
        .padding(5)
        .on_press(msg)
        .into()
}

/// Month / Week / Day on one track; the shown one lifted.
fn segmented<'a>(current: View, ink: &Inks) -> Element<'a, Msg> {
    let track = Color { a: 0.06, ..ink.text };
    let segs = [("Month", View::Month), ("Week", View::Week), ("Day", View::Day)]
        .into_iter()
        .map(|(label, view)| {
            let on = view == current;
            button(text(label).font(fonts::chrome()).size(12))
                .padding(Padding::from([3.0, 10.0]))
                .style(move |theme: &Theme, status| segment_style(theme, status, on))
                .on_press(Msg::SetView(view))
                .into()
        });
    container(row(segs).spacing(SPACE_XS))
        .padding(2)
        .style(move |_| container::Style {
            background: Some(track.into()),
            border: Border {
                radius: RADIUS_MD.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

fn segment_style(theme: &Theme, status: button::Status, on: bool) -> button::Style {
    let text = theme.extended_palette().background.base.text;
    let hover = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let fill = match (on, hover) {
        (true, _) => Some(Color { a: 0.13, ..text }.into()),
        (false, true) => Some(Color { a: 0.06, ..text }.into()),
        (false, false) => None,
    };
    button::Style {
        background: fill,
        text_color: if on || hover {
            text
        } else {
            Color { a: 0.6, ..text }
        },
        border: Border {
            radius: RADIUS_SM.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}
