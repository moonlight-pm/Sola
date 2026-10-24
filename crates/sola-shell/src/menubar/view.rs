//! Menubar window view: the top row of islands (Next board 03, "a bubble
//! per group").
//!
//! Layout (left-to-right), a gap in from every edge, the ground between:
//!   (✿) (1 Main  2 Mail ◆)        (toast)        (✉ 🔔 ᛒ) (spectrum)
//!                                            (CPU GPU MEM) (RX TX) (clock)
//!
//! Each group is one island: [`ISLAND_H`] tall, a pill, [`ISLAND_GAP`]
//! apart, with a lit top edge. An island whose panel is showing lifts a
//! step. Its items are the hit targets (each opens its own panel). App
//! menus are not here: each app carries its own in its window's titlebar
//! (kit `window_frame`, Next slice 6).
//!
//! Colours come from the live theme palette; the flower is Sola's amber.

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

/// Sola's amber: the flower is the brand mark, not a theme atom.
const SOL: Color = Color::from_rgb(1.0, 0.722, 0.0);
/// 14px lucide/SVG and LED matrices: iced centers the layout box, which
/// hangs below 13pt cap height, so marks sit low. Same lift for icons
/// and pixel graphs.
const CHROME_NUDGE_UP: f32 = 2.0;

/// An island of the top row: a pill on the ground with a lit top edge,
/// lifted a step while `open` (its panel shows). `pad_h` either side of
/// its content.
pub(super) fn island<'a>(
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
pub(super) fn item<'a>(content: impl Into<Element<'a, Msg>>, on_press: Msg) -> Element<'a, Msg> {
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
                container(chrome_nudge(icon_colored("sola/flower", ICON_SIZE, SOL)))
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
    let left = row![flower].align_y(Alignment::Center);

    // ── Right: an island per group ───────────────────────────────────
    let mut islands: Vec<Element<'_, Msg>> = Vec::new();

    let mut extras: Vec<Element<'_, Msg>> = Vec::new();
    if let Some(unread) = shell.mail_unread_badge() {
        let accent = shell.theme.extended_palette().primary.base.color;
        extras.push(item(extra_badge("lucide/mail", unread, accent), Msg::RaiseMail));
    }
    if shell.notify.pile_count() > 0 {
        let tint = if shell.notify.unseen {
            shell.theme.extended_palette().primary.base.color
        } else {
            fg
        };
        extras.push(item(
            extra_badge("lucide/bell", shell.notify.pile_count(), tint),
            Msg::ToggleNotifyPile,
        ));
    }
    // Left of Bluetooth: panel anchors walk right-to-left, so nothing
    // anchored moves when this comes and goes.
    if let Some(chip) = crate::mouse::bar_chip(&shell.mouse) {
        extras.push(mouse_chip(chip, fg, muted));
    }
    if let Some(icon) = crate::bluetooth::bar_icon(&shell.bluetooth.snapshot) {
        let tint = if icon.muted { muted } else { fg };
        extras.push(item(extra_icon(icon.name, tint), Msg::ToggleBluetooth));
    }
    let extras_open = panel_active(shell, crate::app::Panel::Bluetooth)
        || panel_active(shell, crate::app::Panel::NotifyPile);
    if !extras.is_empty() {
        islands.push(island(items(extras), extras_open, ISLAND_PAD_H));
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
    }
    meters.push(stat("MEM", shell.mem_hist.to_vec(), 100.0, Tint::Level, Metric::Mem));
    islands.push(island(
        items(meters),
        stat_open(&[Metric::Cpu, Metric::Gpu, Metric::Mem]),
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

    // The toast is the one live thing, an island in the true center of the
    // row (layered over it, so uneven sides do not pull it off center).
    let bar: Element<'_, Msg> = if mb.toast.is_some() {
        let toast = container(island(toast_widget(mb.toast.as_deref()), false, ISLAND_PAD_H))
            .width(Length::Fill)
            .height(Length::Fixed(ISLAND_H))
            .center_x(Length::Fill);
        stack![base, toast]
            .width(Length::Fill)
            .height(Length::Fixed(ISLAND_H))
            .into()
    } else {
        base
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

/// Lucide extras: 14px glyph, optically lifted with the LED matrices.
fn extra_icon(name: &str, color: Color) -> Element<'static, Msg> {
    chrome_nudge(icon_colored(name, ICON_SIZE, color))
}

/// Glyph + chrome count, lifted as a pair so the numeral sits on the
/// same band as CPU and the icon matches a lone extra (bell / bluetooth).
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
fn mouse_chip(chip: crate::mouse::BarChip, fg: Color, muted: Color) -> Element<'static, Msg> {
    use crate::mouse::Tone;
    let color = match chip.tone {
        Tone::Normal => fg,
        Tone::Low => crate::stats::AMBER,
        Tone::Critical => crate::stats::RED,
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
