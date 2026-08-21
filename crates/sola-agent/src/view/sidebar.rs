//! Session sidebar — sola-kit [`SidebarPanel`] with OD session cards.
//!
//! Matches Open Design `sola-agent-ds.html`: equal graphite cards, surface-only
//! selection, slim bottom context progress bar (no numeric label), rail with
//! hover X close on top and relative time below. No LIVE badge.

use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{
    RADIUS_MD, RADIUS_SM, SPACE_MD, SPACE_SM, linear_bg, mix, mix_white,
};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input;
use sola_kit::components::text_input::text_input;
use sola_kit::components::{
    DividerColors, SidebarIndicator, SidebarItem, SidebarPanel, SidebarSection,
};
use sola_kit::fonts;

use crate::protocol::SessionSummary;
use crate::{App, Msg};

/// Default context window when size is unknown (matches OD `CTX_MAX_K` × 1k).
const DEFAULT_CTX_SIZE: u64 = 500_000;

/// OD session card min-height (~76) + a little for layout slack.
const SESSION_CARD_H: f32 = 80.0;

pub(crate) fn view(app: &App) -> Element<'_, Msg> {
    let busy = app.streaming || app.pending.is_some();

    let header = sidebar_header(app);

    let needle = app.session_filter.trim().to_ascii_lowercase();
    let filtered: Vec<&SessionSummary> = app
        .sessions
        .iter()
        .filter(|s| {
            if needle.is_empty() {
                return true;
            }
            s.title.to_ascii_lowercase().contains(&needle)
                || s.cwd.to_ascii_lowercase().contains(&needle)
        })
        .collect();

    let sections = build_sections(&filtered, app, busy);

    // Divider bands: raised sidebar | hairline | deeper main pane.
    let p_bg = app.theme.extended_palette().background;
    let side_bg = Color {
        a: 0.96,
        ..p_bg.weaker.color
    };
    let main_bg = Color {
        r: p_bg.base.color.r * 0.88,
        g: p_bg.base.color.g * 0.88,
        b: p_bg.base.color.b * 0.88,
        a: 1.0,
    };
    let line = Color {
        a: 0.45,
        ..p_bg.stronger.color
    };

    let mut panel = SidebarPanel::new(sections)
        .header(header)
        // OD: margin-bottom 8px between session cards.
        .item_spacing(SPACE_MD)
        .section_scroll(app.session_section_scroll, Msg::SessionSectionScroll)
        .controller(&app.sidebar, Msg::Sidebar)
        .resizable_with(
            app.sidebar_w,
            DividerColors {
                a: side_bg,
                line,
                b: main_bg,
            },
        );

    if filtered.is_empty() {
        let empty = column![
            kit_text::body(if app.sessions.is_empty() {
                "No sessions yet"
            } else {
                "No matches"
            })
            .style(kit_text::muted),
            kit_text::caption(if app.sessions.is_empty() {
                "New starts a Grok conversation for a project."
            } else {
                "Try another filter."
            })
            .style(kit_text::muted),
        ]
        .spacing(SPACE_SM);
        panel = panel.footer(empty.into());
    }

    panel.build()
}

fn build_sections(
    filtered: &[&SessionSummary],
    app: &App,
    busy: bool,
) -> Vec<SidebarSection<'static, Msg>> {
    if filtered.is_empty() {
        return Vec::new();
    }

    let items: Vec<SidebarItem<'static, Msg>> = filtered
        .iter()
        .map(|s| session_item(s, app, busy))
        .collect();
    vec![SidebarSection::new("Sessions", items).fill()]
}

fn sidebar_header(app: &App) -> Element<'_, Msg> {
    let search = container(
        row![
            text("⌕").size(12).style(kit_text::muted),
            text_input("Filter sessions", &app.session_filter)
                .on_input(Msg::SessionFilter)
                .size(12)
                .padding(0)
                .style(filter_input_style)
                .width(Length::Fill),
        ]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([10.0, 12.0]))
    .width(Length::Fill)
    .style(filter_shell_style);

    if let Some(r) = &app.rename {
        let field = text_input("Session title", &r.draft)
            .on_input(Msg::RenameDraft)
            .on_submit(Msg::RenameCommit)
            .size(13)
            .style(text_input::style)
            .width(Length::Fill);
        let cancel = button(text("✕").size(12))
            .padding(Padding::from([6, 8]))
            .style(kit_btn::ghost)
            .on_press(Msg::RenameCancel);
        let rename_row = container(
            column![
                text("Rename")
                    .font(fonts::ui_medium())
                    .size(11)
                    .style(kit_text::muted),
                row![field, cancel]
                    .spacing(SPACE_SM)
                    .align_y(Alignment::Center),
            ]
            .spacing(6.0),
        )
        .padding(Padding::from([10.0, 12.0]))
        .width(Length::Fill)
        .style(filter_shell_style);

        return column![search, rename_row].spacing(SPACE_MD).into();
    }

    search.into()
}

fn session_item(summary: &SessionSummary, app: &App, busy: bool) -> SidebarItem<'static, Msg> {
    let selected = app.session_id.as_deref() == Some(summary.id.as_str());
    let max_dir = ((app.sidebar_w - 100.0) / 7.0).clamp(10.0, 36.0) as usize;
    let max_title = ((app.sidebar_w - 80.0) / 6.4).clamp(12.0, 52.0) as usize;
    let dir = ellipsize(&project_leaf(&summary.cwd), max_dir);
    let title = ellipsize(&summary.title, max_title);
    let when = relative_time(summary.updated);

    let (used, size) = if selected {
        (
            app.usage_used.or(summary.usage_used),
            app.usage_size.or(summary.usage_size),
        )
    } else {
        (summary.usage_used, summary.usage_size)
    };

    let working = summary.busy || (selected && (app.streaming || app.pending.is_some() || busy));
    let indicator = if working {
        SidebarIndicator::Active
    } else {
        SidebarIndicator::Idle
    };

    let hovered = app.sidebar.hover() == Some(summary.id.as_str());
    let armed = app.delete_armed.as_deref() == Some(summary.id.as_str());
    let body = session_card_body(
        &dir,
        &title,
        &when,
        used,
        size,
        selected,
        indicator,
        summary.id.clone(),
        hovered,
        armed,
    );

    // Custom body owns padding; card chrome draws OD graphite surface.
    // Hover tracking still needs `.id`; close lives in the rail (not hover_action).
    SidebarItem::new(dir, Msg::SelectSession(summary.id.clone()))
        .id(summary.id.clone())
        .active(selected)
        .card()
        .content(body)
        .height_hint(SESSION_CARD_H)
}

/// OD session card body:
/// ```text
/// ●  Project                     [×]
///    Generated title…             12m
/// ──────────────── ctx bar ─────────
/// ```
fn session_card_body(
    dir: &str,
    title: &str,
    when: &str,
    used: Option<u64>,
    size: Option<u64>,
    selected: bool,
    indicator: SidebarIndicator,
    session_id: String,
    show_close: bool,
    armed: bool,
) -> Element<'static, Msg> {
    let project = text(dir.to_string())
        .font(fonts::ui_medium())
        .size(13)
        .style(move |theme: &Theme| {
            let p = theme.extended_palette();
            iced::widget::text::Style {
                color: Some(if selected {
                    Color::from_rgb(0.949, 0.961, 0.980) // #f2f5fa
                } else {
                    p.background.base.text
                }),
            }
        })
        .width(Length::Fill);

    let subtitle = text(title.to_string())
        .font(fonts::ui())
        .size(12)
        .style(move |theme: &Theme| {
            let p = theme.extended_palette();
            let muted = p.secondary.base.text;
            let fg = p.background.base.text;
            iced::widget::text::Style {
                color: Some(if selected {
                    mix(fg, muted, 0.72)
                } else {
                    muted
                }),
            }
        })
        .width(Length::Fill);

    let meta = column![project, subtitle].spacing(3.0).width(Length::Fill);

    let when_el = text(when.to_string())
        .font(fonts::ui())
        .size(10)
        .style(move |theme: &Theme| {
            let p = theme.extended_palette();
            let muted = p.secondary.base.text;
            let fg = p.background.base.text;
            iced::widget::text::Style {
                color: Some(if selected {
                    mix(fg, muted, 0.55)
                } else {
                    Color { a: 0.85, ..muted }
                }),
            }
        });

    // Rail: 22px close slot on top (always reserved), time below.
    let close_slot: Element<'static, Msg> = if show_close {
        close_button(session_id, armed)
    } else {
        Space::new().width(22.0).height(22.0).into()
    };

    let rail = column![
        container(close_slot)
            .width(Length::Fixed(30.0))
            .align_x(Alignment::End),
        container(when_el)
            .width(Length::Fixed(30.0))
            .align_x(Alignment::End)
            .padding(Padding {
                top: 0.0,
                right: 0.0,
                bottom: 2.0,
                left: 0.0,
            }),
    ]
    .spacing(4.0)
    .align_x(Alignment::End)
    .width(Length::Fixed(30.0));

    let row_main = row![
        container(status_dot(indicator))
            .width(Length::Fixed(12.0))
            .padding(Padding {
                top: 6.0,
                right: 0.0,
                bottom: 0.0,
                left: 2.0,
            }),
        meta,
        rail,
    ]
    .spacing(10.0)
    .align_y(Alignment::Start)
    .width(Length::Fill);

    let main_pad = container(row_main)
        .padding(Padding {
            top: 12.0,
            right: 12.0,
            bottom: 12.0,
            left: 12.0,
        })
        .width(Length::Fill);

    let bar = context_bar(used, size, selected);

    column![main_pad, bar].width(Length::Fill).into()
}

/// Slim 3px inset progress track — fill only, no label (OD `.ctx-track`).
fn context_bar(used: Option<u64>, size: Option<u64>, selected: bool) -> Element<'static, Msg> {
    let size = size.filter(|s| *s > 0).unwrap_or(DEFAULT_CTX_SIZE);
    let used = used.unwrap_or(0);
    let pct = if size == 0 {
        0.0
    } else {
        (used as f32 / size as f32).clamp(0.0, 1.0)
    };

    // FillPortion needs integers; 0 fill keeps an empty quiet track.
    let used_parts = ((pct * 1000.0).round() as u16).max(if pct > 0.001 { 1 } else { 0 });
    let empty_parts = 1000u16.saturating_sub(used_parts).max(1);

    let fill_row: Element<'static, Msg> = if used_parts == 0 {
        Space::new().width(Length::Fill).height(3.0).into()
    } else {
        row![
            container(Space::new().width(Length::Fill).height(3.0))
                .width(Length::FillPortion(used_parts))
                .height(Length::Fixed(3.0))
                .style(move |theme: &Theme| {
                    let accent = theme.extended_palette().primary.base.color;
                    let bright = mix(accent, Color::from_rgb(0.545, 0.914, 1.0), 0.55);
                    let a = if selected { 1.0 } else { 0.90 };
                    container::Style {
                        background: Some(linear_bg(
                            90.0,
                            &[
                                (
                                    0.0,
                                    Color {
                                        a: if selected { 0.50 } else { 0.40 },
                                        ..accent
                                    },
                                ),
                                (1.0, Color { a, ..bright }),
                            ],
                        )),
                        border: Border {
                            radius: 999.0.into(),
                            ..Default::default()
                        },
                        ..container::Style::default()
                    }
                }),
            Space::new()
                .width(Length::FillPortion(empty_parts))
                .height(3.0),
        ]
        .width(Length::Fill)
        .height(Length::Fixed(3.0))
        .into()
    };

    // Track is the 3px pill; outer padding insets it from the card edges.
    let track = container(fill_row)
        .width(Length::Fill)
        .height(Length::Fixed(3.0))
        .clip(true)
        .style(move |theme: &Theme| {
            let raised = theme.extended_palette().background.weaker.color;
            let track = if selected {
                mix_white(raised, 0.08)
            } else {
                mix_white(raised, 0.06)
            };
            container::Style {
                background: Some(Background::Color(track)),
                border: Border {
                    radius: 999.0.into(),
                    ..Default::default()
                },
                ..container::Style::default()
            }
        });

    container(track)
        .width(Length::Fill)
        .padding(Padding {
            top: 0.0,
            right: 10.0,
            bottom: 10.0,
            left: 10.0,
        })
        .into()
}

fn close_button(session_id: String, armed: bool) -> Element<'static, Msg> {
    let color = if armed {
        Color {
            r: 0.94,
            g: 0.44,
            b: 0.47,
            a: 1.0,
        }
    } else {
        Color {
            r: 0.55,
            g: 0.58,
            b: 0.66,
            a: 0.95,
        }
    };
    button(
        text("×")
            .font(fonts::ui())
            .size(14)
            .style(move |_t: &Theme| iced::widget::text::Style { color: Some(color) }),
    )
    .padding(Padding::from([2, 6]))
    .style(move |theme: &Theme, status| {
        let p = theme.extended_palette();
        let bg = match status {
            button::Status::Hovered if armed => Color {
                a: 0.22,
                ..p.danger.base.color
            },
            button::Status::Hovered => Color {
                a: 0.14,
                ..p.danger.base.color
            },
            button::Status::Pressed => Color {
                a: 0.28,
                ..p.danger.base.color
            },
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: RADIUS_SM.into(),
                ..Default::default()
            },
            text_color: color,
            ..button::Style::default()
        }
    })
    .on_press(Msg::SessionDeleteClick(session_id))
    .into()
}

fn status_dot(indicator: SidebarIndicator) -> Element<'static, Msg> {
    let (color, ring) = match indicator {
        SidebarIndicator::Active => (
            Color {
                r: 0.24,
                g: 0.81,
                b: 0.56,
                a: 1.0,
            },
            Some(Color {
                r: 0.24,
                g: 0.81,
                b: 0.56,
                a: 0.20,
            }),
        ),
        SidebarIndicator::Idle
        | SidebarIndicator::Working
        | SidebarIndicator::Waiting
        | SidebarIndicator::Done => (
            Color {
                r: 0.55,
                g: 0.58,
                b: 0.66,
                a: 0.55,
            },
            None,
        ),
    };
    // Soft glow ring for live: outer 13px wash + 7px core (OD box-shadow 0 0 0 3px).
    let core = container(Space::new().width(7.0).height(7.0))
        .width(Length::Fixed(7.0))
        .height(Length::Fixed(7.0))
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: 999.0.into(),
                ..Default::default()
            },
            ..container::Style::default()
        });
    if let Some(ring) = ring {
        container(core)
            .padding(3)
            .style(move |_t: &Theme| container::Style {
                background: Some(Background::Color(ring)),
                border: Border {
                    radius: 999.0.into(),
                    ..Default::default()
                },
                ..container::Style::default()
            })
            .into()
    } else {
        core.into()
    }
}

fn ellipsize(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let take = max_chars.saturating_sub(1).max(1);
    let t: String = s.chars().take(take).collect();
    format!("{t}…")
}

fn filter_shell_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.70,
            ..p.background.base.color
        })),
        border: Border {
            color: Color {
                a: 0.55,
                ..p.background.stronger.color
            },
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        ..container::Style::default()
    }
}

fn filter_input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut s = text_input::style(theme, status);
    s.background = Background::Color(Color::TRANSPARENT);
    s.border = Border {
        color: Color::TRANSPARENT,
        width: 0.0,
        radius: RADIUS_SM.into(),
    };
    s
}

pub(crate) fn short_path(p: &str) -> String {
    if p.is_empty() {
        return String::new();
    }
    if let Ok(home) = std::env::var("HOME") {
        if let Some(rest) = p.strip_prefix(&home) {
            return format!("~{rest}");
        }
    }
    p.to_string()
}

pub(crate) fn project_leaf(p: &str) -> String {
    let short = short_path(p);
    short
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(&short)
        .to_string()
}

fn relative_time(updated: u64) -> String {
    if updated == 0 {
        return String::new();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(updated);
    let age = now.saturating_sub(updated);
    if age < 60 {
        "now".into()
    } else if age < 3600 {
        format!("{}m", age / 60)
    } else if age < 86400 {
        format!("{}h", age / 3600)
    } else if age < 86400 * 14 {
        format!("{}d", age / 86400)
    } else {
        chrono::DateTime::from_timestamp(updated as i64, 0)
            .map(|d| d.format("%b %e").to_string())
            .unwrap_or_default()
    }
}
