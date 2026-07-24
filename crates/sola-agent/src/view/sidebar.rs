//! Session sidebar — full-height list; width owned by parent (resizable split).

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::widget::text::Wrapping;
use iced::{Alignment, Element, Length, Padding};
use sola_kit::components::button as kit_btn;
use sola_kit::components::sidebar::style as sidebar_style;
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input;
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;

use crate::protocol::SessionSummary;
use crate::{App, Msg};

pub(crate) fn view(app: &App) -> Element<'_, Msg> {
    let busy = app.streaming || app.pending.is_some();

    let mut new_btn = kit_btn::labeled_sm("New", kit_btn::secondary);
    if !busy {
        new_btn = new_btn.on_press(Msg::NewSession);
    }

    let header = container(
        row![
            kit_text::subheading("Sessions"),
            Space::new().width(Length::Fill),
            new_btn,
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([SPACE_LG, SPACE_MD]))
    .width(Length::Fill);

    let list: Element<'_, Msg> = if app.sessions.is_empty() {
        container(
            column![
                kit_text::body("No sessions yet").style(kit_text::muted),
                kit_text::caption(
                    "New starts a Grok conversation for a project. \
                     Sessions from every project appear here."
                )
                .style(kit_text::muted),
            ]
            .spacing(SPACE_SM)
            .padding(Padding::from([SPACE_LG, SPACE_MD])),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    } else {
        let mut col = column![].spacing(SPACE_XS).padding(Padding {
            top: 0.0,
            right: SPACE_SM,
            bottom: SPACE_SM,
            left: SPACE_SM,
        });
        for summary in &app.sessions {
            col = col.push(session_row(summary, app, busy));
        }
        scrollable(col)
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
    };

    // Header fixed top, list fills the rest — no cwd footer.
    let body = column![header, list]
        .width(Length::Fill)
        .height(Length::Fill);

    container(body)
        .style(sidebar_style)
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

    // Single-line title with ellipsis — budget chars from sidebar width.
    let max_chars = ((app.sidebar_w - 72.0) / 7.0).clamp(12.0, 64.0) as usize;
    let mut title = ellipsize(&summary.title, max_chars);
    if summary.pinned {
        title = format!("★ {title}");
        title = ellipsize(&title, max_chars + 2);
    }

    let when = relative_time(summary.updated);
    let project = short_path(&summary.cwd);
    let meta = if when.is_empty() {
        project
    } else if project.is_empty() {
        when
    } else {
        format!("{project} · {when}")
    };

    let live_dot: Element<'a, Msg> = if summary.live {
        text("●")
            .size(10)
            .style(kit_text::success)
            .into()
    } else {
        Space::new().width(0).into()
    };

    let title_row = row![
        live_dot,
        text(title)
            .font(fonts::ui())
            .size(12)
            .wrapping(Wrapping::None),
    ]
    .spacing(SPACE_XS)
    .align_y(Alignment::Center);

    let label = column![
        title_row,
        kit_text::caption(meta).style(kit_text::muted),
    ]
    .spacing(1.0)
    .width(Length::Fill);

    let mut item = button(label)
        .width(Length::Fill)
        .padding(Padding::from([SPACE_SM + 2.0, SPACE_MD]))
        .style(kit_btn::list_item(selected));
    if !busy {
        item = item.on_press(Msg::SelectSession(summary.id.clone()));
    }

    let pin_label = if summary.pinned { "★" } else { "☆" };
    let pin = button(text(pin_label).size(12))
        .padding(Padding::from([4, 6]))
        .style(kit_btn::ghost)
        .on_press(Msg::TogglePin(summary.id.clone()));

    let rename = button(text("✎").size(11))
        .padding(Padding::from([4, 6]))
        .style(kit_btn::ghost)
        .on_press(Msg::StartRename(summary.id.clone()));

    row![item, rename, pin]
        .spacing(SPACE_XS)
        .align_y(Alignment::Center)
        .padding(Padding {
            top: 0.0,
            right: SPACE_XS,
            bottom: 0.0,
            left: SPACE_XS,
        })
        .into()
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
            // Prefer leaf project name when deep: ~/Workspace/Sola → Sola
            // but keep ~/Workspace/Sola/.worktrees/foo recognizable.
            let short = format!("~{rest}");
            return short;
        }
    }
    p.to_string()
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
        "just now".into()
    } else if age < 3600 {
        format!("{}m ago", age / 60)
    } else if age < 86400 {
        format!("{}h ago", age / 3600)
    } else if age < 86400 * 14 {
        format!("{}d ago", age / 86400)
    } else {
        chrono::DateTime::from_timestamp(updated as i64, 0)
            .map(|d| d.format("%b %e").to_string())
            .unwrap_or_default()
    }
}
