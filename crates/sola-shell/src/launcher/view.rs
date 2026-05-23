//! Launcher window view.
//!
//! Renders a full-overlay transparent backdrop with a centered search card.
//! Outside-click on the backdrop fires `Msg::CloseLauncher`.  The search
//! card contains a text input (query), a rule, and a scrollable list of
//! filtered applications.
//!
//! Chord wiring (Meta+Space toggle, Escape close, arrows nav, Enter launch)
//! is handled in `on_chord` — Task 10.  The messages are defined here and
//! in `Msg` so Task 10 can route to them.

use iced::widget::{
    column, container, mouse_area, row, rule, scrollable, stack, text, text_input,
    Id as WidgetId,
};
use iced::{Alignment, Element, Length, Padding};

use sola_kit::components::{icon, text_input::style as input_style};

use crate::app::{Msg, Shell};

/// Stable widget ID for the launcher query input — used to focus it on open.
pub const QUERY_INPUT_ID: &str = "launcher-query";

/// Render the launcher overlay for `shell`.
///
/// When the launcher is not active this returns an invisible full-screen
/// placeholder (same pattern as the menu overlay).  The surface stays open
/// so that composition can make it visible without a window-create round-trip.
pub fn view(shell: &Shell) -> Element<'_, Msg> {
    if !shell.launcher.active {
        // Invisible placeholder — keeps iced from getting an empty view.
        return mouse_area(
            container(text(""))
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Msg::CloseLauncher)
        .into();
    }

    let launcher = &shell.launcher;

    // --- query text input ---
    let query_input: Element<'_, Msg> = text_input("Search applications…", &launcher.query)
        .id(WidgetId::new(QUERY_INPUT_ID))
        .on_input(Msg::LauncherQuery)
        .style(input_style)
        .padding(Padding::new(8.0))
        .size(16)
        .width(Length::Fill)
        .into();

    // --- application rows ---
    let rows: Vec<Element<'_, Msg>> = if launcher.filtered_ids.is_empty() {
        vec![container(text("No matching applications."))
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

                let icon_el: Element<'_, Msg> = icon(icon_name, 16);
                let label_el: Element<'_, Msg> = text(label_str).size(14).into();

                let row_content: Element<'_, Msg> = row![icon_el, label_el]
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .into();

                let app_id_clone = app_id.clone();
                let row_btn = iced::widget::button(row_content)
                    .on_press(Msg::Launch)
                    .padding(Padding {
                        top: 6.0,
                        bottom: 6.0,
                        left: 12.0,
                        right: 12.0,
                    })
                    .width(Length::Fill)
                    .style(move |theme: &iced::Theme, status| {
                        let p = theme.extended_palette();
                        if is_selected {
                            iced::widget::button::Style {
                                background: Some(iced::Background::Color(
                                    p.primary.weak.color,
                                )),
                                text_color: p.primary.weak.text,
                                border: iced::Border {
                                    radius: 4.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                        } else {
                            match status {
                                iced::widget::button::Status::Hovered => {
                                    iced::widget::button::Style {
                                        background: Some(iced::Background::Color(
                                            p.background.weak.color,
                                        )),
                                        text_color: p.background.base.text,
                                        border: iced::Border {
                                            radius: 4.0.into(),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    }
                                }
                                _ => iced::widget::button::Style {
                                    background: None,
                                    text_color: p.background.base.text,
                                    border: iced::Border {
                                        radius: 4.0.into(),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                },
                            }
                        }
                    });

                // Suppress unused variable warning; app_id_clone used for
                // future on_click-with-index routing (Task 10 refines selection
                // from click index; for now Enter/click both fire Msg::Launch).
                let _ = app_id_clone;
                row_btn.into()
            })
            .collect()
    };

    let list: Element<'_, Msg> =
        scrollable(column(rows).width(Length::Fill).spacing(2))
            .width(Length::Fill)
            .height(Length::Fixed(320.0))
            .into();

    // --- card ---
    let card_body: Element<'_, Msg> = column![
        query_input,
        rule::horizontal(1),
        list,
    ]
    .spacing(0)
    .width(Length::Fill)
    .into();

    let card: Element<'_, Msg> = container(card_body)
        .width(Length::Fixed(560.0))
        .padding(Padding::new(0.0))
        .style(|theme: &iced::Theme| {
            let p = theme.extended_palette();
            iced::widget::container::Style {
                background: Some(iced::Background::Color(p.background.base.color)),
                border: iced::Border {
                    color: p.background.strong.color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                shadow: iced::Shadow {
                    color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                    offset: iced::Vector::new(0.0, 8.0),
                    blur_radius: 24.0,
                },
                ..Default::default()
            }
        })
        .into();

    // Position card: centered horizontally, top-third vertically.
    // Use padding-top ≈ 33% of 1052px ≈ 350px as a fixed placeholder.
    // Task 10 corrects this from OutputGeometry.
    let positioned: Element<'_, Msg> = container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: 440.0,
            left: 0.0,
            right: 0.0,
            bottom: 0.0,
        })
        .align_x(iced::alignment::Horizontal::Center)
        .into();

    // Backdrop: full-screen mouse_area that dismisses on outside click.
    // No background fill — the launcher window is transparent so the
    // compositor shows app surfaces behind it through any unoccupied area.
    let backdrop: Element<'_, Msg> = mouse_area(
        container(text(""))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &iced::Theme| iced::widget::container::Style::default()),
    )
    .on_press(Msg::CloseLauncher)
    .into();

    // Stack: backdrop (layer 0) + card (layer 1).
    stack![backdrop, positioned]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
