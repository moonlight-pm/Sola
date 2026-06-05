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

            // Card container: kit list_tile_style handles selected highlight
            // (primary fill, RADIUS_MD=6) and unselected (transparent).
            // Selected tiles also get primary.base.text for label legibility.
            let card_container: Element<'_, Msg> = container(card_content)
                .padding(Padding {
                    top: 16.0,
                    bottom: 16.0,
                    left: 20.0,
                    right: 20.0,
                })
                .style(sola_kit::components::card::list_tile_style(is_selected))
                .into();

            mouse_area(card_container)
                .on_enter(Msg::SwitcherHover { index: i })
                .into()
        })
        .collect();

    // --- backplate: shrink-wraps the cards with 36px padding ---
    // accent_backplate provides the primary-tinted translucent fill (0.18
    // alpha), matching border (0.35 alpha, width 1), 16px radius, and the
    // deep drop shadow — no manual palette close-over needed.
    let backplate: Element<'_, Msg> = sola_kit::components::accent_backplate(
        row(cards)
            .spacing(12)
            .align_y(Alignment::Center),
    )
    .padding(Padding::new(36.0))
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
