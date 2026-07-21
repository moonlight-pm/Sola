//! Launcher window view.
//!
//! Spotlight-like restraint: dim backdrop, single search field, compact
//! result list. Outside-click on the backdrop fires `Msg::CloseLauncher`.
//!
//! Chord wiring (Meta+Space toggle, Escape close, arrows nav, Enter launch)
//! is handled in `on_chord`. Messages are defined on `Msg`.

use iced::widget::{
    column, container, mouse_area, row, scrollable, stack, text, Id as WidgetId,
};
use iced::{Alignment, Element, Length, Padding};

use sola_kit::components::{
    button as kit_btn, divider::horizontal_divider, icon, modal,
    text_input::{style as input_style, text_input},
};
use sola_kit::fonts;

use crate::app::{Msg, Shell};

/// Stable widget ID for the launcher query input — used to focus it on open.
pub const QUERY_INPUT_ID: &str = "launcher-query";

// ── Density (Spotlight-ish) ───────────────────────────────────────────
const QUERY_SIZE: f32 = 16.0;
const QUERY_PAD: f32 = 12.0;
const ROW_ICON: u16 = 20;
const ROW_LABEL: f32 = 14.0;
const ROW_GAP: f32 = 10.0;
const LIST_SPACING: f32 = 2.0;
const LIST_MAX_H: f32 = 360.0;
const LIST_INSET: f32 = 6.0;

/// Render the launcher overlay for `shell`.
///
/// When inactive: invisible full-screen placeholder (surface stays mapped so
/// composition can show it without a window-create round-trip).
pub fn view(shell: &Shell) -> Element<'_, Msg> {
    if !shell.launcher.active {
        return mouse_area(
            container(text(""))
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Msg::CloseLauncher)
        .into();
    }

    let launcher = &shell.launcher;

    // Search field — part of the card chrome, not a loud form control.
    let query_input: Element<'_, Msg> = text_input("Search…", &launcher.query)
        .id(WidgetId::new(QUERY_INPUT_ID))
        .on_input(Msg::LauncherQuery)
        .style(input_style)
        .padding(Padding::new(QUERY_PAD))
        .size(QUERY_SIZE)
        .width(Length::Fill)
        .into();

    let rows: Vec<Element<'_, Msg>> = if launcher.filtered_ids.is_empty() {
        vec![container(
            text("No matching applications.")
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
        .into()]
    } else {
        launcher
            .filtered_ids
            .iter()
            .enumerate()
            .map(|(i, app_id)| {
                let app = shell.applications.get(app_id);
                let label_str = app.map(|a| a.label.as_str()).unwrap_or(app_id.as_str());
                let icon_name = app.map(|a| a.icon.as_str()).unwrap_or("lucide/box");
                let is_selected = i == launcher.selected;

                let icon_el: Element<'_, Msg> = icon(icon_name, ROW_ICON);
                let label_el: Element<'_, Msg> = text(label_str)
                    .font(fonts::ui())
                    .size(ROW_LABEL)
                    .into();

                let row_content: Element<'_, Msg> = row![icon_el, label_el]
                    .spacing(ROW_GAP)
                    .align_y(Alignment::Center)
                    .into();

                // Quiet selection via list_item (selection atom, not accent).
                let lp = shell.style.launcher_pad;
                iced::widget::button(row_content)
                    .on_press(Msg::Launch)
                    .padding(Padding {
                        top: lp,
                        bottom: lp,
                        left: lp + 4.0,
                        right: lp + 4.0,
                    })
                    .width(Length::Fill)
                    .style(kit_btn::list_item(is_selected))
                    .into()
            })
            .collect()
    };

    let list: Element<'_, Msg> = scrollable(
        column(rows)
            .width(Length::Fill)
            .spacing(LIST_SPACING),
    )
    .width(Length::Fill)
    .height(Length::Fixed(LIST_MAX_H))
    .into();

    // Kit modal: raised panel, calm shadow (Spotlight restraint).
    let card_body: Element<'_, Msg> = column![
        query_input,
        horizontal_divider(),
        container(list).padding(Padding::new(LIST_INSET)),
    ]
    .spacing(0)
    .width(Length::Fill)
    .into();

    let card: Element<'_, Msg> = modal(card_body)
        .width(Length::Fixed(shell.style.launcher_width))
        .into();

    let positioned: Element<'_, Msg> = container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();

    // Backdrop dim from shell-backdrop-dim.
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
    .on_press(Msg::CloseLauncher)
    .into();

    stack![backdrop, positioned]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
