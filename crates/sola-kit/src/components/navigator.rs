//! A navigator's look: the window an app with a window per thing opens to
//! pick that thing in — a heading with its Esc hint, a field, then rows
//! to pick (an icon, a label, a note at the right) and rows that only say
//! something. Projects picks a project or a session in one, Design a
//! project or a canvas. The rows and what a pick does are the app's.

use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Element, Length, Padding, Theme};

use super::button as kit_btn;
use super::icon::icon;
use super::scroll;
use super::style::{SPACE_MD, SPACE_SM};
use super::text as kit_text;
use crate::fonts;

/// The list's width at most: a list to read, not a page.
const MAX_W: f32 = 560.0;
/// How far a row under another sits in (a session under its folder).
pub const INDENT: f32 = 22.0;
const ROW_ICON: u16 = 16;

/// The window's body: `heading` with `hint` at its right, the `field`,
/// then the `rows`, scrolling.
pub fn page<'a, M: 'a>(
    heading: String,
    hint: &'a str,
    field: impl Into<Element<'a, M, Theme>>,
    rows: Vec<Element<'a, M, Theme>>,
) -> Element<'a, M, Theme> {
    let hint = text(hint).font(fonts::chrome()).size(12).style(kit_text::muted);
    let list = scroll::scrollable(column(rows).spacing(2)).height(Length::Fill);
    let body = column![
        row![
            text(heading).font(fonts::display()).size(20),
            Space::new().width(Length::Fill),
            hint
        ]
        .align_y(Alignment::Center),
        field.into(),
        list,
    ]
    .spacing(SPACE_MD + SPACE_SM)
    .max_width(MAX_W);

    container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding::new(28.0).bottom(12.0))
        .align_x(Alignment::Center)
        .into()
}

/// A pickable row: an icon, a label, and a note at the right; `indent`
/// sets it under the row above.
pub fn pick_row<'a, M: Clone + 'a>(
    on_pick: M,
    selected: bool,
    indent: f32,
    icon_name: &str,
    label: String,
    note: Option<String>,
) -> Element<'a, M, Theme> {
    let mut content = row![icon(icon_name, ROW_ICON), text(label).font(fonts::ui()).size(14)]
        .spacing(10)
        .align_y(Alignment::Center);
    if let Some(note) = note {
        content = content
            .push(Space::new().width(Length::Fill))
            .push(text(note).font(fonts::chrome()).size(12).style(kit_text::muted));
    }
    button(content)
        .on_press(on_pick)
        .padding(Padding {
            top: 8.0,
            bottom: 8.0,
            left: 10.0 + indent,
            right: 12.0,
        })
        .width(Length::Fill)
        .style(kit_btn::list_item(selected))
        .into()
}

/// A row that says something and cannot be picked.
pub fn note_row<'a, M: 'a>(icon_name: &str, note: String) -> Element<'a, M, Theme> {
    container(
        row![
            icon(icon_name, ROW_ICON),
            text(note).font(fonts::chrome()).size(13).style(kit_text::muted),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(Padding::new(10.0))
    .width(Length::Fill)
    .into()
}
