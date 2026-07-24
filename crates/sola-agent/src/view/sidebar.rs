//! Session sidebar — filter + status-dot rows (graphite agent DS).

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::widget::text::Wrapping;
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{
    RADIUS_MD, RADIUS_SM, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS,
};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input;
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;

use crate::protocol::SessionSummary;
use crate::{App, Msg};

pub(crate) fn view(app: &App) -> Element<'_, Msg> {
    let busy = app.streaming || app.pending.is_some();

    let search = container(
        row![
            text("⌕")
                .size(12)
                .style(kit_text::muted),
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
    .padding(Padding::from([7.0, 10.0]))
    .width(Length::Fill)
    .style(filter_shell_style);

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

    let list: Element<'_, Msg> = if filtered.is_empty() {
        container(
            column![
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
            .spacing(SPACE_SM)
            .padding(Padding::from([SPACE_LG, SPACE_MD])),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    } else {
        let mut col = column![
            text("SESSIONS")
                .font(fonts::ui_medium())
                .size(10)
                .style(kit_text::muted)
        ]
        .spacing(1.0)
        .padding(Padding {
            top: 4.0,
            right: SPACE_SM,
            bottom: SPACE_SM,
            left: SPACE_SM,
        });
        for summary in filtered {
            col = col.push(session_row(summary, app, busy));
        }
        scrollable(col)
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
    };

    let body = column![
        container(search).padding(Padding {
            top: 10.0,
            right: 10.0,
            bottom: 6.0,
            left: 10.0,
        }),
        list,
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    container(body)
        .style(sidebar_chrome)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn session_row<'a>(summary: &'a SessionSummary, app: &'a App, busy: bool) -> Element<'a, Msg> {
    let selected = app.session_id.as_deref() == Some(summary.id.as_str());
    let renaming = app
        .rename
        .as_ref()
        .is_some_and(|r| r.id == summary.id);

    if renaming {
        let draft = app
            .rename
            .as_ref()
            .map(|r| r.draft.as_str())
            .unwrap_or("");
        let field = text_input("Session title", draft)
            .on_input(Msg::RenameDraft)
            .on_submit(Msg::RenameCommit)
            .size(12)
            .style(text_input::style)
            .width(Length::Fill);
        let cancel = button(text("✕").size(11))
            .padding(Padding::from([4, 6]))
            .style(kit_btn::ghost)
            .on_press(Msg::RenameCancel);
        return row![field, cancel]
            .spacing(SPACE_XS)
            .align_y(Alignment::Center)
            .padding(Padding::from([SPACE_SM, SPACE_SM]))
            .into();
    }

    let max_chars = ((app.sidebar_w - 88.0) / 6.8).clamp(10.0, 56.0) as usize;
    let title = ellipsize(&summary.title, max_chars);
    let when = relative_time(summary.updated);
    let project = short_path(&summary.cwd);

    // Status: live TUI → ready green; streaming selected → running amber; else idle.
    let status_kind = if summary.live {
        StatusDot::Ready
    } else if selected && app.streaming {
        StatusDot::Running
    } else {
        StatusDot::Idle
    };

    let meta = column![
        text(title)
            .font(fonts::ui())
            .size(12.5)
            .wrapping(Wrapping::None),
        text(project)
            .font(fonts::mono())
            .size(10.5)
            .style(kit_text::muted)
            .wrapping(Wrapping::None),
    ]
    .spacing(2.0)
    .width(Length::Fill);

    let pin_label = if summary.pinned { "★" } else { "☆" };
    let pin = button(text(pin_label).size(12))
        .padding(Padding::from([2, 4]))
        .style(if summary.pinned {
            pin_starred
        } else {
            kit_btn::ghost
        })
        .on_press(Msg::TogglePin(summary.id.clone()));

    let rename = button(text("✎").size(10))
        .padding(Padding::from([2, 4]))
        .style(kit_btn::ghost)
        .on_press(Msg::StartRename(summary.id.clone()));

    // Selectable body must not nest other buttons (iced constraint).
    let body = row![
        status_dot(status_kind),
        meta,
        text(when)
            .font(fonts::ui())
            .size(10)
            .style(kit_text::muted),
    ]
    .spacing(8.0)
    .align_y(Alignment::Start)
    .padding(Padding::from([8.0, 6.0]));

    let mut item = button(body)
        .width(Length::Fill)
        .padding(0)
        .style(session_row_style(selected));
    if !busy {
        item = item.on_press(Msg::SelectSession(summary.id.clone()));
    }

    let chrome = row![item, rename, pin]
        .spacing(2.0)
        .align_y(Alignment::Start);

    container(chrome)
        .padding(Padding {
            top: 1.0,
            right: 6.0,
            bottom: 1.0,
            left: 8.0,
        })
        .width(Length::Fill)
        .style(if selected {
            session_outer_selected
        } else {
            session_outer_idle
        })
        .into()
}

fn session_outer_idle(_theme: &Theme) -> container::Style {
    container::Style::default()
}

fn session_outer_selected(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let selection = sola_kit::theme::selection();
    let bg = Color {
        r: selection.r * 0.85 + p.primary.base.color.r * 0.08,
        g: selection.g * 0.85 + p.primary.base.color.g * 0.08,
        b: selection.b * 0.85 + p.primary.base.color.b * 0.08,
        a: 1.0,
    };
    container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: Color {
                a: 0.18,
                ..p.primary.base.color
            },
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        ..container::Style::default()
    }
}

#[derive(Clone, Copy)]
enum StatusDot {
    Idle,
    Ready,
    Running,
}

fn status_dot<'a>(kind: StatusDot) -> Element<'a, Msg> {
    let (c, glow) = match kind {
        StatusDot::Idle => (
            Color {
                r: 0.55,
                g: 0.58,
                b: 0.66,
                a: 0.55,
            },
            false,
        ),
        StatusDot::Ready => (
            Color {
                r: 0.24,
                g: 0.81,
                b: 0.56,
                a: 1.0,
            },
            true,
        ),
        StatusDot::Running => (
            Color {
                r: 0.91,
                g: 0.72,
                b: 0.29,
                a: 1.0,
            },
            true,
        ),
    };
    let dot = container(Space::new().width(7.0).height(7.0))
        .width(Length::Fixed(7.0))
        .height(Length::Fixed(7.0))
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(c)),
            border: Border {
                color: if glow {
                    Color { a: 0.18, ..c }
                } else {
                    Color::TRANSPARENT
                },
                width: if glow { 3.0 } else { 0.0 },
                radius: 999.0.into(),
            },
            ..container::Style::default()
        });
    container(dot)
        .padding(Padding {
            top: 5.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        })
        .into()
}

fn session_row_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = theme.extended_palette();
        // Selected chrome is on the outer container; button stays transparent.
        let base = button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: if selected {
                Color {
                    r: 0.96,
                    g: 0.98,
                    b: 1.0,
                    a: 1.0,
                }
            } else {
                p.background.base.text
            },
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: RADIUS_MD.into(),
            },
            shadow: Default::default(),
            snap: false,
        };
        if selected {
            return base;
        }
        match status {
            button::Status::Hovered | button::Status::Pressed => button::Style {
                background: Some(Background::Color(Color {
                    a: 0.65,
                    ..p.background.strong.color
                })),
                ..base
            },
            button::Status::Disabled => kit_btn::ghost(theme, status),
            button::Status::Active => base,
        }
    }
}

fn pin_starred(theme: &Theme, status: button::Status) -> button::Style {
    let mut s = kit_btn::ghost(theme, status);
    s.text_color = theme.extended_palette().warning.base.color;
    s
}

fn sidebar_chrome(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.96,
            ..p.background.weaker.color
        })),
        border: Border {
            color: Color {
                a: 0.55,
                ..p.background.stronger.color
            },
            width: 0.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
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

fn ellipsize(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let take = max_chars.saturating_sub(1).max(1);
    let t: String = s.chars().take(take).collect();
    format!("{t}…")
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
