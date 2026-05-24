//! Switcher overlay view — alt-tab equivalent.
//!
//! Renders a centered row of app cards over a transparent full-screen backdrop.
//! When `switcher.active` is false, returns an invisible placeholder so the
//! surface stays alive without rendering content.

use iced::widget::{column, container, mouse_area, row, text};
use iced::{Alignment, Element, Length, Padding};
use sola_kit::components::icon;

use crate::app::{Msg, Shell};

/// Render the switcher overlay for `shell`.
///
/// Layout:
///   Full-screen invisible mouse_area (click-outside-to-cancel)
///   └─ Centered backplate card sized to fit the apps with ~36px padding.
///      Background: slight primary-tinted translucent fill (no real blur
///      available in iced; the alpha gives a similar feel against dark
///      backgrounds).
///      Inside: row of switcher_card per app in `switcher.apps`.
pub fn view(shell: &Shell) -> Element<'_, Msg> {
    if !shell.switcher.active {
        // Invisible placeholder — keeps iced from getting an empty view.
        return container(text(""))
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    let switcher = &shell.switcher;

    // --- app cards ---
    let cards: Vec<Element<'_, Msg>> = switcher
        .apps
        .iter()
        .enumerate()
        .map(|(i, app)| {
            // Look up display label and icon from the application catalog.
            let catalog_entry = shell.applications.get(&app.app_id);
            let label_str = catalog_entry
                .map(|a| a.label.as_str())
                .unwrap_or(app.app_id.as_str());
            let icon_name = catalog_entry
                .map(|a| a.icon.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("lucide/box");

            let is_selected = i == switcher.selected;

            let icon_el: Element<'_, Msg> = icon(icon_name, 52);
            let label_el: Element<'_, Msg> = text(label_str).size(13).into();

            let card_content: Element<'_, Msg> = column![icon_el, label_el]
                .spacing(8)
                .align_x(Alignment::Center)
                .into();

            // Card container with highlighted background when selected.
            let card_container: Element<'_, Msg> = container(card_content)
                .padding(Padding {
                    top: 16.0,
                    bottom: 16.0,
                    left: 20.0,
                    right: 20.0,
                })
                .style(move |theme: &iced::Theme| {
                    let p = theme.extended_palette();
                    if is_selected {
                        iced::widget::container::Style {
                            background: Some(iced::Background::Color(p.primary.base.color)),
                            border: iced::Border {
                                radius: 8.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    } else {
                        iced::widget::container::Style {
                            background: None,
                            border: iced::Border {
                                radius: 8.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    }
                })
                .into();

            mouse_area(card_container)
                .on_enter(Msg::SwitcherHover { index: i })
                .into()
        })
        .collect();

    // Backplate close-over: derive a slight primary-tinted background and
    // matching border from the shell's REAL theme (the per-window overlay
    // palette is TRANSPARENT — see app.rs::theme).
    let real = shell.theme.extended_palette();
    let primary = real.primary.base.color;
    let backplate_bg = iced::Color::from_rgba(primary.r, primary.g, primary.b, 0.18);
    let backplate_border = iced::Color::from_rgba(primary.r, primary.g, primary.b, 0.35);

    // --- backplate: shrink-wraps the cards with 36px padding ---
    let backplate: Element<'_, Msg> = container(
        row(cards)
            .spacing(12)
            .align_y(Alignment::Center),
    )
    .padding(Padding::new(36.0))
    .style(move |_theme: &iced::Theme| iced::widget::container::Style {
        background: Some(iced::Background::Color(backplate_bg)),
        border: iced::Border {
            color: backplate_border,
            width: 1.0,
            radius: 16.0.into(),
        },
        shadow: iced::Shadow {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.45),
            offset: iced::Vector::new(0.0, 12.0),
            blur_radius: 32.0,
        },
        ..Default::default()
    })
    .into();

    // Center the backplate on screen.
    let centered: Element<'_, Msg> = container(backplate)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();

    // Full-screen invisible click-catcher dismisses the switcher. The
    // backplate sits inside its own region and absorbs hover/clicks first,
    // so clicking outside the cards is what reaches this layer.
    mouse_area(centered)
        .on_press(Msg::SwitcherCancel)
        .into()
}
