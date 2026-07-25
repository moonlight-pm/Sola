//! Menubar window view.
//!
//! Layout (left-to-right):
//!   [≡] [App Name] [Menu1] [Menu2] … ──────────────── [toast] [stats] [clock]
//!    ^system-menu  ^app-title  ^menu-labels (index 0 is the app name menu)
//!
//! Type matches macOS menu bar: one chrome face throughout (labels, stats,
//! clock). Focused-app name is bold (macOS application menu title). Colours
//! come from the live theme palette (no view-local hex). Mono is for
//! code/detail panels, not menubar status values.
//!
//! Hit targets are full bar height ([`BAR_H`] = window height) so a pointer
//! at y=0 on the screen still activates a menu title.

use iced::widget::{container, mouse_area, row, text};
use iced::{Alignment, Color, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::icon_colored;
use sola_kit::fonts;

use crate::app::Msg;
use crate::components::clock::clock_widget;
use crate::components::toast::toast_widget;
use crate::menu::state::synthesized_menu;
use crate::menubar::{FlashTarget, WINDOW_HEIGHT};

// ── Density (logical px) ────────────────────────────────────────────────
// macOS menu bar ~13pt system chrome, regular weight; app name bold.
// Horizontal rhythm comes from per-item pad, not big row gaps.
// Bar height stays at `WINDOW_HEIGHT` (28) for zoning.
const CHROME_SIZE: f32 = 13.0;
const ICON_SIZE: u16 = 14;
/// Full window height — every interactive label uses this so the hit box
/// reaches the top edge of the screen (y=0), not a short centred chip.
const BAR_H: f32 = WINDOW_HEIGHT as f32;
/// Horizontal pad inside each menubar hit target.
/// ~9px ≈ macOS menu-title breathing room at 13pt.
const ITEM_PAD_H: f32 = 9.0;
const ITEM_PAD: Padding = Padding {
    top: 0.0,
    right: ITEM_PAD_H,
    bottom: 0.0,
    left: ITEM_PAD_H,
};
/// Optical nudge for the flower glyph (SVG visual center sits slightly
/// low relative to SF Pro Text cap height at 13pt).
const FLOWER_NUDGE_UP: f32 = 1.5;
/// Gap *between* right-cluster status buttons (CPU … clock). Combined with
/// ITEM_PAD this reads like separate menu extras, not one fused strip.
const CLUSTER_SPACING: f32 = 4.0;
/// Gap between label and value inside one status indicator.
const STAT_INNER_SPACING: f32 = 5.0;
// Fixed value-slot widths so indicators don't reflow as digits change.
// Chrome type is proportional — space-padding alone cannot pin layout.
// "100%" / "—" ≈ 36px; rates up to "999.9 MB/s" ≈ 78px at 13pt.
const STAT_VALUE_W: f32 = 36.0;
const RATE_VALUE_W: f32 = 78.0;

/// Bold chrome for the focused-app title (macOS application menu name).
fn app_title_font() -> iced::Font {
    iced::Font {
        weight: iced::font::Weight::Bold,
        ..fonts::chrome()
    }
}

/// Full-height menubar control: button is exactly [`BAR_H`] tall so layout
/// (and hit testing) spans y=0…BAR_H of the window.
///
/// Iced buttons pin their child at the top of the padded area — wrap the
/// label in a Fill-height container so the text is vertically centred while
/// the hit box stays full-height.
fn bar_button<'a>(
    content: impl Into<Element<'a, Msg>>,
    active: bool,
    on_press: Msg,
) -> iced::widget::Button<'a, Msg> {
    let centered = container(content.into())
        .width(Length::Shrink)
        .height(Length::Fill)
        .align_y(Alignment::Center);
    iced::widget::button(centered)
        .style(kit_btn::menubar(active))
        .padding(ITEM_PAD)
        .height(Length::Fixed(BAR_H))
        .on_press(on_press)
}

/// Full-height hit target with optional hover-to-switch while a menu is open.
fn bar_item<'a>(
    content: impl Into<Element<'a, Msg>>,
    active: bool,
    on_press: Msg,
    on_enter: Option<Msg>,
) -> Element<'a, Msg> {
    let btn = bar_button(content, active, on_press);
    match on_enter {
        Some(enter) => mouse_area(btn).on_enter(enter).into(),
        None => btn.into(),
    }
}

/// Render the menubar for `shell`.
pub fn view(shell: &crate::app::Shell) -> Element<'_, Msg> {
    let mb = &shell.menubar;
    let fg = shell.theme.palette().text;
    // Slightly quiet prefix ("CPU") so the value carries the scan weight —
    // same face/size as the value, just lower opacity (not a second size).
    let muted = Color { a: 0.62, ..fg };

    // ── System-menu icon ──────────────────────────────────────────────
    let system_active =
        (shell.menu_open && shell.current_open_is_system) || flashing(shell, true, 0);
    // Extra bottom pad optically lifts the flower into the text baseline
    // band without changing the outer hit target height.
    let flower = container(icon_colored("sola/flower", ICON_SIZE, fg)).padding(Padding {
        top: 0.0,
        right: 0.0,
        bottom: FLOWER_NUDGE_UP,
        left: 0.0,
    });
    let system_btn: Element<'_, Msg> = bar_item(
        flower,
        system_active,
        Msg::OpenMenu {
            index: 0,
            is_system: true,
        },
        Some(Msg::HoverMenu {
            index: 0,
            is_system: true,
        }),
    );

    // ── Focused-app title ─────────────────────────────────────────────
    // Bold — matches macOS application menu title vs regular menu labels.
    let app_title_str = focused_app_title(shell);
    let clickable = has_menu(shell);
    let title_active = (shell.menu_open
        && !shell.current_open_is_system
        && shell.current_open_index == Some(0))
        || flashing(shell, false, 0);
    let app_title: Element<'_, Msg> = if clickable {
        bar_item(
            text(app_title_str)
                .font(app_title_font())
                .size(CHROME_SIZE),
            title_active,
            Msg::OpenMenu {
                index: 0,
                is_system: false,
            },
            Some(Msg::HoverMenu {
                index: 0,
                is_system: false,
            }),
        )
    } else {
        container(
            text(app_title_str)
                .font(app_title_font())
                .size(CHROME_SIZE),
        )
        .padding(ITEM_PAD)
        .height(Length::Fixed(BAR_H))
        .align_y(Alignment::Center)
        .into()
    };

    // ── App-menu labels (menus[1..]) ──────────────────────────────────
    let menu_labels: Vec<Element<'_, Msg>> = app_menu_labels(shell);

    // ── Right cluster: toast + clock ─────────────────────────────────
    let toast = toast_widget(mb.toast.as_deref());
    let clock_active = shell.menu_open && shell.open_panel == Some(crate::app::Panel::Calendar);
    let clock: Element<'_, Msg> = bar_button(
        clock_widget(&mb.clock_now),
        clock_active,
        Msg::ToggleCalendar,
    )
    .into();

    // ── System-stat indicators (left of clock) ───────────────────────
    let neutral = fg;
    let first_tick = shell.cpu_hist.is_empty();
    let cpu_pct = shell.stats.cpu_pct;
    let cpu_btn: Element<'_, Msg> = bar_button(
        stat_indicator(
            "CPU",
            if first_tick {
                "\u{2014}".to_string()
            } else {
                format!("{:.0}%", cpu_pct)
            },
            crate::stats::level_color(cpu_pct, neutral),
            muted,
            STAT_VALUE_W,
        ),
        shell.open_panel == Some(crate::app::Panel::Stat(crate::stats::Metric::Cpu)),
        Msg::ToggleStatPanel(crate::stats::Metric::Cpu),
    )
    .into();

    let mem_pct = shell.stats.mem_pct;
    let mem_btn: Element<'_, Msg> = bar_button(
        stat_indicator(
            "MEM",
            if first_tick {
                "\u{2014}".to_string()
            } else {
                format!("{:.0}%", mem_pct)
            },
            crate::stats::level_color(mem_pct, neutral),
            muted,
            STAT_VALUE_W,
        ),
        shell.open_panel == Some(crate::app::Panel::Stat(crate::stats::Metric::Mem)),
        Msg::ToggleStatPanel(crate::stats::Metric::Mem),
    )
    .into();

    let rx_btn: Element<'_, Msg> = bar_button(
        rate_indicator("RX", shell.stats.net_down, neutral, muted),
        shell.open_panel == Some(crate::app::Panel::Stat(crate::stats::Metric::Rx)),
        Msg::ToggleStatPanel(crate::stats::Metric::Rx),
    )
    .into();
    let tx_btn: Element<'_, Msg> = bar_button(
        rate_indicator("TX", shell.stats.net_up, neutral, muted),
        shell.open_panel == Some(crate::app::Panel::Stat(crate::stats::Metric::Tx)),
        Msg::ToggleStatPanel(crate::stats::Metric::Tx),
    )
    .into();

    // ── Assemble ──────────────────────────────────────────────────────
    let mut left = vec![system_btn, app_title];
    left.extend(menu_labels);

    let mut cluster: Vec<Element<'_, Msg>> = vec![cpu_btn];
    if let Some(g) = shell.stats.gpu {
        let gpu_btn: Element<'_, Msg> = bar_button(
            stat_indicator(
                "GPU",
                format!("{:.0}%", g.util),
                crate::stats::level_color(g.util, neutral),
                muted,
                STAT_VALUE_W,
            ),
            shell.open_panel == Some(crate::app::Panel::Stat(crate::stats::Metric::Gpu)),
            Msg::ToggleStatPanel(crate::stats::Metric::Gpu),
        )
        .into();
        cluster.push(gpu_btn);
    }
    cluster.push(mem_btn);
    cluster.push(rx_btn);
    cluster.push(tx_btn);
    cluster.push(clock);

    // Fixed BAR_H root — matches the window size so children with Fixed(BAR_H)
    // are not shrink-wrapped / vertically centred with a dead band at y=0.
    container(
        row![
            row(left).height(Length::Fixed(BAR_H)),
            iced::widget::Space::new().width(Length::Fill),
            toast,
            iced::widget::row(cluster)
                .spacing(CLUSTER_SPACING)
                .height(Length::Fixed(BAR_H)),
        ]
        .width(Length::Fill)
        .height(Length::Fixed(BAR_H)),
    )
    .width(Length::Fill)
    .height(Length::Fixed(BAR_H))
    .into()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn focused_app_title(shell: &crate::app::Shell) -> String {
    let Some(ref app_id) = shell.focused_app_id else {
        return String::new();
    };

    if let Some(payload) = shell.menus.get_menu(app_id) {
        if let Some(first) = payload.menus.first() {
            return first.label.clone();
        }
    }

    let synth = synthesized_menu(app_id, &display_label(shell, app_id));
    synth
        .menus
        .first()
        .map(|d| d.label.clone())
        .unwrap_or_else(|| app_id.clone())
}

fn has_menu(shell: &crate::app::Shell) -> bool {
    shell.focused_app_id.is_some()
}

fn flashing(shell: &crate::app::Shell, is_system: bool, index: usize) -> bool {
    shell.menubar.flash == Some(FlashTarget { is_system, index })
}

fn app_menu_labels(shell: &crate::app::Shell) -> Vec<Element<'_, Msg>> {
    let Some(ref app_id) = shell.focused_app_id else {
        return Vec::new();
    };

    let owned_synth;
    let payload = match shell.menus.get_menu(app_id) {
        Some(p) => p,
        None => {
            owned_synth = synthesized_menu(app_id, &display_label(shell, app_id));
            &owned_synth
        }
    };

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
            bar_item(
                text(menu.label.clone())
                    .font(fonts::chrome())
                    .size(CHROME_SIZE),
                active,
                Msg::OpenMenu {
                    index,
                    is_system: false,
                },
                Some(Msg::HoverMenu {
                    index,
                    is_system: false,
                }),
            )
        })
        .collect()
}

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

/// Status indicator: muted chrome label + chrome value (same face/size as
/// menu titles). No mono — menubar reads as one type system.
///
/// `value_w` pins the value slot so the cluster doesn't bounce when
/// digits/units change (chrome is proportional).
fn stat_indicator<'a>(
    label: &'a str,
    value: String,
    color: Color,
    muted: Color,
    value_w: f32,
) -> Element<'a, Msg> {
    row![
        text(label)
            .font(fonts::chrome())
            .size(CHROME_SIZE)
            .style(move |_: &Theme| iced::widget::text::Style {
                color: Some(muted),
            }),
        text(value)
            .font(fonts::chrome())
            .size(CHROME_SIZE)
            .width(Length::Fixed(value_w))
            .align_x(iced::alignment::Horizontal::Right)
            .style(move |_: &Theme| iced::widget::text::Style {
                color: Some(color),
            }),
    ]
    .spacing(STAT_INNER_SPACING)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

/// TX/RX rate indicator — same chrome type as [`stat_indicator`].
fn rate_indicator<'a>(
    label: &'a str,
    bps: f32,
    color: Color,
    muted: Color,
) -> Element<'a, Msg> {
    let value = crate::stats::view::fmt_rate(bps);
    stat_indicator(label, value, color, muted, RATE_VALUE_W)
}
