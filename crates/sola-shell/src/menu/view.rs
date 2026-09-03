//! Menu dropdown window view.
//!
//! Layout: the overlay window is already placed at the card (see
//! [`crate::zoning::menu_overlay_frame`]). View is the card plus a
//! backdrop that fills leftover window pixels (dismiss). Clicks outside
//! the surface hit whatever is under it (app / menubar / empty seat).
//!
//! Density matches macOS menu-bar dropdowns: chrome type at 13, compact
//! row pad, hairline separators with vertical breathing room, kit
//! `popover` chrome (calmer materials) + `menu_item` hover.

use iced::widget::{column, container, mouse_area, row, text};
use iced::{Element, Length, Padding};

use crate::app::Msg;
use sola_bus::topics::MenuItem;
use sola_kit::components::{
    button as kit_btn, divider::horizontal_divider, popover, text as kit_text,
};
use sola_kit::fonts;

/// Menu row type size — same as menubar chrome (P3).
const MENU_TYPE: f32 = 13.0;
/// Shortcut / accelerator size (slightly quieter than the label).
const ACCEL_TYPE: f32 = 12.0;
/// Per-item vertical, horizontal pad inside the row.
const ITEM_PAD: [f32; 2] = [3.0, 10.0];
/// Vertical breathing room around a separator hairline.
const SEP_V_PAD: f32 = 4.0;
/// Fixed menu card width (macOS-ish min; content rarely exceeds this).
pub const MENU_WIDTH: f32 = 240.0;
/// Generous height for a typical app/system menu (capped by usable area).
pub const MENU_HEIGHT: f32 = 480.0;

/// Render the menu overlay for `shell`.
pub fn view(shell: &crate::app::Shell) -> Element<'_, Msg> {
    if !shell.menu_open {
        // Not open — render an invisible full-screen placeholder so iced
        // never gets an empty view.  CloseMenu on press is harmless.
        return mouse_area(container(text("")).width(Length::Fill).height(Length::Fill))
            .on_press(Msg::CloseMenu)
            .into();
    }

    // Clock calendar panel takes over the same window when open.
    match shell.open_panel {
        Some(crate::app::Panel::Calendar) => return calendar_panel(shell),
        Some(crate::app::Panel::Stat(m)) => return crate::stats::view::panel(shell, m),
        Some(crate::app::Panel::NotifyPile) => return crate::notify::view::pile_panel(shell),
        Some(crate::app::Panel::Bluetooth) => return crate::bluetooth::view::panel(shell),
        Some(crate::app::Panel::Audio) => return crate::audio::view::panel(shell),
        None => {}
    }

    // When the system-menu button was pressed, show the shell's own menu
    // (registered by BusSetup as "sola-shell").  Otherwise show the
    // focused app's menu at the selected index.
    let (app_id, items): (String, Vec<MenuItem>) = if shell.current_open_is_system {
        let shell_menu = shell.menus.get_menu(crate::app::Shell::APP_ID);
        let items = shell_menu
            .and_then(|p| p.menus.first())
            .map(|m| m.items.clone())
            .unwrap_or_default();
        (crate::app::Shell::APP_ID.to_string(), items)
    } else {
        let app_id_str = shell
            .focused_app_id
            .as_deref()
            .unwrap_or(crate::app::Shell::APP_ID);
        let index = shell.current_open_index.unwrap_or(0);

        let payload = shell.effective_app_menu(app_id_str);

        let items = payload
            .menus
            .get(index)
            .map(|m| m.items.clone())
            .unwrap_or_default();
        (app_id_str.to_string(), items)
    };

    let items_el: Element<'_, Msg> = if items.is_empty() {
        text("").into()
    } else {
        let rows: Vec<Element<'_, Msg>> = items
            .into_iter()
            .map(|item| menu_item_view_owned(item, app_id.clone()))
            .collect();
        // Fill the card horizontally so each row spans the full menu width
        // and the shortcut sits flush against the right padding (macOS feel).
        column(rows).width(Length::Fill).spacing(0).into()
    };

    // Dropdown card — kit popover chrome (raised bg, calm shadow, MD radius).
    // Default popover pad is already SPACE_SM (4); keep explicit for clarity.
    let card: Element<'_, Msg> = popover(items_el).width(Length::Fill).into();
    crate::menu::host_card(card)
}

// ---------------------------------------------------------------------------
// Per-item view
// ---------------------------------------------------------------------------

/// Render the calendar dropdown, right-anchored under the menubar clock,
/// over a full-screen dismiss backdrop (same window as the menu dropdown).
fn calendar_panel(shell: &crate::app::Shell) -> Element<'_, Msg> {
    let today = shell.menubar.clock_now.date_naive();
    crate::menu::host_card(crate::calendar::view(shell.calendar_month, today))
}

/// Build a menu item element from owned data (no borrows from caller's locals).
fn menu_item_view_owned(item: MenuItem, app_id: String) -> Element<'static, Msg> {
    match item {
        MenuItem::Divider => menu_separator(),
        MenuItem::Action {
            id,
            label,
            shortcut,
            disabled,
            checked,
        } => {
            // Fixed-width leading slot so labels align across checked / unchecked.
            let check_slot: Element<'static, Msg> = text(if checked { "✓" } else { " " })
                .font(fonts::chrome())
                .size(MENU_TYPE)
                .width(Length::Fixed(14.0))
                .into();

            let label_txt: Element<'static, Msg> = if disabled {
                text(label)
                    .font(fonts::chrome())
                    .size(MENU_TYPE)
                    .style(kit_text::muted)
                    .into()
            } else {
                text(label).font(fonts::chrome()).size(MENU_TYPE).into()
            };

            let shortcut_txt: Element<'static, Msg> = if let Some(chord) = shortcut {
                // Muted-but-visible: dim `palette().text`. Avoid
                // `kit_text::muted` (secondary.base.text) — on the dropdown
                // card it can resolve invisible against weaker bg.
                text(chord.display())
                    .font(fonts::chrome())
                    .size(ACCEL_TYPE)
                    .style(|theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(iced::Color {
                            a: 0.55,
                            ..theme.palette().text
                        }),
                    })
                    .into()
            } else {
                text("").font(fonts::chrome()).size(MENU_TYPE).into()
            };

            let item_row: Element<'static, Msg> = row![
                check_slot,
                label_txt,
                iced::widget::Space::new().width(Length::Fill),
                shortcut_txt,
            ]
            .width(Length::Fill)
            .align_y(iced::alignment::Vertical::Center)
            .padding(ITEM_PAD)
            .spacing(8.0)
            .into();

            if disabled {
                container(item_row).width(Length::Fill).into()
            } else {
                iced::widget::button(item_row)
                    .padding(Padding::new(0.0))
                    .width(Length::Fill)
                    .on_press(Msg::MenuAction {
                        app_id,
                        action_id: id,
                    })
                    .style(kit_btn::menu_item)
                    .into()
            }
        }
    }
}

/// Hairline separator with vertical breathing room (macOS menu section gap).
fn menu_separator() -> Element<'static, Msg> {
    container(horizontal_divider())
        .width(Length::Fill)
        .padding(Padding {
            top: SEP_V_PAD,
            bottom: SEP_V_PAD,
            left: 0.0,
            right: 0.0,
        })
        .into()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
