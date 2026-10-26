//! Stat detail dropdown panels, rendered in the Menu window.
//!
//! Each panel draws its island's LEDs at the panel's size
//! ([`pixel::PANEL`]): the last minute as columns each tinted by its own
//! sample, per-thread load as a shape (threads a gap apart, cores two),
//! processes and memory as LED bars. The value you came for, names and
//! figures are type (the value was a dot readout until Joshua: "i don't
//! like the 'digital' number"). Every color is the theme's: ink at rest, its warning
//! past 75% and danger past 90% ([`crate::stats::level_color`]), RX in the
//! accent and TX in success as the islands draw them. Design:
//! `design/instrument/panels.html`.

use iced::widget::{Space, column, container, row, text};
use iced::{Alignment, Color, Element, Length, Padding, Theme};
use sola_kit::components::popover;
use sola_kit::components::style::{HAIRLINE_A, mix, mix_white};
use sola_kit::fonts;

use crate::app::{Msg, Shell};
use crate::stats::cpu::Proc;
use crate::stats::pixel::{self, PANEL};
use crate::stats::Metric;

/// 60 history columns on the panel grid (299 px) plus the card's pad.
pub const CARD_WIDTH: f32 = 324.0;
/// Live overlay height (header + graph + process list). Caps at usable area.
pub const CARD_HEIGHT: f32 = 420.0;

/// Inside the card.
const PAD: f32 = 12.0;
/// The last minute: a column a second, eight cells tall.
const HISTORY_COLS: usize = 60;
const HISTORY_ROWS: usize = 8;
/// Per-thread load: a column a thread, six cells tall.
const THREAD_ROWS: usize = 6;
/// A process's bar, and a whole-width bar (memory, VRAM).
const PROC_CELLS: usize = 16;
const WIDE_CELLS: usize = HISTORY_COLS;
/// The value you came for.
const VALUE_SIZE: f32 = 28.0;
/// How far an unlit cell is from its surface toward the ink.
const UNLIT: f32 = 0.16;

/// The panel's inks: labels and captions dim, figures a step brighter.
/// Unlit cells are the ink 16% of the way into the surface under them,
/// opaque: a translucent white dot reads about three times brighter
/// through iced's linear blending (the kit's hairline note), and at the
/// panel's 4 px the field would drown the value.
struct Inks {
    text: Color,
    dim: Color,
    figure: Color,
    unlit: Color,
    rule: Color,
}

fn inks(theme: &Theme) -> Inks {
    let p = theme.extended_palette();
    let text = p.background.base.text;
    Inks {
        text,
        dim: Color { a: 0.55, ..text },
        figure: Color { a: 0.78, ..text },
        unlit: mix(text, p.background.weaker.color, UNLIT),
        rule: mix_white(p.background.weaker.color, HAIRLINE_A),
    }
}

/// Build the panel for `metric`, dropped under its menubar indicator, over a
/// dismiss backdrop. Mirrors `crate::menu::view`'s anchored dropdown.
pub fn panel(shell: &Shell, metric: Metric) -> Element<'_, Msg> {
    let card = match metric {
        Metric::Cpu => cpu_card(shell),
        Metric::Gpu => gpu_card(shell),
        Metric::Mem => mem_card(shell),
        Metric::Rx => rate_card(shell, Metric::Rx),
        Metric::Tx => rate_card(shell, Metric::Tx),
    };
    crate::menu::host_card(card)
}

// ---------------------------------------------------------------------------
// The card's parts
// ---------------------------------------------------------------------------

/// A caption: small caps, dim (the sidebar's section label).
fn caps<'a>(label: &str, ink: Color) -> Element<'a, Msg> {
    text(label.to_uppercase())
        .font(fonts::ui_medium())
        .size(10)
        .style(move |_: &Theme| iced::widget::text::Style { color: Some(ink) })
        .into()
}

/// A figure: mono, a step under full ink.
fn figure<'a>(value: String, ink: Color) -> iced::widget::Text<'a, Theme> {
    text(value)
        .font(fonts::MONO)
        .size(11)
        .style(move |_: &Theme| iced::widget::text::Style { color: Some(ink) })
}

/// The value, large, in the UI face, tinted like its meter.
fn value<'a>(value: &str, lit: Color) -> Element<'a, Msg> {
    text(value.to_string())
        .font(fonts::ui_medium())
        .size(VALUE_SIZE)
        .style(move |_: &Theme| iced::widget::text::Style { color: Some(lit) })
        .into()
}

/// Label and value on the left, what it is on the right.
fn header<'a>(
    label: &str,
    shown: &str,
    lit: Color,
    model: String,
    sub: String,
    ink: &Inks,
) -> Element<'a, Msg> {
    let figure_ink = ink.figure;
    row![
        column![caps(label, ink.dim), value(shown, lit)].spacing(2),
        Space::new().width(Length::Fill),
        column![
            text(model)
                .size(12)
                .style(move |_: &Theme| iced::widget::text::Style {
                    color: Some(figure_ink)
                }),
            figure(sub, ink.dim),
        ]
        .spacing(4)
        .align_x(iced::alignment::Horizontal::Right),
    ]
    .align_y(Alignment::Start)
    .into()
}

/// A section: its caption (and a figure at the right), then `body`.
fn section<'a>(label: &str, right: String, body: Element<'a, Msg>, ink: &Inks) -> Element<'a, Msg> {
    column![
        row![caps(label, ink.dim), Space::new().width(Length::Fill), figure(right, ink.dim)]
            .align_y(Alignment::Center),
        body,
    ]
    .spacing(7)
    .into()
}

fn rule<'a>(ink: &Inks) -> Element<'a, Msg> {
    let c = ink.rule;
    container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(move |_: &Theme| container::Style {
            background: Some(iced::Background::Color(c)),
            ..Default::default()
        })
        .into()
}

/// The last minute: a column per sample (oldest left), `max` at the top,
/// each column in `color_of(sample)`.
fn history<'a>(samples: &[f32], max: f32, color_of: impl Fn(f32) -> Color, ink: &Inks) -> Element<'a, Msg> {
    let s = pixel::last_n(samples, HISTORY_COLS);
    let max = max.max(1.0);
    let fills: Vec<f32> = s.iter().map(|v| v / max).collect();
    let colors: Vec<Color> = s.iter().map(|&v| color_of(v)).collect();
    pixel::columns(&fills, &colors, HISTORY_ROWS, PANEL, ink.unlit)
}

/// Ranked processes: the name, an LED bar against the first, the figure.
fn processes<'a>(rows: &[Proc], kind: &str, value: impl Fn(&Proc) -> String, ink: &Inks) -> Element<'a, Msg> {
    let max = rows.first().map_or(1.0, |p| p.value.max(f32::EPSILON));
    let name_ink = ink.figure;
    let list = column(rows.iter().map(|p| {
        row![
            text(p.name.clone())
                .size(12.5)
                .width(Length::Fill)
                .style(move |_: &Theme| iced::widget::text::Style {
                    color: Some(name_ink)
                }),
            pixel::bar(&[((p.value / max).clamp(0.0, 1.0), ink.text)], PROC_CELLS, PANEL, ink.unlit),
            figure(value(p), ink.figure)
                .width(Length::Fixed(52.0))
                .align_x(iced::alignment::Horizontal::Right),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .into()
    }))
    .spacing(6);
    section("Top processes", format!("by {kind}"), list.into(), ink)
}

/// Label–figure pairs along the foot, spread across the card.
fn foot<'a>(pairs: Vec<(&'static str, String)>, ink: &Inks) -> Element<'a, Msg> {
    let n = pairs.len();
    let mut r = row![].align_y(Alignment::Center);
    for (i, (label, value)) in pairs.into_iter().enumerate() {
        r = r.push(row![caps(label, ink.dim), figure(value, ink.figure)].spacing(8).align_y(Alignment::Center));
        if i + 1 < n {
            r = r.push(Space::new().width(Length::Fill));
        }
    }
    r.into()
}

fn card<'a>(parts: Vec<Element<'a, Msg>>) -> Element<'a, Msg> {
    popover(column(parts).spacing(14))
        .padding(Padding::new(PAD))
        .width(Length::Fixed(CARD_WIDTH))
        .into()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().copied().fold(0.0, f32::max)
}

// ---------------------------------------------------------------------------
// The panels
// ---------------------------------------------------------------------------

fn cpu_card(shell: &Shell) -> Element<'_, Msg> {
    let theme = &shell.theme;
    let ink = inks(theme);
    let s = &shell.stats;
    let id = crate::stats::cpu::identity();
    let level = |v: f32| crate::stats::level_color(v, theme);
    let samples = shell.cpu_hist.to_vec();

    let mut parts = vec![
        header(
            "CPU",
            &percent(s.cpu_pct),
            level(s.cpu_pct),
            id.model.clone(),
            format!("{}C / {}T", id.cores, id.threads),
            &ink,
        ),
        section(
            "Last 60 seconds",
            format!("peak {:.0}%", peak(&samples)),
            history(&samples, 100.0, level, &ink),
            &ink,
        ),
    ];

    if let Some(crate::stats::Detail::Cpu(d)) = &s.detail {
        // A column per thread, a gap between threads, two between
        // cores. No numbers: the shape is the point.
        let cols = crate::stats::cpu::by_core(&d.per_core, &id.groups);
        // NaN: no cells at all, so the cores stand apart even when idle.
        let fills: Vec<f32> = cols.iter().map(|c| c.map_or(f32::NAN, |v| v / 100.0)).collect();
        let colors: Vec<Color> = cols.iter().map(|c| level(c.unwrap_or(0.0))).collect();
        parts.push(section(
            "Per-thread load",
            format!("{} threads", d.per_core.len()),
            pixel::columns(&fills, &colors, THREAD_ROWS, PANEL, ink.unlit),
            &ink,
        ));
        parts.push(rule(&ink));
        parts.push(processes(&d.top, "CPU", |p| format!("{:.0}%", p.value), &ink));
        parts.push(rule(&ink));
        parts.push(foot(
            vec![
                ("Load", format!("{:.1}  {:.1}  {:.1}", d.load[0], d.load[1], d.load[2])),
                ("Up", fmt_uptime(d.uptime_secs)),
            ],
            &ink,
        ));
    }
    card(parts)
}

fn mem_card(shell: &Shell) -> Element<'_, Msg> {
    let theme = &shell.theme;
    let ink = inks(theme);
    let s = &shell.stats;
    let level = |v: f32| crate::stats::level_color(v, theme);
    let detail = match &s.detail {
        Some(crate::stats::Detail::Mem(d)) => Some(d),
        _ => None,
    };
    let total_gb = detail.map_or(0.0, |d| d.info.total_kb as f32 / 1024.0 / 1024.0);
    let samples = shell.mem_hist.to_vec();

    let mut parts = vec![
        header(
            "MEM",
            &percent(s.mem_pct),
            level(s.mem_pct),
            format!("{total_gb:.0} GB"),
            "RAM".into(),
            &ink,
        ),
        section(
            "Last 60 seconds",
            format!("peak {:.0}%", peak(&samples)),
            history(&samples, 100.0, level, &ink),
            &ink,
        ),
    ];

    if let Some(d) = detail {
        // Used lit, cache at 40%, free dark: one row of the panel's LEDs.
        let (used, cache, free) = d.info.segments_kb();
        let total = (used + cache + free).max(1) as f32;
        let cache_ink = Color { a: 0.4, ..ink.text };
        let gb = |kb: u64| format!("{:.1} GB", kb as f32 / 1024.0 / 1024.0);
        let legend = row![
            row![caps("Used", ink.dim), figure(gb(used), ink.figure)].spacing(6),
            row![caps("Cache", ink.dim), figure(gb(cache), ink.figure)].spacing(6),
            row![caps("Free", ink.dim), figure(gb(free), ink.figure)].spacing(6),
        ]
        .spacing(14)
        .align_y(Alignment::Center);
        let bar = pixel::bar(
            &[(used as f32 / total, ink.text), (cache as f32 / total, cache_ink)],
            WIDE_CELLS,
            PANEL,
            ink.unlit,
        );
        parts.push(section("Memory", String::new(), column![bar, legend].spacing(8).into(), &ink));
        parts.push(rule(&ink));
        parts.push(processes(&d.top, "RAM", |p| format!("{:.0} MB", p.value), &ink));
        parts.push(rule(&ink));
        let swap_used = d.info.swap_total_kb.saturating_sub(d.info.swap_free_kb) as f32 / 1024.0 / 1024.0;
        let swap_tot = d.info.swap_total_kb as f32 / 1024.0 / 1024.0;
        parts.push(foot(
            vec![
                ("Swap", format!("{swap_used:.1} / {swap_tot:.0} GB")),
                ("Pressure", format!("{:.0}%", s.mem_pct)),
            ],
            &ink,
        ));
    }
    card(parts)
}

fn gpu_card(shell: &Shell) -> Element<'_, Msg> {
    let theme = &shell.theme;
    let ink = inks(theme);
    let s = &shell.stats;
    let level = |v: f32| crate::stats::level_color(v, theme);
    let util = s.gpu.map_or(0.0, |g| g.util);
    let detail = match &s.detail {
        Some(crate::stats::Detail::Gpu(d)) => Some(d),
        _ => None,
    };
    let samples = shell.gpu_hist.to_vec();

    let mut parts = vec![
        header(
            "GPU",
            &percent(util),
            level(util),
            detail.map_or_else(|| "GPU".into(), |d| short_gpu(&d.name)),
            detail.map_or_else(String::new, |d| format!("{:.0} GB", d.mem_total_mb / 1024.0)),
            &ink,
        ),
        section(
            "Last 60 seconds",
            format!("peak {:.0}%", peak(&samples)),
            history(&samples, 100.0, level, &ink),
            &ink,
        ),
    ];

    if let Some(d) = detail {
        let frac = if d.mem_total_mb > 0.0 {
            d.mem_used_mb / d.mem_total_mb
        } else {
            0.0
        };
        parts.push(section(
            "VRAM",
            format!("{:.1} / {:.0} GB", d.mem_used_mb / 1024.0, d.mem_total_mb / 1024.0),
            pixel::bar(&[(frac, level(frac * 100.0))], WIDE_CELLS, PANEL, ink.unlit),
            &ink,
        ));
        if !d.top_gpu.is_empty() {
            parts.push(rule(&ink));
            parts.push(processes(&d.top_gpu, "GPU", |p| format!("{:.0}%", p.value), &ink));
        }
        if !d.top_vram.is_empty() {
            parts.push(rule(&ink));
            parts.push(processes(&d.top_vram, "VRAM", |p| format!("{:.0} MB", p.value), &ink));
        }
        parts.push(rule(&ink));
        parts.push(foot(
            vec![
                ("Temp", format!("{:.0}\u{00b0}C", d.temp_c)),
                ("Power", format!("{:.0} W", d.power_w)),
                ("Fan", format!("{:.0}%", d.fan_pct)),
            ],
            &ink,
        ));
    }
    card(parts)
}

/// One direction of the network: the rate in dots, the last minute scaled
/// to its own peak, the interface.
fn rate_card(shell: &Shell, metric: Metric) -> Element<'_, Msg> {
    let theme = &shell.theme;
    let ink = inks(theme);
    let p = theme.extended_palette();
    let (label, direction, rate, samples, color) = if metric == Metric::Tx {
        ("TX", "Transmit", shell.stats.net_up, shell.net_up_hist.to_vec(), p.success.base.color)
    } else {
        ("RX", "Receive", shell.stats.net_down, shell.net_down_hist.to_vec(), p.primary.base.color)
    };
    let detail = match &shell.stats.detail {
        Some(crate::stats::Detail::Net(d)) => Some(d),
        _ => None,
    };
    let iface = detail
        .map(|d| d.iface.clone())
        .filter(|i| !i.is_empty())
        .unwrap_or_else(|| "—".into());
    let max = peak(&samples).max(1.0);

    let mut parts = vec![
        header(label, &fmt_rate(rate), color, direction.into(), iface, &ink),
        section(
            "Last 60 seconds",
            format!("peak {}", fmt_rate(max)),
            history(&samples, max, |_| color, &ink),
            &ink,
        ),
    ];
    if let Some(d) = detail {
        let session = if metric == Metric::Tx { d.total_up } else { d.total_down };
        parts.push(rule(&ink));
        parts.push(foot(
            vec![
                ("Interface", format!("{}  {}", d.iface, d.ip)),
                ("Session", fmt_bytes(session)),
            ],
            &ink,
        ));
    }
    card(parts)
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/// A percentage: `7%`, `83%`, `100%`.
fn percent(p: f32) -> String {
    format!("{:.0}%", p.clamp(0.0, 100.0))
}

/// Human bytes/sec: B/s, KB/s, MB/s.
pub fn fmt_rate(bps: f32) -> String {
    if bps >= 1_000_000.0 {
        format!("{:.1} MB/s", bps / 1_000_000.0)
    } else if bps >= 1000.0 {
        format!("{:.0} KB/s", bps / 1000.0)
    } else {
        format!("{:.0} B/s", bps)
    }
}

fn fmt_uptime(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else {
        format!("{h}h {m}m")
    }
}

fn fmt_bytes(b: u64) -> String {
    let f = b as f32;
    if f >= 1e9 {
        format!("{:.1} GB", f / 1e9)
    } else if f >= 1e6 {
        format!("{:.0} MB", f / 1e6)
    } else {
        format!("{:.0} KB", f / 1e3)
    }
}

/// "NVIDIA GeForce RTX 3090 Ti" → "RTX 3090 Ti".
fn short_gpu(name: &str) -> String {
    name.rsplit("GeForce ").next().unwrap_or(name).to_string()
}
