//! Super+K overlay — searchable list of built-in chords.
//!
//! Same modal chrome as the launcher: dim backdrop, raised card, filter
//! field, list_item rows. Group labels are quiet section heads.

use iced::widget::{Id as WidgetId, column, container, mouse_area, row, scrollable, stack, text};
use iced::{Alignment, Element, Length, Padding};

use sola_kit::components::{
    button as kit_btn,
    divider::horizontal_divider,
    modal,
    text_input::{style as input_style, text_input},
};
use sola_kit::fonts;

use crate::app::{Msg, Shell};

pub const QUERY_INPUT_ID: &str = "shortcuts-query";

const QUERY_SIZE: f32 = 16.0;
const QUERY_PAD: f32 = 12.0;
const ROW_LABEL: f32 = 14.0;
const ACCEL_SIZE: f32 = 12.0;
const GROUP_SIZE: f32 = 11.0;
const LIST_SPACING: f32 = 2.0;
const LIST_MAX_H: f32 = 440.0;
const LIST_INSET: f32 = 6.0;
const CARD_WIDTH: f32 = 520.0;

pub fn view(shell: &Shell) -> Element<'_, Msg> {
    if !shell.shortcuts.active {
        return mouse_area(container(text("")).width(Length::Fill).height(Length::Fill))
            .on_press(Msg::CloseShortcuts)
            .into();
    }

    let state = &shell.shortcuts;

    let query_input: Element<'_, Msg> = text_input("Filter shortcuts…", &state.query)
        .id(WidgetId::new(QUERY_INPUT_ID))
        .on_input(Msg::ShortcutsQuery)
        .on_submit(Msg::ShortcutsActivate)
        .style(input_style)
        .padding(Padding::new(QUERY_PAD))
        .size(QUERY_SIZE)
        .width(Length::Fill)
        .into();

    let mut rows: Vec<Element<'_, Msg>> = Vec::new();
    if state.filtered.is_empty() {
        rows.push(
            container(
                text("No matching shortcuts.")
                    .font(fonts::chrome())
                    .size(13)
                    .style(|theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(iced::Color {
                            a: 0.55,
                            ..theme.palette().text
                        }),
                    }),
            )
            .padding(Padding::new(12.0))
            .width(Length::Fill)
            .into(),
        );
    } else {
        let mut last_group = "";
        let lp = shell.style.launcher_pad;
        for (visible_i, &row_i) in state.filtered.iter().enumerate() {
            let Some(entry) = state.rows.get(row_i) else {
                continue;
            };
            if entry.group != last_group {
                last_group = entry.group;
                rows.push(
                    container(
                        text(entry.group)
                            .font(fonts::ui_medium())
                            .size(GROUP_SIZE)
                            .style(|theme: &iced::Theme| iced::widget::text::Style {
                                color: Some(iced::Color {
                                    a: 0.5,
                                    ..theme.palette().text
                                }),
                            }),
                    )
                    .padding(Padding {
                        top: if rows.is_empty() { 4.0 } else { 10.0 },
                        bottom: 4.0,
                        left: lp + 4.0,
                        right: lp + 4.0,
                    })
                    .width(Length::Fill)
                    .into(),
                );
            }

            let is_selected = visible_i == state.selected;
            let label: Element<'_, Msg> =
                text(&entry.label).font(fonts::ui()).size(ROW_LABEL).into();
            let chord: Element<'_, Msg> = if let Some(c) = entry.chord {
                text(c.display())
                    .font(fonts::chrome())
                    .size(ACCEL_SIZE)
                    .style(|theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(iced::Color {
                            a: 0.55,
                            ..theme.palette().text
                        }),
                    })
                    .into()
            } else {
                text("").into()
            };
            let row_content: Element<'_, Msg> =
                row![label, iced::widget::Space::new().width(Length::Fill), chord,]
                    .align_y(Alignment::Center)
                    .width(Length::Fill)
                    .into();

            rows.push(
                iced::widget::button(row_content)
                    .on_press(Msg::ShortcutsPick(visible_i))
                    .padding(Padding {
                        top: lp,
                        bottom: lp,
                        left: lp + 4.0,
                        right: lp + 4.0,
                    })
                    .width(Length::Fill)
                    .style(kit_btn::list_item(is_selected))
                    .into(),
            );
        }
    }

    let list: Element<'_, Msg> = scrollable(column(rows).width(Length::Fill).spacing(LIST_SPACING))
        .width(Length::Fill)
        .height(Length::Fixed(LIST_MAX_H))
        .into();

    let card_body: Element<'_, Msg> = column![
        query_input,
        horizontal_divider(),
        container(list).padding(Padding::new(LIST_INSET)),
    ]
    .spacing(0)
    .width(Length::Fill)
    .into();

    let card: Element<'_, Msg> = modal(card_body).width(Length::Fixed(CARD_WIDTH)).into();

    let positioned: Element<'_, Msg> = container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();

    let dim = shell.style.backdrop_dim;
    let backdrop: Element<'_, Msg> = mouse_area(
        container(text(""))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme: &iced::Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(dim)),
                ..Default::default()
            }),
    )
    .on_press(Msg::CloseShortcuts)
    .into();

    stack![backdrop, positioned]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
