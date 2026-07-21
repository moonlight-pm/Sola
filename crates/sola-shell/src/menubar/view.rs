//! Menubar window view.
//!
//! Layout (left-to-right):
//!   [≡] [App Name] [Menu1] [Menu2] … ──────────────── [toast] [clock]
//!    ^system-menu  ^app-title  ^menu-labels (index 0 is the app name menu)
//!
//! Density targets macOS menu bar: compact type, tight horizontal padding,
//! quiet status cluster. Type uses kit font *roles* (not family constants);
//! colours come from the live theme palette (no view-local hex).

use iced::widget::{container, mouse_area, row, text};
use iced::{Color, Element, Length, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::icon_colored;
use sola_kit::fonts;

use crate::app::Msg;
use crate::components::clock::clock_widget;
use crate::components::toast::toast_widget;
use crate::menu::state::synthesized_menu;
use crate::menubar::FlashTarget;

// ── Density (logical px) ────────────────────────────────────────────────
// macOS menu bar reads ~13pt chrome with tight hit padding. Keep bar height
// at `WINDOW_HEIGHT` (28); only type + padding tighten here.
const LABEL_SIZE: f32 = 13.0;
const TITLE_SIZE: f32 = 13.0;
const STAT_LABEL_SIZE: f32 = 10.0;
const STAT_VALUE_SIZE: f32 = 12.0;
const ICON_SIZE: u16 = 14;
/// Vertical, horizontal padding inside each menubar button.
const ITEM_PAD: [u16; 2] = [1, 6];
/// Gap between right-cluster status items (CPU … clock). Item pad already
/// supplies most breathing room; large inter-item gaps fight scanability.
const CLUSTER_SPACING: f32 = 2.0;
/// Gap between left-cluster labels (flower / app / File / …).
const LEFT_SPACING: f32 = 0.0;

/// Render the menubar for `shell`.
pub fn view(shell: &crate::app::Shell) -> Element<'_, Msg> {
    let mb = &shell.menubar;
    let fg = shell.theme.palette().text;
    let muted = Color { a: 0.55, ..fg };

    // ── System-menu icon ──────────────────────────────────────────────
    // Flower glyph; clickable region is whole padded area.
    let system_active =
        (shell.menu_open && shell.current_open_is_system) || flashing(shell, true, 0);
    // button: press + hover-fill; mouse_area: hover-to-switch signal
    // (outer mouse_area still receives on_enter — only presses are captured by the button).
    let system_btn: Element<'_, Msg> = mouse_area(
        iced::widget::button(container(icon_colored("sola/flower", ICON_SIZE, fg)))
            .style(kit_btn::menubar(system_active))
            .padding(ITEM_PAD)
            .on_press(Msg::OpenMenu {
                index: 0,
                is_system: true,
            }),
    )
    .on_enter(Msg::HoverMenu {
        index: 0,
        is_system: true,
    })
    .into();

    // ── Focused-app title ─────────────────────────────────────────────
    // Medium-weight chrome of the focused app's display name (first menu
    // label, or the app label from the applications catalog, or the raw app_id).
    let app_title_str = focused_app_title(shell);
    let clickable = has_menu(shell);
    let title_active = (shell.menu_open
        && !shell.current_open_is_system
        && shell.current_open_index == Some(0))
        || flashing(shell, false, 0);
    let app_title: Element<'_, Msg> = if clickable {
        mouse_area(
            iced::widget::button(
                text(app_title_str)
                    .font(fonts::ui_medium())
                    .size(TITLE_SIZE),
            )
            .style(kit_btn::menubar(title_active))
            .padding(ITEM_PAD)
            .on_press(Msg::OpenMenu {
                index: 0,
                is_system: false,
            }),
        )
        .on_enter(Msg::HoverMenu {
            index: 0,
            is_system: false,
        })
        .into()
    } else {
        container(
            text(app_title_str)
                .font(fonts::ui_medium())
                .size(TITLE_SIZE),
        )
        .padding(ITEM_PAD)
        .into()
    };

    // ── App-menu labels (menus[1..]) ──────────────────────────────────
    let menu_labels: Vec<Element<'_, Msg>> = app_menu_labels(shell);

    // ── Right cluster: toast + clock ─────────────────────────────────
    let toast = toast_widget(mb.toast.as_deref());
    // Clock is a button that toggles the calendar dropdown.
    let clock_active = shell.menu_open && shell.open_panel == Some(crate::app::Panel::Calendar);
    let clock: Element<'_, Msg> = iced::widget::button(clock_widget(&mb.clock_now))
        .style(kit_btn::menubar(clock_active))
        .padding(ITEM_PAD)
        .on_press(Msg::ToggleCalendar)
        .into();

    // ── System-stat indicators (left of clock) ───────────────────────
    // Neutral value colour = live theme text (not a frozen hex).
    let neutral = fg;
    let first_tick = shell.cpu_hist.is_empty();
    let cpu_pct = shell.stats.cpu_pct;
    let cpu_btn: Element<'_, Msg> = iced::widget::button(stat_indicator(
        "CPU",
        if first_tick {
            "\u{2014}".to_string()
        } else {
            format!("{:.0}%", cpu_pct)
        },
        crate::stats::level_color(cpu_pct, neutral),
        muted,
    ))
    .style(kit_btn::menubar(
        shell.open_panel == Some(crate::app::Panel::Stat(crate::stats::Metric::Cpu)),
    ))
    .padding(ITEM_PAD)
    .on_press(Msg::ToggleStatPanel(crate::stats::Metric::Cpu))
    .into();

    let mem_pct = shell.stats.mem_pct;
    let mem_btn: Element<'_, Msg> = iced::widget::button(stat_indicator(
        "MEM",
        if first_tick {
            "\u{2014}".to_string()
        } else {
            format!("{:.0}%", mem_pct)
        },
        crate::stats::level_color(mem_pct, neutral),
        muted,
    ))
    .style(kit_btn::menubar(
        shell.open_panel == Some(crate::app::Panel::Stat(crate::stats::Metric::Mem)),
    ))
    .padding(ITEM_PAD)
    .on_press(Msg::ToggleStatPanel(crate::stats::Metric::Mem))
    .into();

    // TX/RX are separate indicators (same style as CPU/MEM), each with its
    // own detail panel. RX = download (net_down), TX = upload (net_up).
    let rx_btn: Element<'_, Msg> = iced::widget::button(rate_indicator(
        "RX",
        shell.stats.net_down,
        neutral,
        muted,
    ))
    .style(kit_btn::menubar(
        shell.open_panel == Some(crate::app::Panel::Stat(crate::stats::Metric::Rx)),
    ))
    .padding(ITEM_PAD)
    .on_press(Msg::ToggleStatPanel(crate::stats::Metric::Rx))
    .into();
    let tx_btn: Element<'_, Msg> = iced::widget::button(rate_indicator(
        "TX",
        shell.stats.net_up,
        neutral,
        muted,
    ))
    .style(kit_btn::menubar(
        shell.open_panel == Some(crate::app::Panel::Stat(crate::stats::Metric::Tx)),
    ))
    .padding(ITEM_PAD)
    .on_press(Msg::ToggleStatPanel(crate::stats::Metric::Tx))
    .into();

    // ── Assemble ──────────────────────────────────────────────────────
    let mut left = vec![system_btn, app_title];
    left.extend(menu_labels);

    // ── Indicator cluster (GPU hidden when no NVIDIA GPU) ─────────────
    let mut cluster: Vec<Element<'_, Msg>> = vec![cpu_btn];
    if let Some(g) = shell.stats.gpu {
        let gpu_btn: Element<'_, Msg> = iced::widget::button(stat_indicator(
            "GPU",
            format!("{:.0}%", g.util),
            crate::stats::level_color(g.util, neutral),
            muted,
        ))
        .style(kit_btn::menubar(
            shell.open_panel == Some(crate::app::Panel::Stat(crate::stats::Metric::Gpu)),
        ))
        .padding(ITEM_PAD)
        .on_press(Msg::ToggleStatPanel(crate::stats::Metric::Gpu))
        .into();
        cluster.push(gpu_btn);
    }
    cluster.push(mem_btn);
    cluster.push(rx_btn);
    cluster.push(tx_btn);
    cluster.push(clock);

    row![
        row(left)
            .spacing(LEFT_SPACING)
            .align_y(iced::alignment::Vertical::Center),
        iced::widget::Space::new().width(iced::Length::Fill),
        toast,
        iced::widget::row(cluster)
            .spacing(CLUSTER_SPACING)
            .align_y(iced::alignment::Vertical::Center),
    ]
    .height(Length::Fill)
    .align_y(iced::alignment::Vertical::Center)
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

/// True if the focused app has a clickable menubar title. We always have
/// at least a synthesized "Quit <App>" menu for any focused app, so the
/// title is clickable whenever any app is focused.
fn has_menu(shell: &crate::app::Shell) -> bool {
    shell.focused_app_id.is_some()
}

/// True when the menubar label addressed by `(is_system, index)` is the one
/// currently flashing as keyboard-shortcut feedback (the macOS "command ran
/// through the menu" pulse). Reuses the open-menu highlight, so a flash looks
/// identical to a momentary selection.
fn flashing(shell: &crate::app::Shell, is_system: bool, index: usize) -> bool {
    shell.menubar.flash == Some(FlashTarget { is_system, index })
}

/// Build the app-menu label buttons (menus[1..] of the focused app).
/// Each label becomes a `mouse_area` wrapping a kit menubar button.
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
            let active = (shell.menu_open
                && !shell.current_open_is_system
                && shell.current_open_index == Some(index))
                || flashing(shell, false, index);
            mouse_area(
                iced::widget::button(
                    text(menu.label.clone())
                        .font(fonts::chrome())
                        .size(LABEL_SIZE),
                )
                .style(kit_btn::menubar(active))
                .padding(ITEM_PAD)
                .on_press(Msg::OpenMenu {
                    index,
                    is_system: false,
                }),
            )
            .on_enter(Msg::HoverMenu {
                index,
                is_system: false,
            })
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

/// One numbers-only menubar indicator: muted chrome label + mono value.
fn stat_indicator<'a>(
    label: &'a str,
    value: String,
    color: Color,
    muted: Color,
) -> Element<'a, Msg> {
    row![
        // Label stays a fixed muted gray; only the value tints on threshold.
        text(label)
            .font(fonts::chrome())
            .size(STAT_LABEL_SIZE)
            .style(move |_: &Theme| iced::widget::text::Style {
                color: Some(muted),
            }),
        // Value padded to a fixed 4-char field (mono → tabular) so "9%",
        // "100%", and "—" all render the same width; the indicator never
        // reflows as the value changes.
        text(format!("{value:>4}"))
            .font(fonts::mono())
            .size(STAT_VALUE_SIZE)
            .style(move |_: &Theme| iced::widget::text::Style {
                color: Some(color),
            }),
    ]
    .spacing(4)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

/// Menubar rate indicator (TX/RX): same layout as [`stat_indicator`], but
/// the value is a byte-rate string padded to a fixed field so the cluster
/// doesn't reflow when units jump (B/s → KB/s → MB/s).
fn rate_indicator<'a>(
    label: &'a str,
    bps: f32,
    color: Color,
    muted: Color,
) -> Element<'a, Msg> {
    let value = crate::stats::view::fmt_rate(bps);
    row![
        text(label)
            .font(fonts::chrome())
            .size(STAT_LABEL_SIZE)
            .style(move |_: &Theme| iced::widget::text::Style {
                color: Some(muted),
            }),
        // 9 chars covers "999 KB/s" / "12.3 MB/s"; mono keeps tabular width.
        text(format!("{value:>9}"))
            .font(fonts::mono())
            .size(STAT_VALUE_SIZE)
            .style(move |_: &Theme| iced::widget::text::Style {
                color: Some(color),
            }),
    ]
    .spacing(4)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}
