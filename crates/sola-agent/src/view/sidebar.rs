//! Session sidebar — sola-kit [`SidebarPanel`] with filter header.

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{RADIUS_MD, RADIUS_SM, SPACE_MD, SPACE_SM};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input;
use sola_kit::components::text_input::text_input;
use sola_kit::components::{DividerColors, SidebarItem, SidebarPanel, SidebarSection};
use sola_kit::fonts;

use crate::protocol::SessionSummary;
use crate::{App, Msg};

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

    let sections = if filtered.is_empty() {
        Vec::new()
    } else {
        let items: Vec<SidebarItem<Msg>> = filtered
            .iter()
            .map(|s| session_item(s, app, busy))
            .collect();
        // Fill section: sticky "Sessions" label + bar-less item scroll with
        // ↑ N … / ↓ N … chips (see SidebarPanel::section_scroll).
        vec![SidebarSection::new("Sessions", items).fill()]
    };

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
        .section_scroll(app.session_section_scroll, Msg::SessionSectionScroll)
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
    .padding(Padding::from([8.0, 10.0]))
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
        .padding(Padding::from([8.0, 10.0]))
        .width(Length::Fill)
        .style(filter_shell_style);

        return column![search, rename_row].spacing(8.0).into();
    }

    search.into()
}

fn session_item(summary: &SessionSummary, app: &App, busy: bool) -> SidebarItem<Msg> {
    let selected = app.session_id.as_deref() == Some(summary.id.as_str());
    // Budget title/path for list width so clip + ellipsis both read clean.
    let max_title = ((app.sidebar_w - 72.0) / 7.0).clamp(12.0, 48.0) as usize;
    let max_path = ((app.sidebar_w - 72.0) / 6.2).clamp(10.0, 42.0) as usize;
    let title = ellipsize(&summary.title, max_title);
    let project = ellipsize(&short_path(&summary.cwd), max_path);
    let when = relative_time(summary.updated);

    // Single click selects; double-click rename is handled in App
    // (two SelectSession within a short window) so the kit button path
    // keeps hover chrome.
    let _ = busy;
    SidebarItem::new(title, Msg::SelectSession(summary.id.clone()))
        .active(selected)
        .subtitle(project)
        .secondary(when)
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
