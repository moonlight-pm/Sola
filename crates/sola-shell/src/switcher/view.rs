//! Switcher overlay view — macOS Cmd+Tab–style app HUD.
//!
//! A short centered horizontal strip of large app icons on a frosted
//! pill backplate. Selection is a soft neutral plate under the icon;
//! the selected app's name sits directly under that icon (other icons
//! keep an empty caption slot so the row stays aligned). Click outside
//! cancels.
//!
//! When `switcher.active` is false, returns an invisible placeholder so the
//! surface stays mapped without drawing content.

use iced::widget::{column, container, mouse_area, row, text, Space};
use iced::{Alignment, Element, Length, Padding};
use sola_kit::components::icon;
use sola_kit::fonts;

use crate::app::{Msg, Shell};

// ── Cmd+Tab HUD density ───────────────────────────────────────────────
/// Large icon face — closer to macOS dock/switcher than list chrome.
const ICON_SIZE: u16 = 72;
/// Soft plate around the selected icon (icon + pad).
const ICON_CELL: f32 = 96.0;
/// Gap between icon cells in the strip.
const ICON_GAP: f32 = 6.0;
/// Caption under the selected icon only.
const CAPTION_SIZE: f32 = 13.0;
/// Reserved height under every icon so the strip stays level when only
/// the selected cell draws a label.
const CAPTION_SLOT_H: f32 = 18.0;
/// Space between icon plate and caption slot.
const CAPTION_GAP: f32 = 6.0;
/// Keep the pill off the screen edges.
const SCREEN_MARGIN: f32 = 48.0;

/// Render the switcher overlay for `shell`.
///
/// Layout:
///   Full-screen invisible mouse_area (click-outside-to-cancel)
///   └─ Centered pill backplate (shell-switcher-bg/border/pad)
///        · Horizontal row of icon columns
///        · Each column: icon plate + caption slot (label only when selected)
pub fn view(shell: &Shell) -> Element<'_, Msg> {
    if !shell.switcher.active {
        return container(text(""))
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    let switcher = &shell.switcher;
    let tp = shell.style.switcher_tile_pad;

    let cells: Vec<Element<'_, Msg>> = switcher
        .apps
        .iter()
        .enumerate()
        .map(|(i, app)| {
            // Window app_ids come from the compositor; catalog keys may
            // differ in case (e.g. Wayland `orca` vs launcher `Orca`).
            let catalog_entry = shell.applications.get_for_window(&app.app_id);
            let icon_name = catalog_entry
                .map(|a| a.icon.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("lucide/box");

            let is_selected = i == switcher.selected;

            // Same icon resolver as the launcher: pack SVGs theme-tinted,
            // path / pack PNG refs full-color (official app faces).
            let icon_el: Element<'_, Msg> = icon(icon_name, ICON_SIZE);

            let plate: Element<'_, Msg> = container(icon_el)
                .width(Length::Fixed(ICON_CELL))
                .height(Length::Fixed(ICON_CELL))
                .center_x(Length::Fixed(ICON_CELL))
                .center_y(Length::Fixed(ICON_CELL))
                .padding(Padding::new(tp))
                .style(sola_kit::components::card::list_tile_style_colored(
                    is_selected,
                    shell.style.switcher_icon_bg,
                    shell.style.switcher_icon_fg,
                ))
                .into();

            // Caption under *this* icon when selected; empty slot otherwise
            // so unselected icons don't jump when selection moves.
            let caption: Element<'_, Msg> = if is_selected {
                let label = catalog_entry
                    .map(|a| a.label.as_str())
                    .unwrap_or(app.app_id.as_str());
                container(
                    text(label)
                        .font(fonts::chrome())
                        .size(CAPTION_SIZE)
                        .style(|theme: &iced::Theme| iced::widget::text::Style {
                            color: Some(iced::Color {
                                a: 0.92,
                                ..theme.palette().text
                            }),
                        }),
                )
                .width(Length::Fixed(ICON_CELL))
                .height(Length::Fixed(CAPTION_SLOT_H))
                .center_x(Length::Fixed(ICON_CELL))
                .center_y(Length::Fixed(CAPTION_SLOT_H))
                .into()
            } else {
                Space::new()
                    .width(Length::Fixed(ICON_CELL))
                    .height(Length::Fixed(CAPTION_SLOT_H))
                    .into()
            };

            let cell: Element<'_, Msg> = column![plate, caption]
                .spacing(CAPTION_GAP)
                .align_x(Alignment::Center)
                .into();

            mouse_area(cell)
                .on_enter(Msg::SwitcherHover { index: i })
                .into()
        })
        .collect();

    let strip: Element<'_, Msg> = row(cells)
        .spacing(ICON_GAP)
        .align_y(Alignment::Start)
        .into();

    // Horizontal pad on the pill is a bit wider than vertical (macOS
    // HUD is short and wide). Vertical = shell-switcher-pad; horizontal
    // = pad + 8.
    let pad = shell.style.switcher_pad;
    let backplate: Element<'_, Msg> = sola_kit::components::backplate(
        strip,
        shell.style.switcher_bg,
        shell.style.switcher_border,
    )
    .padding(Padding {
        top: pad,
        // Slightly tighter under the caption slot so the name doesn't
        // float in a tall empty pad band at the bottom of the pill.
        bottom: (pad - 2.0).max(6.0),
        left: pad + 8.0,
        right: pad + 8.0,
    })
    .into();

    // Cap width so a very long MRU strip doesn't hit the bezel; the row
    // still lays out left-to-right (no wrapping grid).
    let output_w = shell.output_size.map(|(w, _)| w as f32).unwrap_or(1920.0);
    let max_w = (output_w - 2.0 * SCREEN_MARGIN).max(ICON_CELL);

    let constrained: Element<'_, Msg> = container(backplate)
        .max_width(max_w)
        .into();

    let centered: Element<'_, Msg> = container(constrained)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();

    mouse_area(centered)
        .on_press(Msg::SwitcherCancel)
        .into()
}
