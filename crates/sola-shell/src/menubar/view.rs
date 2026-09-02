//! Menubar window view.
//!
//! Layout (left-to-right):
//!   [≡] [App Name] [Menu1] [Menu2] … ──────── [mail?] [stats] [clock]
//!    ^system-menu  ^app-title  ^menu-labels (index 0 is the app name menu)
//!
//! Transient toasts are overlaid at the **horizontal center** of the bar
//! (not in the right cluster), so short status like "Opening Terminal…"
//! reads as bar-level feedback rather than a trailing status item.
//!
//! Type matches macOS menu bar: one chrome face throughout (labels, stats,
//! clock). Focused-app name is bold (macOS application menu title). Colours
//! come from the live theme palette (no view-local hex). Mono is for
//! code/detail panels, not menubar status values.
//!
//! Hit targets are full bar height ([`BAR_H`] = window height) so a pointer
//! at y=0 on the screen still activates a menu title.

use iced::widget::{container, mouse_area, row, stack, text};
use iced::{Alignment, Color, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::icon_colored;
use sola_kit::fonts;

use crate::app::Msg;
use crate::components::clock::clock_widget;
use crate::components::toast::toast_widget;
use crate::menu::state::synthesized_menu;
use crate::menubar::{
    EXTRA_PAD_H, FlashTarget, ICON_SIZE, MENU_PAD_H, PHRASE_GAP, STAT_INNER_SPACING, STAT_PAD_H,
    WINDOW_HEIGHT,
};

// ── Density (logical px) ────────────────────────────────────────────────
// macOS menu bar ~13pt system chrome, regular weight; app name bold.
// Left titles keep MENU_PAD_H. The right cluster is four phrases (extras,
// percents, rates, clock): tight pad inside a phrase, PHRASE_GAP between.
// Bar height stays at `WINDOW_HEIGHT` (28) for zoning.
const CHROME_SIZE: f32 = 13.0;
/// Full window height — every interactive label uses this so the hit box
/// reaches the top edge of the screen (y=0), not a short centred chip.
const BAR_H: f32 = WINDOW_HEIGHT as f32;
fn menu_pad() -> Padding {
    Padding {
        top: 0.0,
        right: MENU_PAD_H,
        bottom: 0.0,
        left: MENU_PAD_H,
    }
}
fn extra_pad() -> Padding {
    Padding {
        top: 0.0,
        right: EXTRA_PAD_H,
        bottom: 0.0,
        left: EXTRA_PAD_H,
    }
}
fn stat_pad() -> Padding {
    Padding {
        top: 0.0,
        right: STAT_PAD_H,
        bottom: 0.0,
        left: STAT_PAD_H,
    }
}
/// Optical nudge for the flower glyph (SVG visual center sits slightly
/// low relative to SF Pro Text cap height at 13pt).
const FLOWER_NUDGE_UP: f32 = 1.5;
/// Pixel graphs are a 14px LED matrix; iced centers the text layout box,
/// which hangs below the cap height, so the dots sit low. Lift to match.
const PIXEL_NUDGE_UP: f32 = 2.0;

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
    pad: Padding,
) -> iced::widget::Button<'a, Msg> {
    let centered = container(content.into())
        .width(Length::Shrink)
        .height(Length::Fill)
        .align_y(Alignment::Center);
    iced::widget::button(centered)
        .style(kit_btn::menubar(active))
        .padding(pad)
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
    let btn = bar_button(content, active, on_press, menu_pad());
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
    let title_active =
        (shell.menu_open && !shell.current_open_is_system && shell.current_open_index == Some(0))
            || flashing(shell, false, 0);
    let app_title: Element<'_, Msg> = if clickable {
        crate::menubar::report::ReportX::wrap(
            0,
            bar_item(
                text(app_title_str).font(app_title_font()).size(CHROME_SIZE),
                title_active,
                Msg::OpenMenu {
                    index: 0,
                    is_system: false,
                },
                Some(Msg::HoverMenu {
                    index: 0,
                    is_system: false,
                }),
            ),
        )
    } else {
        container(text(app_title_str).font(app_title_font()).size(CHROME_SIZE))
            .padding(menu_pad())
            .height(Length::Fixed(BAR_H))
            .align_y(Alignment::Center)
            .into()
    };

    // ── App-menu labels (menus[1..]) ──────────────────────────────────
    let menu_labels: Vec<Element<'_, Msg>> = app_menu_labels(shell);

    // ── Right cluster: stats + clock (toast is centered overlay) ─────
    let clock_active = panel_active(shell, crate::app::Panel::Calendar);
    let clock: Element<'_, Msg> = bar_button(
        clock_widget(&mb.clock_now),
        clock_active,
        Msg::ToggleCalendar,
        menu_pad(),
    )
    .into();

    // ── System-stat indicators (left of clock) ───────────────────────
    let cpu_btn: Element<'_, Msg> = bar_button(
        stat_graph(
            "CPU",
            shell.cpu_hist.to_vec(),
            100.0,
            crate::stats::pixel::Tint::Level,
            muted,
        ),
        panel_active(shell, crate::app::Panel::Stat(crate::stats::Metric::Cpu)),
        Msg::ToggleStatPanel(crate::stats::Metric::Cpu),
        stat_pad(),
    )
    .into();

    let mem_btn: Element<'_, Msg> = bar_button(
        stat_graph(
            "MEM",
            shell.mem_hist.to_vec(),
            100.0,
            crate::stats::pixel::Tint::Level,
            muted,
        ),
        panel_active(shell, crate::app::Panel::Stat(crate::stats::Metric::Mem)),
        Msg::ToggleStatPanel(crate::stats::Metric::Mem),
        stat_pad(),
    )
    .into();

    let rx_peak = shell.net_down_hist.peak().max(1.0);
    let tx_peak = shell.net_up_hist.peak().max(1.0);
    let rx_btn: Element<'_, Msg> = bar_button(
        stat_graph(
            "RX",
            shell.net_down_hist.to_vec(),
            rx_peak,
            crate::stats::pixel::Tint::Rx,
            muted,
        ),
        panel_active(shell, crate::app::Panel::Stat(crate::stats::Metric::Rx)),
        Msg::ToggleStatPanel(crate::stats::Metric::Rx),
        stat_pad(),
    )
    .into();
    let tx_btn: Element<'_, Msg> = bar_button(
        stat_graph(
            "TX",
            shell.net_up_hist.to_vec(),
            tx_peak,
            crate::stats::pixel::Tint::Tx,
            muted,
        ),
        panel_active(shell, crate::app::Panel::Stat(crate::stats::Metric::Tx)),
        Msg::ToggleStatPanel(crate::stats::Metric::Tx),
        stat_pad(),
    )
    .into();

    // ── Assemble ──────────────────────────────────────────────────────
    let mut left = vec![system_btn, app_title];
    left.extend(menu_labels);

    let mut extras: Vec<Element<'_, Msg>> = Vec::new();
    if let Some(unread) = shell.mail_unread_badge() {
        let accent = shell.theme.extended_palette().primary.base.color;
        extras.push(mail_unread_chip(unread, accent));
    }
    if shell.notify.pile_count() > 0 {
        let tint = if shell.notify.unseen {
            shell.theme.extended_palette().primary.base.color
        } else {
            fg
        };
        extras.push(notify_pile_chip(tint));
    }
    if let Some(icon) = crate::audio::bar_icon(&shell.audio.snapshot) {
        extras.push(audio_chip(
            icon,
            fg,
            muted,
            panel_active(shell, crate::app::Panel::Audio),
        ));
    }
    if let Some(icon) = crate::bluetooth::bar_icon(&shell.bluetooth.snapshot) {
        extras.push(bluetooth_chip(
            icon,
            fg,
            muted,
            panel_active(shell, crate::app::Panel::Bluetooth),
        ));
    }

    let mut percents: Vec<Element<'_, Msg>> = vec![cpu_btn];
    if shell.stats.gpu.is_some() {
        percents.push(
            bar_button(
                stat_graph(
                    "GPU",
                    shell.gpu_hist.to_vec(),
                    100.0,
                    crate::stats::pixel::Tint::Level,
                    muted,
                ),
                panel_active(shell, crate::app::Panel::Stat(crate::stats::Metric::Gpu)),
                Msg::ToggleStatPanel(crate::stats::Metric::Gpu),
                stat_pad(),
            )
            .into(),
        );
    }
    percents.push(mem_btn);

    // Four phrases, not nine beads: extras · percents · rates · clock.
    let mut phrases: Vec<Element<'_, Msg>> = Vec::new();
    if !extras.is_empty() {
        phrases.push(row(extras).height(Length::Fixed(BAR_H)).into());
    }
    phrases.push(row(percents).height(Length::Fixed(BAR_H)).into());
    phrases.push(row![rx_btn, tx_btn].height(Length::Fixed(BAR_H)).into());
    phrases.push(clock);

    // Base chrome: left menus | flexible gap | right phrases.
    // Fixed BAR_H root — matches the window size so children with Fixed(BAR_H)
    // are not shrink-wrapped / vertically centred with a dead band at y=0.
    let base: Element<'_, Msg> = row![
        row(left).height(Length::Fixed(BAR_H)),
        iced::widget::Space::new().width(Length::Fill),
        iced::widget::row(phrases)
            .spacing(PHRASE_GAP)
            .height(Length::Fixed(BAR_H)),
    ]
    .width(Length::Fill)
    .height(Length::Fixed(BAR_H))
    .into();

    // Toast sits in the true horizontal center of the bar (window mid-line),
    // layered above left/right chrome so asymmetric clusters don't pull it
    // off center. Only stacked when a message is active so the fill overlay
    // cannot sit idle over the chrome.
    let bar: Element<'_, Msg> = if mb.toast.is_some() {
        let toast_layer: Element<'_, Msg> = container(toast_widget(mb.toast.as_deref()))
            .width(Length::Fill)
            .height(Length::Fixed(BAR_H))
            .center_x(Length::Fill)
            .align_y(Alignment::Center)
            .into();
        stack![base, toast_layer]
            .width(Length::Fill)
            .height(Length::Fixed(BAR_H))
            .into()
    } else {
        base
    };

    container(bar)
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
            if !first.label.is_empty() {
                return first.label.clone();
            }
        }
    }
    // Empty host app_id (pre-inference gamescope) — try gamescope menu.
    if app_id.is_empty() {
        if let Some(payload) = shell.menus.get_menu("gamescope") {
            if let Some(first) = payload.menus.first() {
                if !first.label.is_empty() {
                    return first.label.clone();
                }
            }
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

/// Chip highlight only while that panel is actually showing. `open_panel`
/// can lag `menu_open` on chord/focus dismiss — requiring both stops the
/// volume (and other) chips from staying filled after the popover is gone.
fn panel_active(shell: &crate::app::Shell, panel: crate::app::Panel) -> bool {
    shell.menu_open && shell.open_panel == Some(panel)
}

fn app_menu_labels(shell: &crate::app::Shell) -> Vec<Element<'_, Msg>> {
    let Some(ref app_id) = shell.focused_app_id else {
        return Vec::new();
    };

    let payload = shell.effective_app_menu(app_id);

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
            crate::menubar::report::ReportX::wrap(
                index,
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
                ),
            )
        })
        .collect()
}

fn display_label(shell: &crate::app::Shell, app_id: &str) -> String {
    if let Some(app) = shell.applications.get_for_window(app_id) {
        return app.label.clone();
    }
    // gamescope sometimes maps with empty app_id before river infers it —
    // still prefer the Arcade-published gamescope catalog label.
    if app_id.is_empty() {
        if let Some(app) = shell.applications.get_for_window("gamescope") {
            return app.label.clone();
        }
        // Fall back to a non-empty window title if we have one.
        if let Some(t) = shell
            .known_windows
            .iter()
            .find(|w| w.app_id.is_empty() && !w.title.is_empty())
            .map(|w| w.title.clone())
        {
            return t;
        }
        return String::new();
    }
    let mut chars = app_id.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Status indicator: muted chrome label + fixed btop-style pixel graph.
fn stat_graph<'a>(
    label: &'a str,
    samples: Vec<f32>,
    max: f32,
    tint: crate::stats::pixel::Tint,
    muted: Color,
) -> Element<'a, Msg> {
    row![
        text(label)
            .font(fonts::chrome())
            .size(CHROME_SIZE)
            .style(move |_: &Theme| iced::widget::text::Style { color: Some(muted) }),
        container(crate::stats::pixel::graph(samples, max, tint)).padding(Padding {
            top: 0.0,
            right: 0.0,
            bottom: PIXEL_NUDGE_UP,
            left: 0.0,
        }),
    ]
    .spacing(STAT_INNER_SPACING)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

/// Inbox unread: mail glyph + accent count. Hidden by the caller when
/// mail is closed or the count is zero.
fn mail_unread_chip(unread: u32, accent: Color) -> Element<'static, Msg> {
    let label = if unread > 99 {
        "99+".to_string()
    } else {
        unread.to_string()
    };
    bar_button(
        row![
            icon_colored("lucide/mail", ICON_SIZE, accent),
            text(label)
                .font(fonts::chrome())
                .size(CHROME_SIZE)
                .style(move |_: &Theme| iced::widget::text::Style {
                    color: Some(accent)
                }),
        ]
        .spacing(STAT_INNER_SPACING)
        .align_y(Alignment::Center),
        false,
        Msg::RaiseMail,
        extra_pad(),
    )
    .into()
}

fn audio_chip(
    icon: crate::audio::BarIcon,
    fg: Color,
    muted: Color,
    active: bool,
) -> Element<'static, Msg> {
    let tint = if icon.muted { muted } else { fg };
    bar_button(
        icon_colored(icon.name, ICON_SIZE, tint),
        active,
        Msg::ToggleAudio,
        extra_pad(),
    )
    .into()
}

fn bluetooth_chip(
    icon: crate::bluetooth::BarIcon,
    fg: Color,
    muted: Color,
    active: bool,
) -> Element<'static, Msg> {
    let tint = if icon.muted { muted } else { fg };
    bar_button(
        icon_colored(icon.name, ICON_SIZE, tint),
        active,
        Msg::ToggleBluetooth,
        extra_pad(),
    )
    .into()
}

fn notify_pile_chip(accent: Color) -> Element<'static, Msg> {
    bar_button(
        icon_colored("lucide/bell", ICON_SIZE, accent),
        false,
        Msg::ToggleNotifyPile,
        extra_pad(),
    )
    .into()
}
