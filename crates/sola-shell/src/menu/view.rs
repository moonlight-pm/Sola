//! Menu dropdown window view.
//!
//! Layout:
//!   Full-window transparent backdrop (mouse_area → CloseMenu on outside click)
//!   └── Stack layer 1: container with left-padding = menu_anchor_x
//!       └── Card column of menu items for the currently open menu.
//!
//! The window is always present but renders nothing visible when
//! `shell.menu_open` is false — composition hides the surface (Task 10).
//! The backdrop mouse_area still catches clicks so we can emit CloseMenu
//! if something slips through.

use iced::widget::{column, container, mouse_area, row, stack, text};
use iced::{Element, Length, Padding};

use crate::app::Msg;
use crate::menu::state::synthesized_menu;
use sola_bus::topics::MenuItem;
use sola_kit::components::{button as kit_btn, divider::horizontal_divider, popover, text as kit_text};

/// Render the menu overlay for `shell`.
pub fn view(shell: &crate::app::Shell) -> Element<'_, Msg> {
    if !shell.menu_open {
        // Not open — render an invisible full-screen placeholder so iced
        // never gets an empty view.  CloseMenu on press is harmless.
        return mouse_area(
            container(text(""))
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Msg::CloseMenu)
        .into();
    }

    // Clock calendar panel takes over the same window when open.
    if shell.current_open_is_calendar {
        return calendar_panel(shell);
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

        let payload = match shell.menus.get_menu(app_id_str) {
            Some(p) => p.clone(),
            None => {
                let label = resolve_label(shell, app_id_str);
                synthesized_menu(app_id_str, &label)
            }
        };

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
        column(rows)
            .width(Length::Fill)
            .spacing(0)
            .into()
    };

    // Dropdown card — opaque rounded panel positioned at anchor_x from left.
    //
    // `popover(...)` supplies the card chrome (bg background.weaker, hairline
    // border at RADIUS_LG corners, fixed drop shadow) from the ambient theme,
    // which now has opaque background tiers. No need to close over shell.theme.
    let anchor_x = shell.menu_anchor_x;
    let card: Element<'_, Msg> = popover(items_el)
        .padding(Padding::new(4.0))
        .width(Length::Fixed(220.0))
        .into();

    // Outer container positions the card at anchor_x by using left padding.
    let positioned: Element<'_, Msg> = container(card)
        .padding(Padding {
            top: 0.0,
            left: anchor_x,
            right: 0.0,
            bottom: 0.0,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(iced::alignment::Vertical::Top)
        .into();

    // Backdrop: full-screen mouse_area that dismisses on outside click.
    let backdrop: Element<'_, Msg> = mouse_area(
        container(text(""))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(Msg::CloseMenu)
    .into();

    // Stack: backdrop (layer 0, sets intrinsic size) + positioned card (layer 1).
    stack![backdrop, positioned]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ---------------------------------------------------------------------------
// Per-item view
// ---------------------------------------------------------------------------

/// Render the calendar dropdown, right-anchored under the menubar clock,
/// over a full-screen dismiss backdrop (same window as the menu dropdown).
fn calendar_panel(shell: &crate::app::Shell) -> Element<'_, Msg> {
    let today = shell.menubar.clock_now.date_naive();
    let card = crate::calendar::view(shell.calendar_month, today);

    // Right-align the card near the screen's right edge (under the clock).
    let output_w = shell.output_size.map(|(w, _)| w as f32).unwrap_or(1920.0);
    let left = (output_w - crate::calendar::CARD_WIDTH - 8.0).max(0.0);

    let positioned: Element<'_, Msg> = container(card)
        .padding(Padding {
            top: 0.0,
            left,
            right: 0.0,
            bottom: 0.0,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(iced::alignment::Vertical::Top)
        .into();

    let backdrop: Element<'_, Msg> = mouse_area(
        container(text(""))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(Msg::CloseMenu)
    .into();

    stack![backdrop, positioned]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Build a menu item element from owned data (no borrows from caller's locals).
fn menu_item_view_owned(item: MenuItem, app_id: String) -> Element<'static, Msg> {
    match item {
        MenuItem::Divider => horizontal_divider().into(),
        MenuItem::Action {
            id,
            label,
            shortcut,
            disabled,
            ..
        } => {
            let label_txt: Element<'static, Msg> = if disabled {
                text(label).size(13.0).style(kit_text::muted).into()
            } else {
                text(label).size(13.0).into()
            };

            let shortcut_txt: Element<'static, Msg> = if let Some(chord) = shortcut {
                // Muted-but-visible: a dimmed version of the base text colour.
                // We do NOT use `kit_text::muted` (secondary.base.text) here —
                // on the dropdown card it resolves to a colour that renders
                // invisible, so the accelerators silently disappeared. Deriving
                // from `palette().text` (the same colour the labels use, known
                // to contrast with the card) and dropping the alpha keeps the
                // shortcut clearly readable while still reading as secondary.
                text(chord.display())
                    .size(12.0)
                    .style(|theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(iced::Color {
                            a: 0.55,
                            ..theme.palette().text
                        }),
                    })
                    .into()
            } else {
                text("").size(13.0).into()
            };

            let item_row: Element<'static, Msg> = row![
                label_txt,
                iced::widget::Space::new().width(Length::Fill),
                shortcut_txt,
            ]
            .width(Length::Fill)
            .align_y(iced::alignment::Vertical::Center)
            .padding([4.0, 8.0])
            .spacing(16.0)
            .into();

            if disabled {
                container(item_row).width(Length::Fill).into()
            } else {
                // Use iced::button so we get free Hovered status styling.
                // Active = transparent; Hovered = background.strong (kit
                // list_item style — subtler than primary fill, consistent
                // across all kit surfaces).
                iced::widget::button(item_row)
                    .padding(Padding::new(0.0))
                    .width(Length::Fill)
                    .on_press(Msg::MenuAction {
                        app_id,
                        action_id: id,
                    })
                    .style(kit_btn::list_item(false))
                    .into()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve a human-readable label for an app_id from the applications catalog.
fn resolve_label(shell: &crate::app::Shell, app_id: &str) -> String {
    if let Some(app) = shell.applications.get(app_id) {
        return app.label.clone();
    }
    let mut chars = app_id.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
