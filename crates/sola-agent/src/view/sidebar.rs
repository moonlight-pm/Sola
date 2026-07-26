//! Session sidebar — sola-kit [`SidebarPanel`] with card-style sessions.
//!
//! Each session is a soft raised **card** (kit [`SidebarItemChrome::Card`])
//! with custom body content: project leaf, generated title, context badge,
//! age, and activity. Hover a card for trash (first click arms, second deletes).

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::badge::{self, Tone};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{RADIUS_MD, RADIUS_SM, SPACE_MD, SPACE_SM};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input;
use sola_kit::components::text_input::text_input;
use sola_kit::components::{
    DividerColors, SidebarHoverAction, SidebarIndicator, SidebarItem, SidebarPanel, SidebarSection,
};
use sola_kit::fonts;

use crate::protocol::SessionSummary;
use crate::{App, Msg};

/// Intrinsic card height for scroll-chip math (pad + lines + badge row).
const SESSION_CARD_H: f32 = 92.0;

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
        // Air between cards so each reads as a surface, not a packed list.
        .item_spacing(SPACE_MD)
        .section_scroll(app.session_section_scroll, Msg::SessionSectionScroll)
        .item_hover(app.session_hover.clone(), Msg::SessionHover)
        .resizable_with(
            app.sidebar_w,
            app.dragging_divider,
            Msg::DividerPress,
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
    // Budget for list width so clip + ellipsis both read clean.
    let max_dir = ((app.sidebar_w - 96.0) / 7.0).clamp(10.0, 36.0) as usize;
    let max_title = ((app.sidebar_w - 72.0) / 6.4).clamp(12.0, 52.0) as usize;
    // Directory is the primary identity; generated titles are secondary.
    let dir = ellipsize(&project_leaf(&summary.cwd), max_dir);
    let title = ellipsize(&summary.title, max_title);
    let when = relative_time(summary.updated);

    // Prefer live ACP usage for the open tab; otherwise disk-scanned values
    // so unloaded rows still show last known context size.
    let (used, size) = if selected {
        (
            app.usage_used.or(summary.usage_used),
            app.usage_size.or(summary.usage_size),
        )
    } else {
        (summary.usage_used, summary.usage_size)
    };
    let context = format_context_kb(used, size);

    // Activity: recent disk activity, or the selected session streaming.
    let working =
        summary.busy || (selected && (app.streaming || app.pending.is_some()));
    let indicator = if working {
        SidebarIndicator::Active
    } else {
        SidebarIndicator::Idle
    };

    let _ = busy;
    let armed = app.delete_armed.as_deref() == Some(summary.id.as_str());
    let body = session_card_body(&dir, &title, &when, context.as_deref(), working, indicator);

    // Collapsed / fallback label still uses the project leaf.
    SidebarItem::new(dir, Msg::SelectSession(summary.id.clone()))
        .id(summary.id.clone())
        .active(selected)
        .card()
        .content(body)
        .height_hint(SESSION_CARD_H)
        .hover_action(SidebarHoverAction {
            message: Msg::SessionDeleteClick(summary.id.clone()),
            armed,
        })
}

/// Card face: status + project, title, then meta chips (context / live / age).
///
/// Layout (Overview-inspired density):
/// ```text
/// ●  Sola                         12m
///    That works perfectly. Merge…
///    [42k/500k]  [LIVE]
/// ```
fn session_card_body(
    dir: &str,
    title: &str,
    when: &str,
    context: Option<&str>,
    working: bool,
    indicator: SidebarIndicator,
) -> Element<'static, Msg> {
    let title_row = row![
        status_dot(indicator),
        text(dir.to_string())
            .font(fonts::ui_medium())
            .size(14)
            .width(Length::Fill),
        text(when.to_string())
            .font(fonts::ui())
            .size(11)
            .style(|theme: &Theme| {
                let c = theme.extended_palette().background.base.text;
                iced::widget::text::Style {
                    color: Some(Color { a: 0.45, ..c }),
                }
            }),
    ]
    .spacing(SPACE_MD)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let subtitle = text(title.to_string())
        .font(fonts::ui())
        .size(12)
        .style(|theme: &Theme| {
            let c = theme.extended_palette().background.base.text;
            iced::widget::text::Style {
                color: Some(Color { a: 0.48, ..c }),
            }
        })
        .width(Length::Fill);

    // Indent subtitle under the title text (past the status dot).
    let subtitle = container(subtitle)
        .padding(Padding {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 14.0,
        })
        .width(Length::Fill);

    let mut chips = row![].spacing(SPACE_SM).align_y(Alignment::Center);
    if let Some(kb) = context {
        chips = chips.push(badge::badge(kb.to_string(), Tone::Neutral));
    }
    if working {
        chips = chips.push(badge::badge("LIVE", Tone::Success));
    }
    let chips = container(chips).padding(Padding {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 14.0,
    });

    column![title_row, subtitle, chips]
        .spacing(SPACE_SM + 1.0)
        .width(Length::Fill)
        .into()
}

fn status_dot(indicator: SidebarIndicator) -> Element<'static, Msg> {
    let color = match indicator {
        SidebarIndicator::Active => Color {
            r: 0.24,
            g: 0.81,
            b: 0.56,
            a: 1.0,
        },
        SidebarIndicator::Idle => Color {
            r: 0.45,
            g: 0.48,
            b: 0.55,
            a: 0.55,
        },
    };
    container(iced::widget::Space::new().width(7.0).height(7.0))
        .width(Length::Fixed(7.0))
        .height(Length::Fixed(7.0))
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: 999.0.into(),
                ..Default::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// Compact context badge for a session row (`42k` or `42k/500k`).
fn format_context_kb(used: Option<u64>, size: Option<u64>) -> Option<String> {
    let used = used?;
    let used_k = (used + 500) / 1000;
    if let Some(size) = size.filter(|s| *s > 0) {
        let size_k = (size + 500) / 1000;
        Some(format!("{used_k}k/{size_k}k"))
    } else {
        Some(format!("{used_k}k"))
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
