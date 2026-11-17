//! Menubar window view: the top row of islands (Next board 03, "a bubble
//! per group").
//!
//! Layout (left-to-right), a gap in from every edge, the ground between:
//!   (✿) (1 Main) (2 Mail ◆)     (toast)     (◆ ✉ 3) (🖱 80% ᛒ) (spectrum)
//!                                       (CPU GPU VRAM MEM) (RX TX) (clock)
//! The workspaces are here only when set to be islands ([`super::workspaces`]).
//!
//! Each group is one island: [`ISLAND_H`] tall, a pill, [`ISLAND_GAP`]
//! apart, with a lit top edge. An island whose panel is showing lifts a
//! step. Its items are the hit targets (each opens its own panel). App
//! menus are not here: each app carries its own in its window's titlebar
//! (kit `window_frame`, Next slice 6).
//!
//! Colours come from the live theme palette; the flower is its own
//! `shell-flower` token (Sola's amber unless a look recolors it).

use iced::widget::{Space, container, mouse_area, row, stack, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::icon_colored;
use sola_kit::components::style::{linear_bg, mix, mix_white};
use sola_kit::fonts;

use crate::app::Msg;
use crate::components::clock::clock_widget;
use crate::components::toast::toast_widget;
use crate::menubar::{
    CHROME_SIZE, EDGE, FlashTarget, ICON_SIZE, ISLAND_GAP, ISLAND_H, ISLAND_PAD_H, ITEM_GAP,
    SPECTRUM_PAD_H, STAT_INNER_SPACING, WINDOW_HEIGHT,
};

/// 14px lucide/SVG and LED matrices: iced centers the layout box, which
/// hangs below 13pt cap height, so marks sit low. Same lift for icons
/// and pixel graphs.
const CHROME_NUDGE_UP: f32 = 2.0;

/// An island of the top row: a pill on the ground with a lit top edge,
/// lifted a step while `open` (its panel shows). `pad_h` either side of
/// its content.
pub(crate) fn island<'a>(
    content: impl Into<Element<'a, Msg>>,
    open: bool,
    pad_h: f32,
) -> Element<'a, Msg> {
    // The face starts 1 px down (the rim); a pixel off its bottom centers
    // the content on the whole pill.
    let face = container(content.into())
        .padding(Padding {
            top: 0.0,
            right: pad_h,
            bottom: 1.0,
            left: pad_h,
        })
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .style(move |theme: &Theme| {
            let fill = island_fill(theme, open);
            let sheen = if open { 0.08 } else { 0.04 };
            container::Style {
                background: Some(linear_bg(
                    180.0,
                    &[(0.0, mix_white(fill, sheen)), (0.55, fill), (1.0, fill)],
                )),
                border: Border {
                    radius: (ISLAND_H / 2.0).into(),
                    ..Border::default()
                },
                ..container::Style::default()
            }
        });
    // The rim shows as the 1 px the face leaves at the top: thin over the
    // middle, gone down the round ends — an inset highlight.
    container(face)
        .padding(Padding {
            top: 1.0,
            ..Padding::ZERO
        })
        .height(Length::Fixed(ISLAND_H))
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(mix_white(island_fill(theme, open), 0.12))),
            border: Border {
                radius: (ISLAND_H / 2.0).into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// An island's face: between the raised and hover greys at rest, the
/// hover grey while open.
fn island_fill(theme: &Theme, open: bool) -> Color {
    let p = theme.extended_palette();
    if open {
        p.background.strong.color
    } else {
        mix(p.background.weak.color, p.background.strong.color, 0.5)
    }
}

/// An item inside an island: its full height is the hit target, nothing
/// drawn (the island shows open).
pub(crate) fn item<'a>(content: impl Into<Element<'a, Msg>>, on_press: Msg) -> Element<'a, Msg> {
    let centered = container(content.into())
        .height(Length::Fill)
        .align_y(Alignment::Center);
    iced::widget::button(centered)
        .style(|theme: &Theme, _status| iced::widget::button::Style {
            background: None,
            text_color: theme.palette().text,
            border: Border::default(),
            shadow: Default::default(),
            snap: false,
        })
        .padding(0)
        .height(Length::Fill)
        .on_press(on_press)
        .into()
}

/// Render the menubar for `shell`.
pub fn view(shell: &crate::app::Shell) -> Element<'_, Msg> {
    let mb = &shell.menubar;
    let fg = shell.theme.palette().text;
    // Slightly quiet prefix ("CPU") so the value carries the scan weight —
    // same face/size as the value, just lower opacity (not a second size).
    let muted = Color { a: 0.62, ..fg };

    // ── Left: the flower ─────────────────────────────────────────────
    let system_open =
        (shell.menu_open && shell.current_open_is_system) || flashing(shell, true, 0);
    let flower = mouse_area(
        container(island(
            item(
                container(chrome_nudge(icon_colored(
                    "sola/flower",
                    ICON_SIZE,
                    shell.style.flower,
                )))
                .width(Length::Fill)
                .align_x(Alignment::Center),
                Msg::OpenMenu {
                    index: 0,
                    is_system: true,
                },
            ),
            system_open,
            0.0,
        ))
        .width(Length::Fixed(ISLAND_H)),
    )
    .on_enter(Msg::HoverMenu {
        index: 0,
        is_system: true,
    });
    // Then the workspaces, when they are islands, and an island per agent
    // session that asks or has finished.
    let left = row![flower]
        .push(crate::menubar::workspaces::islands(shell))
        .push(crate::menubar::workspaces::separator(shell))
        .extend(crate::agents::view::row_islands(shell))
        .spacing(ISLAND_GAP)
        .align_y(Alignment::Center);

    // ── Right: an island per group ───────────────────────────────────
    let mut islands: Vec<Element<'_, Msg>> = Vec::new();

    // Their widths, for where the right-hand islands begin.
    let text_w = |s: &str| fonts::line_width(s, CHROME_SIZE, fonts::chrome());
    let glyph_and = |label: &str| f32::from(ICON_SIZE) + STAT_INNER_SPACING + text_w(label);
    let accent = shell.theme.extended_palette().primary.base.color;

    // Notifications: an icon per app with news since you were last in it
    // (a Sola app's glyph, a web app's favicon), the one lit last at the
    // left; then mail, there for its unread count too, lit or not.
    let mut notes: Vec<Element<'_, Msg>> = Vec::new();
    let mut notes_w: Vec<f32> = Vec::new();
    for lit in shell.lit_apps() {
        let name = shell.app_icon(&lit.app_id);
        notes.push(item(extra_icon(name, accent), Msg::RaiseLit(lit.app_id.clone())));
        notes_w.push(f32::from(ICON_SIZE));
    }
    if let Some(unread) = shell.mail_unread_badge() {
        let tint = if shell.notify.lit(crate::app::MAIL_APP).is_some() {
            accent
        } else {
            fg
        };
        let mail = Msg::RaiseLit(crate::app::MAIL_APP.into());
        notes.push(item(extra_badge("lucide/mail", unread, tint), mail));
        notes_w.push(glyph_and(&sola_kit::components::count_label(unread)));
    }

    // Devices: the mouse's battery left of Bluetooth (panel anchors walk
    // right-to-left, so nothing anchored moves when it comes and goes).
    let mut devices: Vec<Element<'_, Msg>> = Vec::new();
    let mut devices_w: Vec<f32> = Vec::new();
    if let Some(chip) = crate::mouse::bar_chip(&shell.mouse) {
        devices_w.push(glyph_and(&chip.label));
        devices.push(mouse_chip(chip, fg, muted, &shell.theme));
    }
    if let Some(icon) = crate::bluetooth::bar_icon(&shell.bluetooth.snapshot) {
        let tint = if icon.muted { muted } else { fg };
        devices.push(item(extra_icon(icon.name, tint), Msg::ToggleBluetooth));
        devices_w.push(f32::from(ICON_SIZE));
    }

    if !notes.is_empty() {
        islands.push(island(items(notes), false, ISLAND_PAD_H));
    }
    if !devices.is_empty() {
        let open = panel_active(shell, crate::app::Panel::Bluetooth);
        islands.push(island(items(devices), open, ISLAND_PAD_H));
    }

    // Alone in its island: centered as is (no lift to a text band), inset
    // the same all round so its round ends follow the pill's.
    if let Some(icon) = crate::audio::bar_icon(&shell.audio.snapshot) {
        islands.push(island(
            item(
                crate::audio::wave::visualizer(icon.muted, &shell.theme),
                Msg::ToggleAudio,
            ),
            panel_active(shell, crate::app::Panel::Audio),
            SPECTRUM_PAD_H,
        ));
    }

    use crate::stats::Metric;
    use crate::stats::pixel::Tint;
    let stat = |label: &'static str, hist: Vec<f32>, max: f32, tint: Tint, metric: Metric| {
        item(
            stat_graph(label, hist, max, tint, muted, &shell.theme),
            Msg::ToggleStatPanel(metric),
        )
    };
    let stat_open = |metrics: &[Metric]| {
        metrics
            .iter()
            .any(|m| panel_active(shell, crate::app::Panel::Stat(*m)))
    };
    let mut meters = vec![stat("CPU", shell.cpu_hist.to_vec(), 100.0, Tint::Level, Metric::Cpu)];
    if shell.stats.gpu.is_some() {
        meters.push(stat("GPU", shell.gpu_hist.to_vec(), 100.0, Tint::Level, Metric::Gpu));
        meters.push(stat("VRAM", shell.vram_hist.to_vec(), 100.0, Tint::Level, Metric::Vram));
    }
    meters.push(stat("MEM", shell.mem_hist.to_vec(), 100.0, Tint::Level, Metric::Mem));
    islands.push(island(
        items(meters),
        stat_open(&[Metric::Cpu, Metric::Gpu, Metric::Vram, Metric::Mem]),
        ISLAND_PAD_H,
    ));

    let rx_peak = shell.net_down_hist.peak().max(1.0);
    let tx_peak = shell.net_up_hist.peak().max(1.0);
    let net = vec![
        stat("RX", shell.net_down_hist.to_vec(), rx_peak, Tint::Rx, Metric::Rx),
        stat("TX", shell.net_up_hist.to_vec(), tx_peak, Tint::Tx, Metric::Tx),
    ];
    islands.push(island(
        items(net),
        stat_open(&[Metric::Rx, Metric::Tx]),
        ISLAND_PAD_H,
    ));

    islands.push(island(
        item(clock_widget(&mb.clock_now), Msg::ToggleCalendar),
        panel_active(shell, crate::app::Panel::Calendar),
        ISLAND_PAD_H,
    ));

    let base: Element<'_, Msg> = row![
        left,
        Space::new().width(Length::Fill),
        row(islands).spacing(ISLAND_GAP).align_y(Alignment::Center),
    ]
    .width(Length::Fill)
    .height(Length::Fixed(ISLAND_H))
    .align_y(Alignment::Center)
    .into();

    // The toast is the one live thing, an island in the middle of the row
    // (layered over it, so uneven sides do not pull it off center): centred,
    // unless that runs into the right-hand islands.
    let middle = mb.toast.as_deref().map(|message| {
        let w = text_w(message) + 2.0 * ISLAND_PAD_H;
        let toast = island(toast_widget(Some(message)), false, ISLAND_PAD_H);
        // Where the right-hand islands begin: left from the devices
        // island's right edge, past it and the notifications island.
        let devices_right = shell.right_edges().bluetooth + ISLAND_PAD_H + f32::from(ICON_SIZE);
        let right_start = [&devices_w, &notes_w]
            .into_iter()
            .filter(|w| !w.is_empty())
            .fold(devices_right + ISLAND_GAP, |x, w| x - ISLAND_GAP - island_w(w));
        let output_w = shell.output_size.map_or(1920.0, |(w, _)| w as f32);
        let left_end = crate::agents::view::left_group_end(shell);
        let x = place(output_w, w, left_end, right_start);
        let pad = Padding {
            left: (x - EDGE).max(0.0),
            ..Padding::ZERO
        };
        container(toast).padding(pad).width(Length::Fill)
    });
    let bar: Element<'_, Msg> = match middle {
        Some(middle) => {
            let middle = container(middle)
                .width(Length::Fill)
                .height(Length::Fixed(ISLAND_H));
            stack![base, middle]
                .width(Length::Fill)
                .height(Length::Fixed(ISLAND_H))
                .into()
        }
        None => base,
    };

    container(bar)
        .padding(Padding {
            top: EDGE,
            right: EDGE,
            bottom: 0.0,
            left: EDGE,
        })
        .width(Length::Fill)
        .height(Length::Fixed(WINDOW_HEIGHT as f32))
        .into()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The middle island's left edge on an output `output_w` wide: centred,
/// unless that comes within a gap of the right-hand islands (from
/// `right_start`, a narrow output); then just left of them, though never
/// over the left group (to `left_end`).
fn place(output_w: f32, width: f32, left_end: f32, right_start: f32) -> f32 {
    let centred = (output_w - width) / 2.0;
    centred
        .min(right_start - ISLAND_GAP - width)
        .max(left_end + ISLAND_GAP)
}

/// An island's width around items `widths` wide.
fn island_w(widths: &[f32]) -> f32 {
    let gaps = ITEM_GAP * widths.len().saturating_sub(1) as f32;
    2.0 * ISLAND_PAD_H + widths.iter().sum::<f32>() + gaps
}

/// An island's items, [`ITEM_GAP`] apart.
fn items(items: Vec<Element<'_, Msg>>) -> Element<'_, Msg> {
    row(items)
        .spacing(ITEM_GAP)
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .into()
}

fn flashing(shell: &crate::app::Shell, is_system: bool, index: usize) -> bool {
    shell.menubar.flash == Some(FlashTarget { is_system, index })
}

/// An island shows open only while its panel is actually showing.
/// `open_panel` can lag `menu_open` on chord/focus dismiss — requiring
/// both stops an island from staying lifted after the popover is gone.
fn panel_active(shell: &crate::app::Shell, panel: crate::app::Panel) -> bool {
    shell.menu_open && shell.open_panel == Some(panel)
}

/// Status indicator: muted chrome label + fixed btop-style pixel graph.
fn stat_graph<'a>(
    label: &'a str,
    samples: Vec<f32>,
    max: f32,
    tint: crate::stats::pixel::Tint,
    muted: Color,
    theme: &Theme,
) -> Element<'a, Msg> {
    row![
        text(label)
            .font(fonts::chrome())
            .size(CHROME_SIZE)
            .style(move |_: &Theme| iced::widget::text::Style { color: Some(muted) }),
        container(crate::stats::pixel::graph(samples, max, tint, theme)).padding(Padding {
            bottom: CHROME_NUDGE_UP,
            ..Padding::ZERO
        }),
    ]
    .spacing(STAT_INNER_SPACING)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

/// A 14px glyph (or favicon), optically lifted with the LED matrices.
fn extra_icon(name: &str, color: Color) -> Element<'static, Msg> {
    chrome_nudge(icon_colored(name, ICON_SIZE, color))
}

/// Glyph + chrome count, lifted as a pair so the numeral sits on the
/// same band as CPU and the icon matches a lone extra (an app, bluetooth).
fn extra_badge(name: &'static str, count: u32, color: Color) -> Element<'static, Msg> {
    let label = sola_kit::components::count_label(count);
    chrome_nudge(
        row![
            icon_colored(name, ICON_SIZE, color),
            text(label)
                .font(fonts::chrome())
                .size(CHROME_SIZE)
                .style(move |_: &Theme| iced::widget::text::Style { color: Some(color) }),
        ]
        .spacing(STAT_INNER_SPACING)
        .align_y(Alignment::Center),
    )
}

fn chrome_nudge<'a>(content: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    container(content.into())
        .padding(Padding {
            bottom: CHROME_NUDGE_UP,
            ..Padding::ZERO
        })
        .into()
}

/// Mouse glyph + battery percent. Not a button: there is no panel.
fn mouse_chip(
    chip: crate::mouse::BarChip,
    fg: Color,
    muted: Color,
    theme: &Theme,
) -> Element<'static, Msg> {
    use crate::mouse::Tone;
    let p = theme.extended_palette();
    let color = match chip.tone {
        Tone::Normal => fg,
        Tone::Low => p.warning.base.color,
        Tone::Critical => p.danger.base.color,
        Tone::Stale => muted,
    };
    container(chrome_nudge(
        row![
            icon_colored(chip.icon, ICON_SIZE, color),
            text(chip.label)
                .font(fonts::chrome())
                .size(CHROME_SIZE)
                .style(move |_: &Theme| iced::widget::text::Style { color: Some(color) }),
        ]
        .spacing(STAT_INNER_SPACING)
        .align_y(Alignment::Center),
    ))
    .height(Length::Fill)
    .align_y(Alignment::Center)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toast_is_centred_unless_the_right_hand_islands_are_in_the_way() {
        // A wide output: the middle.
        assert_eq!(place(1920.0, 100.0, 40.0, 1200.0), 910.0);
        // A narrow one: just left of the meters.
        assert_eq!(place(1280.0, 100.0, 40.0, 600.0), 600.0 - ISLAND_GAP - 100.0);
        // No room at all: it keeps off the flower.
        assert_eq!(place(400.0, 100.0, 40.0, 100.0), 40.0 + ISLAND_GAP);
    }
}
