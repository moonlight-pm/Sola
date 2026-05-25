//! Menubar window view.
//!
//! Layout (left-to-right):
//!   [≡] [App Name] [Menu1] [Menu2] … ──────────────── [toast] [clock]
//!    ^system-menu  ^app-title  ^menu-labels (index 0 is the app name menu)

use iced::widget::{container, mouse_area, row, text};
use iced::{Element, Length};
use sola_kit::components::icon_colored;
use sola_kit::fonts::{SF_PRO, SF_PRO_MEDIUM};

use crate::app::Msg;
use crate::components::clock::clock_widget;
use crate::components::toast::toast_widget;
use crate::menu::state::synthesized_menu;

/// Render the menubar for `shell`.
pub fn view(shell: &crate::app::Shell) -> Element<'_, Msg> {
    let mb = &shell.menubar;

    // ── System-menu icon ──────────────────────────────────────────────
    // White flower glyph; clickable region is whole padded area.
    let system_fg = iced::Color::WHITE;
    let system_active = shell.menu_open && shell.current_open_is_system;
    let system_btn: Element<'_, Msg> = mouse_area(
        highlight_container(
            container(icon_colored("sola/flower", 16, system_fg))
                .padding([2, 8]),
            system_active,
        ),
    )
    .on_press(Msg::OpenMenu { index: 0, is_system: true })
    .on_enter(Msg::HoverMenu { index: 0, is_system: true })
    .into();

    // ── Focused-app title ─────────────────────────────────────────────
    // Bold text of the focused app's display name (first menu label, or
    // the app label from the applications catalog, or the raw app_id).
    let app_title_str = focused_app_title(shell);
    let clickable = has_menu(shell);
    let title_active = shell.menu_open
        && !shell.current_open_is_system
        && shell.current_open_index == Some(0);
    let app_title: Element<'_, Msg> = if clickable {
        mouse_area(
            highlight_container(
                container(
                    text(app_title_str)
                        .font(SF_PRO_MEDIUM)
                        .size(15),
                )
                .padding([2, 8]),
                title_active,
            ),
        )
        .on_press(Msg::OpenMenu { index: 0, is_system: false })
        .on_enter(Msg::HoverMenu { index: 0, is_system: false })
        .into()
    } else {
        container(
            text(app_title_str)
                .font(SF_PRO_MEDIUM)
                .size(15),
        )
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


/// Wrap a menubar label container so it paints a highlight background
/// when `active` is true. macOS-style: subtle translucent white over the
/// black menubar so the open-menu trigger is visually anchored.
fn highlight_container<'a>(
    inner: iced::widget::Container<'a, Msg>,
    active: bool,
) -> iced::widget::Container<'a, Msg> {
    container(inner).style(move |_theme: &iced::Theme| {
        if !active {
            return iced::widget::container::Style::default();
        }
        iced::widget::container::Style {
            background: Some(iced::Background::Color(
                iced::Color::from_rgba(1.0, 1.0, 1.0, 0.15),
            )),
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
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

/// True if the focused app has a clickable menubar title. We always have
/// at least a synthesized "Quit <App>" menu for any focused app, so the
/// title is clickable whenever any app is focused.
fn has_menu(shell: &crate::app::Shell) -> bool {
    shell.focused_app_id.is_some()
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
            let active = shell.menu_open
                && !shell.current_open_is_system
                && shell.current_open_index == Some(index);
            mouse_area(
                highlight_container(
                    container(
                        text(menu.label.clone())
                            .font(SF_PRO)
                            .size(15),
                    )
                    .padding([2, 8]),
                    active,
                ),
            )
            .on_press(Msg::OpenMenu { index, is_system: false })
            .on_enter(Msg::HoverMenu { index, is_system: false })
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
