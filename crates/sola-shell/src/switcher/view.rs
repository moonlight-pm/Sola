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
///   Full-screen transparent backdrop
///   └─ Centered card strip: `row` of switcher_card per app in `switcher.apps`
///      Each card: icon(52) + label, highlighted if index == selected.
///      Wrapped in mouse_area for hover-select.
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
                            background: Some(iced::Background::Color(p.primary.weak.color)),
                            border: iced::Border {
                                radius: 8.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    } else {
                        // No background for unselected cards — the switcher
                        // window is transparent so only selected cards show
                        // a highlight; the rest are fully see-through.
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

            // Wrap in mouse_area so hovering selects the card.
            mouse_area(card_container)
                .on_enter(Msg::SwitcherHover { index: i })
                .into()
        })
        .collect();

    // --- card strip ---
    let strip: Element<'_, Msg> = container(
        row(cards)
            .spacing(12)
            .align_y(Alignment::Center),
    )
    .padding(Padding::new(24.0))
    .style(|theme: &iced::Theme| {
        let p = theme.extended_palette();
        iced::widget::container::Style {
            background: Some(iced::Background::Color(p.background.base.color)),
            border: iced::Border {
                color: p.background.strong.color,
                width: 1.0,
                radius: 12.0.into(),
            },
            shadow: iced::Shadow {
                color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.4),
                offset: iced::Vector::new(0.0, 12.0),
                blur_radius: 32.0,
            },
            ..Default::default()
        }
    })
    .into();

    // Center the strip horizontally and vertically.
    let centered: Element<'_, Msg> = container(strip)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();

    // Transparent backdrop — dismiss switcher on outside click (Cancel).
    // Chord wiring (Meta+Tab, Super_L release, Escape) is in Task 10.
    mouse_area(centered)
        .on_press(Msg::SwitcherCancel)
        .into()
}
