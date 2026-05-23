//! Menubar window view.
//!
//! Layout (left-to-right):
//!   [≡] [App Name] [Menu1] [Menu2] … ──────────────── [toast] [clock]
//!    ^system-menu  ^app-title  ^menu-labels (index 0 is the app name menu)

use iced::widget::{button, container, mouse_area, row, text};
use iced::{Element, Font, Length};
use sola_kit::components::icon;

use crate::app::Msg;
use crate::components::clock::clock_widget;
use crate::components::toast::toast_widget;
use crate::menu::state::synthesized_menu;

/// Render the menubar for `shell`.
pub fn view(shell: &crate::app::Shell) -> Element<'_, Msg> {
    let mb = &shell.menubar;

    // ── System-menu button ────────────────────────────────────────────
    let system_btn: Element<'_, Msg> = button(icon("lucide/menu", 18))
        .on_press(Msg::OpenMenu { index: 0, is_system: true })
        .padding([2, 8])
        .into();

    // ── Focused-app title ─────────────────────────────────────────────
    // Bold text of the focused app's display name (first menu label, or
    // the app label from the applications catalog, or the raw app_id).
    let app_title_str = focused_app_title(shell);
    let clickable = has_menu(shell);
    let app_title: Element<'_, Msg> = if clickable {
        mouse_area(
            container(text(app_title_str).font(Font::MONOSPACE))
                .padding([2, 8]),
        )
        .on_press(Msg::OpenMenu { index: 0, is_system: false })
        .into()
    } else {
        container(text(app_title_str).font(Font::MONOSPACE))
            .padding([2, 8])
            .into()
    };

    // ── App-menu labels (menus[1..]) ──────────────────────────────────
    let menu_labels: Vec<Element<'_, Msg>> = app_menu_labels(shell);

    // ── Right cluster: toast + clock ─────────────────────────────────
    let toast = toast_widget(mb.toast.as_deref());
    let clock = clock_widget(&mb.clock_now);

    // ── Assemble ──────────────────────────────────────────────────────
    let mut left = vec![system_btn, app_title];
    left.extend(menu_labels);

    row![
        row(left),
        iced::widget::Space::new().width(iced::Length::Fill),
        toast,
        container(clock).padding([2, 8]),
    ]
    .height(Length::Fill)
    .into()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The text shown as the focused-app title in the menubar.
/// Uses menu[0].label (the "app name" slot in the legacy convention),
/// then the applications catalog label, then the raw app_id.
fn focused_app_title(shell: &crate::app::Shell) -> String {
    let Some(ref app_id) = shell.focused_app_id else {
        return String::new();
    };

    // Try the menu cache first — menus[0].label is the app name.
    if let Some(payload) = shell.menus.get_menu(app_id) {
        if let Some(first) = payload.menus.first() {
            return first.label.clone();
        }
    }

    // Synthesize from apps catalog.
    let synth = synthesized_menu(app_id, &display_label(shell, app_id));
    synth
        .menus
        .first()
        .map(|d| d.label.clone())
        .unwrap_or_else(|| app_id.clone())
}

/// True if the focused app has a menu payload with more than zero menus.
fn has_menu(shell: &crate::app::Shell) -> bool {
    shell
        .focused_app_id
        .as_deref()
        .and_then(|id| shell.menus.get_menu(id))
        .map(|p| !p.menus.is_empty())
        .unwrap_or(false)
}

/// Build the app-menu label buttons (menus[1..] of the focused app).
/// Each label becomes a `mouse_area` wrapping styled text.
/// `on_press` → `Msg::OpenMenu { index }`
/// `on_enter` → `Msg::HoverMenu { index }` (only acts if another menu is open)
fn app_menu_labels(shell: &crate::app::Shell) -> Vec<Element<'_, Msg>> {
    let Some(ref app_id) = shell.focused_app_id else {
        return Vec::new();
    };

    // Get the real menu payload; fall back to synthesized (which has no
    // extra labels beyond menus[0]).
    let owned_synth;
    let payload = match shell.menus.get_menu(app_id) {
        Some(p) => p,
        None => {
            owned_synth = synthesized_menu(app_id, &display_label(shell, app_id));
            &owned_synth
        }
    };

    // menus[0] is the "app name" slot shown by app_title; show menus[1..].
    payload
        .menus
        .iter()
        .enumerate()
        .skip(1)
        .map(|(index, menu)| {
            mouse_area(
                container(text(menu.label.clone()))
                    .padding([2, 8]),
            )
            .on_press(Msg::OpenMenu { index, is_system: false })
            .on_enter(Msg::HoverMenu { index })
            .into()
        })
        .collect()
}

/// Resolve a human-readable label for an app_id. Falls back to the
/// app_id itself (first-char uppercased) if no applications entry exists.
fn display_label(shell: &crate::app::Shell, app_id: &str) -> String {
    if let Some(app) = shell.applications.get(app_id) {
        return app.label.clone();
    }
    let mut chars = app_id.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
