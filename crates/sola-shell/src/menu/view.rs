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

use iced::widget::{column, container, mouse_area, rule, row, stack, text};
use iced::{Color, Element, Length, Padding};

use crate::app::Msg;
use crate::menu::state::synthesized_menu;
use sola_bus::topics::MenuItem;

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

    // Resolve the menu to render.
    let app_id = shell
        .focused_app_id
        .as_deref()
        .unwrap_or("sola-shell");
    let index = shell.current_open_index.unwrap_or(0);

    // Resolve payload — either the cached menu or a synthesized one.
    let payload = match shell.menus.get_menu(app_id) {
        Some(p) => p.clone(),
        None => {
            // Synthesized menu: menus[0] only, with Quit action.
            let label = resolve_label(shell, app_id);
            synthesized_menu(app_id, &label)
        }
    };

    // Clone items out of the payload so elements don't borrow from a local.
    let items: Vec<MenuItem> = payload
        .menus
        .get(index)
        .map(|m| m.items.clone())
        .unwrap_or_default();
    let app_id_owned = app_id.to_string();

    let items_el: Element<'_, Msg> = if items.is_empty() {
        text("").into()
    } else {
        let rows: Vec<Element<'_, Msg>> = items
            .into_iter()
            .map(|item| menu_item_view_owned(item, app_id_owned.clone()))
            .collect();
        column(rows)
            .width(Length::Shrink)
            .spacing(0)
            .into()
    };

    // Dropdown card — white rounded panel positioned at anchor_x from left.
    let anchor_x = shell.menu_anchor_x;
    let card: Element<'_, Msg> = container(items_el)
        .padding(Padding::new(4.0))
        .style(|theme: &iced::Theme| {
            let palette = theme.extended_palette();
            iced::widget::container::Style {
                background: Some(iced::Background::Color(
                    palette.background.base.color,
                )),
                border: iced::Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                shadow: iced::Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
                    offset: iced::Vector::new(0.0, 4.0),
                    blur_radius: 12.0,
                },
                ..Default::default()
            }
        })
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

/// Build a menu item element from owned data (no borrows from caller's locals).
fn menu_item_view_owned(item: MenuItem, app_id: String) -> Element<'static, Msg> {
    match item {
        MenuItem::Divider => rule::horizontal(1).into(),
        MenuItem::Action {
            id,
            label,
            shortcut,
            disabled,
            ..
        } => {
            let label_color = if disabled {
                Some(Color::from_rgba(0.5, 0.5, 0.5, 1.0))
            } else {
                None
            };

            let label_txt: Element<'static, Msg> = {
                let t = text(label).size(13.0);
                if let Some(c) = label_color {
                    t.color(c).into()
                } else {
                    t.into()
                }
            };

            let shortcut_txt: Element<'static, Msg> = if let Some(chord) = shortcut {
                let t = text(chord.display()).size(12.0);
                if let Some(c) = label_color {
                    t.color(c).into()
                } else {
                    // Slightly muted for shortcuts even when enabled.
                    t.color(Color::from_rgba(0.55, 0.55, 0.55, 1.0)).into()
                }
            } else {
                text("").size(13.0).into()
            };

            let item_row: Element<'static, Msg> = row![
                label_txt,
                iced::widget::Space::new().width(Length::Fill),
                shortcut_txt,
            ]
            .padding([4.0, 8.0])
            .spacing(16.0)
            .into();

            if disabled {
                container(item_row)
                    .width(Length::Fill)
                    .into()
            } else {
                mouse_area(
                    container(item_row)
                        .width(Length::Fill),
                )
                .on_press(Msg::MenuAction {
                    app_id,
                    action_id: id,
                })
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
