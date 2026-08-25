//! Stat detail dropdown panels, rendered in the Menu window.

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::{column, container, mouse_area, row, stack, text};
use iced::{Color, Element, Length, Padding, Point, Rectangle, Renderer, Theme, mouse};

use crate::app::{Msg, Shell};
use crate::stats::Metric;
use crate::stats::cpu::Proc;
use sola_kit::components::popover;

pub const CARD_WIDTH: f32 = 320.0;

/// Lower-contrast label text. We deliberately do NOT use
/// `sola_kit::components::text::muted` here — on the dropdown card it resolves
/// to a colour that renders invisible (the same trap the menu accelerators
/// hit). Deriving from `palette().text` keeps it visible. Mirrors
/// `crate::calendar::dim`.
fn dim(theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(iced::Color {
            a: 0.55,
            ..theme.palette().text
        }),
    }
}
/// Build the panel for `metric`, dropped under its menubar indicator, over a
/// dismiss backdrop. Mirrors `crate::menu::view`'s anchored dropdown.
pub fn panel(shell: &Shell, metric: Metric) -> Element<'_, Msg> {
    let card = match metric {
        Metric::Cpu => cpu_card(shell),
        Metric::Gpu => gpu_card(shell),
        Metric::Mem => mem_card(shell),
        Metric::Rx => rx_card(shell),
        Metric::Tx => tx_card(shell),
    };

    // Anchor the card's left edge under the indicator, clamped so it never
    // runs off the right screen edge (leaving an 8px gutter like the calendar).
    let output_w = shell.output_size.map(|(w, _)| w as f32).unwrap_or(1920.0);
    let left = shell
        .estimate_stat_x(metric)
        .min((output_w - CARD_WIDTH - 8.0).max(0.0))
        .max(0.0);

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

    let backdrop: Element<'_, Msg> =
        mouse_area(container(text("")).width(Length::Fill).height(Length::Fill))
            .on_press(Msg::CloseMenu)
            .into();

    stack![backdrop, positioned]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ---------------------------------------------------------------------------
// Card shell helpers
// ---------------------------------------------------------------------------

/// Compose a stat card: header (label/value/identity) + body sections.
fn stat_card<'a>(
    label: &'a str,
    value: String,
    value_color: Color,
    identity: Vec<Element<'a, Msg>>,
    body: Vec<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let header = row![
        column![
            text(label).size(11).style(dim),
            row![
                text(value)
                    .font(sola_kit::fonts::MONO)
                    .size(30)
                    .style(move |_: &Theme| iced::widget::text::Style {
                        color: Some(value_color)
                    }),
            ],
        ]
        .spacing(3),
        iced::widget::Space::new().width(Length::Fill),
        column(identity)
            .spacing(3)
            .align_x(iced::alignment::Horizontal::Right),
    ]
    .align_y(iced::alignment::Vertical::Top);

    let mut col = column![header].spacing(14);
    for el in body {
        col = col.push(el);
    }
    popover(col.padding(4))
        .padding(Padding::new(8.0))
        .width(Length::Fixed(CARD_WIDTH))
        .into()
}

/// A thin labeled caption row used above sub-sections.
fn caption<'a>(left: &'a str, right: String) -> Element<'a, Msg> {
    row![
        text(left).size(11).style(dim),
        iced::widget::Space::new().width(Length::Fill),
        text(right)
            .font(sola_kit::fonts::MONO)
            .size(11)
            .style(|_: &Theme| iced::widget::text::Style {
                color: Some(Color {
                    a: 0.5,
                    ..Color::from_rgb(0.902, 0.929, 0.953)
                }),
            }),
    ]
    .into()
}

fn cpu_card(shell: &Shell) -> Element<'_, Msg> {
    let s = &shell.stats;
    let neutral = Color::from_rgb(0.902, 0.929, 0.953);
    let detail = match &s.detail {
        Some(crate::stats::Detail::Cpu(d)) => Some(d),
        _ => None,
    };

    let id = crate::stats::cpu::identity();
    let identity = vec![
        text(id.model.clone())
            .size(12)
            .style(|_: &Theme| iced::widget::text::Style {
                color: Some(Color::from_rgb(0.788, 0.820, 0.851)),
            })
            .into(),
        text(format!("{}C / {}T", id.cores, id.threads))
            .font(sola_kit::fonts::MONO)
            .size(11)
            .style(dim)
            .into(),
    ];

    let samples = shell.cpu_hist.to_vec();
    let graph = column![
        caption("Last 60 seconds", format!("peak {:.0}%", peak(&samples))),
        graph_box(history_graph(
            samples,
            100.0,
            Color::from_rgb(0.0, 0.831, 1.0)
        )),
    ]
    .spacing(6)
    .into();

    let mut body: Vec<Element<'_, Msg>> = vec![graph];

    if let Some(d) = detail {
        let bars: Vec<Element<'_, Msg>> = d.per_core.iter().map(|p| core_bar(*p)).collect();
        body.push(
            column![
                caption("Per-thread load", format!("{} threads", d.per_core.len())),
                row(bars)
                    .spacing(1.5)
                    .align_y(iced::alignment::Vertical::Bottom),
            ]
            .spacing(6)
            .into(),
        );
        body.push(divider());
        body.push(caption("Top processes", "by CPU".into()));
        let max = d.top.first().map(|t| t.value).unwrap_or(1.0);
        for p in &d.top {
            body.push(proc_row(&p.name, format!("{:.0}%", p.value), p.value, max));
        }
        body.push(divider());
        body.push(footer_pair(
            "LOAD AVG",
            format!("{:.1}  {:.1}  {:.1}", d.load[0], d.load[1], d.load[2]),
            "UPTIME",
            fmt_uptime(d.uptime_secs),
        ));
    }

    stat_card(
        "CPU",
        format!("{:.0}%", s.cpu_pct),
        crate::stats::level_color(s.cpu_pct, neutral),
        identity,
        body,
    )
}

// ---------------------------------------------------------------------------
// MEM card
// ---------------------------------------------------------------------------

fn mem_card(shell: &Shell) -> Element<'_, Msg> {
    let s = &shell.stats;
    let neutral = Color::from_rgb(0.902, 0.929, 0.953);
    let detail = match &s.detail {
        Some(crate::stats::Detail::Mem(d)) => Some(d),
        _ => None,
    };

    let total_gb = detail
        .map(|d| d.info.total_kb as f32 / 1024.0 / 1024.0)
        .unwrap_or(0.0);
    let identity = vec![
        text(format!("{total_gb:.0} GB"))
            .size(12)
            .style(|_: &Theme| iced::widget::text::Style {
                color: Some(Color::from_rgb(0.788, 0.820, 0.851)),
            })
            .into(),
        text("RAM")
            .font(sola_kit::fonts::MONO)
            .size(11)
            .style(dim)
            .into(),
    ];

    let graph = column![
        caption(
            "Last 60 seconds",
            format!("peak {:.0}%", peak(&shell.mem_hist.to_vec()))
        ),
        graph_box(history_graph(
            shell.mem_hist.to_vec(),
            100.0,
            Color::from_rgb(0.0, 0.831, 1.0)
        )),
    ]
    .spacing(6)
    .into();
    let mut body: Vec<Element<'_, Msg>> = vec![graph];

    if let Some(d) = detail {
        let (used, cache, free) = d.info.segments_kb();
        body.push(
            column![caption("Memory", String::new()), seg_bar(used, cache, free)]
                .spacing(6)
                .into(),
        );
        body.push(divider());
        body.push(caption("Top processes", "by RAM".into()));
        let max = d.top.first().map(|t| t.value).unwrap_or(1.0);
        for p in &d.top {
            body.push(proc_row(
                &p.name,
                format!("{:.0} MB", p.value),
                p.value,
                max,
            ));
        }
        body.push(divider());
        let swap_used =
            (d.info.swap_total_kb.saturating_sub(d.info.swap_free_kb)) as f32 / 1024.0 / 1024.0;
        let swap_tot = d.info.swap_total_kb as f32 / 1024.0 / 1024.0;
        body.push(footer_pair(
            "SWAP",
            format!("{swap_used:.1} / {swap_tot:.0} GB"),
            "PRESSURE",
            format!("{:.0}%", s.mem_pct),
        ));
    }

    stat_card(
        "MEM",
        format!("{:.0}%", s.mem_pct),
        crate::stats::level_color(s.mem_pct, neutral),
        identity,
        body,
    )
}

/// Three-segment used/cache/free bar (three siblings → real proportions).
fn seg_bar<'a>(used: u64, cache: u64, free: u64) -> Element<'a, Msg> {
    let total = (used + cache + free).max(1);
    let seg = |kb: u64, color: Color| {
        container(text(""))
            .width(Length::FillPortion(
                ((kb as f32 / total as f32) * 1000.0) as u16,
            ))
            .height(Length::Fixed(8.0))
            .style(move |_: &Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(color)),
                ..Default::default()
            })
    };
    container(row![
        seg(used, Color::from_rgb(0.0, 0.831, 1.0)),
        seg(cache, Color::from_rgb(0.122, 0.435, 0.922)),
        seg(free, Color::from_rgb(0.188, 0.211, 0.243)),
    ])
    .width(Length::Fixed(288.0))
    .height(Length::Fixed(8.0))
    .style(|_: &Theme| iced::widget::container::Style {
        border: iced::Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

// ---------------------------------------------------------------------------
// CPU card helpers
// ---------------------------------------------------------------------------

fn peak(samples: &[f32]) -> f32 {
    samples.iter().copied().fold(0.0, f32::max)
}

fn graph_box<'a>(inner: Element<'a, Msg>) -> Element<'a, Msg> {
    container(inner)
        .height(Length::Fixed(58.0))
        .style(|_: &Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(Color::from_rgb(
                0.051, 0.067, 0.090,
            ))),
            border: iced::Border {
                radius: 6.0.into(),
                width: 1.0,
                color: Color::from_rgb(0.129, 0.149, 0.176),
            },
            ..Default::default()
        })
        .into()
}

fn core_bar<'a>(pct: f32) -> Element<'a, Msg> {
    let h = (pct / 100.0 * 22.0).clamp(2.0, 22.0);
    container(text(""))
        .width(Length::Fixed(5.0))
        .height(Length::Fixed(h))
        .style(|_: &Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(Color::from_rgb(
                0.122, 0.435, 0.922,
            ))),
            border: iced::Border {
                radius: 1.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

/// Ranked process list: caption `Top processes` / `by {kind}` then rows.
fn push_top_procs<'a>(
    body: &mut Vec<Element<'a, Msg>>,
    kind: &'static str,
    rows: &'a [Proc],
    value: impl Fn(&Proc) -> String,
) {
    if rows.is_empty() {
        return;
    }
    body.push(divider());
    body.push(caption("Top processes", format!("by {kind}")));
    let max = rows.first().map(|t| t.value).unwrap_or(1.0);
    for p in rows {
        body.push(proc_row(&p.name, value(p), p.value, max));
    }
}

fn divider<'a>() -> Element<'a, Msg> {
    container(text(""))
        .width(Length::Fixed(288.0))
        .height(Length::Fixed(1.0))
        .style(|_: &Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(Color::from_rgb(
                0.129, 0.149, 0.176,
            ))),
            ..Default::default()
        })
        .into()
}

/// One "top process" row: name + value, with a proportion bar underneath.
/// The bar is a two-child row (filled portion + remainder spacer) so the
/// proportion actually shows — a lone FillPortion child would fill 100%.
fn proc_row<'a>(name: &'a str, val: String, value: f32, max: f32) -> Element<'a, Msg> {
    let frac = if max > 0.0 {
        (value / max).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let fill = (frac * 1000.0) as u16;
    let rest = 1000u16.saturating_sub(fill);

    let bar = container(row![
        container(text(""))
            .width(Length::FillPortion(fill))
            .height(Length::Fixed(3.0))
            .style(|_: &Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(Color::from_rgb(
                    0.122, 0.435, 0.922
                ))),
                border: iced::Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
        iced::widget::Space::new().width(Length::FillPortion(rest)),
    ])
    .width(Length::Fixed(288.0))
    .height(Length::Fixed(3.0))
    .style(|_: &Theme| iced::widget::container::Style {
        background: Some(iced::Background::Color(Color::from_rgb(
            0.102, 0.122, 0.153,
        ))),
        border: iced::Border {
            radius: 2.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });

    column![
        row![
            text(name.to_string())
                .size(13)
                .style(|_: &Theme| iced::widget::text::Style {
                    color: Some(Color::from_rgb(0.788, 0.820, 0.851))
                }),
            iced::widget::Space::new().width(Length::Fill),
            text(val).font(sola_kit::fonts::MONO).size(12),
        ],
        bar,
    ]
    .spacing(3)
    .into()
}

fn footer_pair<'a>(l1: &'a str, v1: String, l2: &'a str, v2: String) -> Element<'a, Msg> {
    let cell = |label: &'a str, val: String, right: bool| {
        let c = column![
            text(label).size(10).style(dim),
            text(val)
                .font(sola_kit::fonts::MONO)
                .size(12)
                .style(|_: &Theme| iced::widget::text::Style {
                    color: Some(Color::from_rgb(0.788, 0.820, 0.851))
                }),
        ]
        .spacing(3);
        if right {
            c.align_x(iced::alignment::Horizontal::Right)
        } else {
            c
        }
    };
    row![
        cell(l1, v1, false),
        iced::widget::Space::new().width(Length::Fill),
        cell(l2, v2, true),
    ]
    .into()
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

// ---------------------------------------------------------------------------
// RX / TX cards (one direction each)
// ---------------------------------------------------------------------------

const RX_COLOR: Color = Color::from_rgb(0.0, 0.831, 1.0); // cyan — download
const TX_COLOR: Color = Color::from_rgb(0.247, 0.725, 0.314); // green — upload

fn rx_card(shell: &Shell) -> Element<'_, Msg> {
    rate_card(
        shell,
        "RX",
        "Receive",
        shell.stats.net_down,
        shell.net_down_hist.to_vec(),
        RX_COLOR,
        |d| d.total_down,
    )
}

fn tx_card(shell: &Shell) -> Element<'_, Msg> {
    rate_card(
        shell,
        "TX",
        "Transmit",
        shell.stats.net_up,
        shell.net_up_hist.to_vec(),
        TX_COLOR,
        |d| d.total_up,
    )
}

/// One-direction network card: rate headline, single history graph, iface footer.
fn rate_card<'a, F>(
    shell: &'a Shell,
    label: &'a str,
    direction: &'a str,
    rate: f32,
    samples: Vec<f32>,
    color: Color,
    session_bytes: F,
) -> Element<'a, Msg>
where
    F: FnOnce(&crate::stats::net::NetDetail) -> u64,
{
    let detail = match &shell.stats.detail {
        Some(crate::stats::Detail::Net(d)) => Some(d),
        _ => None,
    };

    let identity = vec![
        text(direction)
            .size(12)
            .style(|_: &Theme| iced::widget::text::Style {
                color: Some(Color::from_rgb(0.788, 0.820, 0.851)),
            })
            .into(),
        text(
            detail
                .map(|d| {
                    if d.iface.is_empty() {
                        "—".into()
                    } else {
                        d.iface.clone()
                    }
                })
                .unwrap_or_else(|| "—".into()),
        )
        .font(sola_kit::fonts::MONO)
        .size(11)
        .style(dim)
        .into(),
    ];

    let max = peak(&samples).max(1.0);
    let graph = column![
        caption("Last 60 seconds", format!("peak {}", fmt_rate(max))),
        graph_box(history_graph(samples, max, color)),
    ]
    .spacing(6)
    .into();

    let mut body: Vec<Element<'_, Msg>> = vec![graph];
    if let Some(d) = detail {
        body.push(divider());
        body.push(footer_pair(
            "INTERFACE",
            format!("{}  {}", d.iface, d.ip),
            "SESSION",
            fmt_bytes(session_bytes(d)),
        ));
    }

    // Rate metrics have no threshold coloring.
    stat_card(
        label,
        fmt_rate(rate),
        Color::from_rgb(0.902, 0.929, 0.953),
        identity,
        body,
    )
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

// ---------------------------------------------------------------------------
// GPU card
// ---------------------------------------------------------------------------

fn gpu_card(shell: &Shell) -> Element<'_, Msg> {
    let s = &shell.stats;
    let neutral = Color::from_rgb(0.902, 0.929, 0.953);
    let util = s.gpu.map(|g| g.util).unwrap_or(0.0);
    let detail = match &s.detail {
        Some(crate::stats::Detail::Gpu(d)) => Some(d),
        _ => None,
    };

    let identity = vec![
        text(
            detail
                .map(|d| short_gpu(&d.name))
                .unwrap_or_else(|| "GPU".into()),
        )
        .size(12)
        .style(|_: &Theme| iced::widget::text::Style {
            color: Some(Color::from_rgb(0.788, 0.820, 0.851)),
        })
        .into(),
        text(
            detail
                .map(|d| format!("{:.0} GB", d.mem_total_mb / 1024.0))
                .unwrap_or_default(),
        )
        .font(sola_kit::fonts::MONO)
        .size(11)
        .style(dim)
        .into(),
    ];

    let graph = column![
        caption(
            "Last 60 seconds",
            format!("peak {:.0}%", peak(&shell.gpu_hist.to_vec()))
        ),
        graph_box(history_graph(
            shell.gpu_hist.to_vec(),
            100.0,
            Color::from_rgb(0.0, 0.831, 1.0)
        )),
    ]
    .spacing(6)
    .into();
    let mut body: Vec<Element<'_, Msg>> = vec![graph];

    if let Some(d) = detail {
        let frac = if d.mem_total_mb > 0.0 {
            d.mem_used_mb / d.mem_total_mb
        } else {
            0.0
        };
        body.push(
            column![
                caption(
                    "VRAM",
                    format!(
                        "{:.1} / {:.0} GB",
                        d.mem_used_mb / 1024.0,
                        d.mem_total_mb / 1024.0
                    )
                ),
                level_bar(frac, Color::from_rgb(0.0, 0.831, 1.0)),
            ]
            .spacing(6)
            .into(),
        );
        push_top_procs(&mut body, "GPU", &d.top_gpu, |p| format!("{:.0}%", p.value));
        push_top_procs(&mut body, "VRAM", &d.top_vram, |p| {
            format!("{:.0} MB", p.value)
        });
        body.push(divider());
        body.push(footer_pair(
            "TEMP",
            format!("{:.0}\u{00b0}C", d.temp_c),
            "POWER",
            format!("{:.0} W", d.power_w),
        ));
        body.push(footer_pair(
            "FAN",
            format!("{:.0}%", d.fan_pct),
            "CLOCK",
            format!("{} MHz", d.clock_mhz),
        ));
    }

    stat_card(
        "GPU",
        format!("{:.0}%", util),
        crate::stats::level_color(util, neutral),
        identity,
        body,
    )
}

/// "NVIDIA GeForce RTX 3090 Ti" → "RTX 3090 Ti".
fn short_gpu(name: &str) -> String {
    name.rsplit("GeForce ").next().unwrap_or(name).to_string()
}

/// Single horizontal fill bar (0..1). Two children (fill + spacer) so the
/// proportion actually shows — a lone FillPortion child would fill 100%.
fn level_bar<'a>(frac: f32, color: Color) -> Element<'a, Msg> {
    let f = frac.clamp(0.0, 1.0);
    let fill = (f * 1000.0) as u16;
    let rest = 1000u16.saturating_sub(fill);
    container(row![
        container(text(""))
            .width(Length::FillPortion(fill))
            .height(Length::Fixed(8.0))
            .style(move |_: &Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(color)),
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
        iced::widget::Space::new().width(Length::FillPortion(rest)),
    ])
    .width(Length::Fixed(288.0))
    .height(Length::Fixed(8.0))
    .style(|_: &Theme| iced::widget::container::Style {
        background: Some(iced::Background::Color(Color::from_rgb(
            0.188, 0.211, 0.243,
        ))),
        border: iced::Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

// ---------------------------------------------------------------------------
// History graph widget
// ---------------------------------------------------------------------------

/// A 60-sample area+line history chart. `max` is the value mapped to the top
/// (e.g. 100.0 for percentages, or the buffer peak for rates).
pub struct Graph {
    pub samples: Vec<f32>,
    pub max: f32,
    pub color: Color,
}

impl<Message> canvas::Program<Message> for Graph {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let n = self.samples.len();
        if n < 2 || self.max <= 0.0 {
            return vec![frame.into_geometry()];
        }
        let w = bounds.width;
        let h = bounds.height;
        let x = |i: usize| (i as f32 / (n - 1) as f32) * w;
        let y = |v: f32| h - (v / self.max).clamp(0.0, 1.0) * h;

        let line = Path::new(|p: &mut canvas::path::Builder| {
            p.move_to(Point::new(x(0), y(self.samples[0])));
            for i in 1..n {
                p.line_to(Point::new(x(i), y(self.samples[i])));
            }
        });
        let area = Path::new(|p: &mut canvas::path::Builder| {
            p.move_to(Point::new(x(0), h));
            for i in 0..n {
                p.line_to(Point::new(x(i), y(self.samples[i])));
            }
            p.line_to(Point::new(x(n - 1), h));
            p.close();
        });
        frame.fill(
            &area,
            Color {
                a: 0.25,
                ..self.color
            },
        );
        frame.stroke(
            &line,
            Stroke::default().with_color(self.color).with_width(1.5),
        );
        vec![frame.into_geometry()]
    }
}

/// Convenience: a fixed-height graph element from samples.
pub fn history_graph<'a, Message: 'a>(
    samples: Vec<f32>,
    max: f32,
    color: Color,
) -> Element<'a, Message> {
    Canvas::new(Graph {
        samples,
        max,
        color,
    })
    .width(Length::Fill)
    .height(Length::Fixed(58.0))
    .into()
}
