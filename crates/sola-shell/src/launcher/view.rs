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

    // The per-window theme on the launcher surface paints a TRANSPARENT
    // background (see app.rs::theme), so we cannot pull the card's
    // background from the in-style palette.  Close over the shell's REAL
    // theme to derive opaque card colours.
    let real = shell.theme.extended_palette();
    let card_bg = real.background.base.color;
    let card_border = real.background.strong.color;
    let primary_base = real.primary.base.color;
    let primary_base_text = real.primary.base.text;
    let bg_text = real.background.base.text;
    let bg_weak = real.background.weak.color;

    // --- query text input ---
    // Chunky: 18px text, 14px padding, no border so it reads as part of
    // the card chrome (appliance feel, not a form).
    let query_input: Element<'_, Msg> = text_input("Search…", &launcher.query)
        .id(WidgetId::new(QUERY_INPUT_ID))
        .on_input(Msg::LauncherQuery)
        .style(input_style)
        .padding(Padding::new(14.0))
        .size(18)
        .width(Length::Fill)
        .into();

    // --- application rows ---
    let rows: Vec<Element<'_, Msg>> = if launcher.filtered_ids.is_empty() {
        vec![container(text("No matching applications.").size(15))
            .padding(Padding::new(16.0))
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

                let icon_el: Element<'_, Msg> = icon(icon_name, 24);
                let label_el: Element<'_, Msg> = text(label_str).size(16).into();

                let row_content: Element<'_, Msg> = row![icon_el, label_el]
                    .spacing(14)
                    .align_y(Alignment::Center)
                    .into();

                let row_btn = iced::widget::button(row_content)
                    .on_press(Msg::Launch)
                    .padding(Padding {
                        top: 12.0,
                        bottom: 12.0,
                        left: 16.0,
                        right: 16.0,
                    })
                    .width(Length::Fill)
                    .style(move |_theme: &iced::Theme, status| {
                        if is_selected {
                            iced::widget::button::Style {
                                background: Some(iced::Background::Color(primary_base)),
                                text_color: primary_base_text,
                                border: iced::Border {
                                    radius: 8.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                        } else {
                            match status {
                                iced::widget::button::Status::Hovered => {
                                    iced::widget::button::Style {
                                        background: Some(iced::Background::Color(bg_weak)),
                                        text_color: bg_text,
                                        border: iced::Border {
                                            radius: 8.0.into(),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    }
                                }
                                _ => iced::widget::button::Style {
                                    background: None,
                                    text_color: bg_text,
                                    border: iced::Border {
                                        radius: 8.0.into(),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                },
                            }
                        }
                    });

                row_btn.into()
            })
            .collect()
    };

    let list: Element<'_, Msg> =
        scrollable(column(rows).width(Length::Fill).spacing(4))
            .width(Length::Fill)
            .height(Length::Fixed(440.0))
            .into();

    // --- card ---
    // Chunky appliance card: 640px wide, 16px internal padding around the
    // body so the rule and rows breathe.
    let card_body: Element<'_, Msg> = column![
        query_input,
        rule::horizontal(1),
        container(list).padding(Padding {
            top: 8.0,
            bottom: 8.0,
            left: 8.0,
            right: 8.0,
        }),
    ]
    .spacing(0)
    .width(Length::Fill)
    .into();

    let card: Element<'_, Msg> = container(card_body)
        .width(Length::Fixed(640.0))
        .padding(Padding::new(0.0))
        .style(move |_theme: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(card_bg)),
            border: iced::Border {
                color: card_border,
                width: 1.0,
                radius: 14.0.into(),
            },
            shadow: iced::Shadow {
                color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.55),
                offset: iced::Vector::new(0.0, 16.0),
                blur_radius: 48.0,
            },
            ..Default::default()
        })
        .into();

    // Vertically + horizontally centered card.
    let positioned: Element<'_, Msg> = container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();

    // Backdrop: very light dim so the launcher feels overlaid on the
    // workspace, not a modal sheet.
    let backdrop: Element<'_, Msg> = mouse_area(
        container(text(""))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &iced::Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(
                    iced::Color::from_rgba(0.0, 0.0, 0.0, 0.40),
                )),
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
