//! Session sidebar — kit raised surface, scrollable list, pin + relative time.

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Alignment, Element, Length, Padding};
use sola_kit::components::button as kit_btn;
use sola_kit::components::sidebar::{SIDEBAR_WIDTH, style as sidebar_style};
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use crate::protocol::SessionSummary;
use crate::{App, Msg};

pub(crate) fn view(app: &App) -> Element<'_, Msg> {
    let busy = app.streaming || app.pending.is_some();

    let mut new_btn = kit_btn::labeled_sm("New", kit_btn::secondary);
    if !busy {
        new_btn = new_btn.on_press(Msg::NewSession);
    }

    let header = row![
        kit_text::subheading("Sessions"),
        Space::new().width(Length::Fill),
        new_btn,
    ]
    .spacing(SPACE_SM)
    .align_y(Alignment::Center)
    .padding(Padding::from([SPACE_MD, SPACE_MD]));

    let list: Element<'_, Msg> = if app.sessions.is_empty() {
        container(
            column![
                kit_text::body("No sessions yet").style(kit_text::muted),
                kit_text::caption(
                    "New starts a Grok conversation for this project. \
                     Existing TUI sessions appear here too."
                )
                .style(kit_text::muted),
            ]
            .spacing(SPACE_SM)
            .padding(Padding::from([SPACE_LG, SPACE_MD])),
        )
        .width(Length::Fill)
        .into()
    } else {
        let mut col = column![].spacing(SPACE_XS).padding(Padding {
            top: 0.0,
            right: SPACE_SM,
            bottom: 0.0,
            left: SPACE_SM,
        });
        for summary in &app.sessions {
            col = col.push(session_row(summary, app, busy));
        }
        scrollable(col).height(Length::Fill).into()
    };

    let cwd = short_path(&app.project_root.to_string_lossy());
    let footer = container(
        kit_text::caption(cwd).style(kit_text::muted),
    )
    .padding(Padding::from([SPACE_MD, SPACE_MD]))
    .width(Length::Fill);

    let body = column![header, list, Space::new().height(Length::Fill), footer]
        .width(Length::Fill)
        .height(Length::Fill);

    container(body)
        .style(sidebar_style)
        .width(Length::Fixed(SIDEBAR_WIDTH + 40.0))
        .height(Length::Fill)
        .into()
}

fn session_row<'a>(summary: &'a SessionSummary, app: &'a App, busy: bool) -> Element<'a, Msg> {
    let selected = app.session_id.as_deref() == Some(summary.id.as_str());
    let pin_mark = if summary.pinned { "★ " } else { "" };
    let title = format!("{pin_mark}{}", summary.title);
    let when = relative_time(summary.updated);

    let label = column![
        text(title)
            .font(fonts::ui())
            .size(12)
            .wrapping(iced::widget::text::Wrapping::Word),
        kit_text::caption(when).style(kit_text::muted),
    ]
    .spacing(SPACE_XS)
    .width(Length::Fill);

    let mut item = button(label)
        .width(Length::Fill)
        .padding(Padding::from([SPACE_SM + 2.0, SPACE_MD]))
        .style(kit_btn::list_item(selected));
    if !busy {
        item = item.on_press(Msg::SelectSession(summary.id.clone()));
    }

    let pin_label = if summary.pinned { "★" } else { "☆" };
    let pin = kit_btn::labeled_sm(pin_label, kit_btn::ghost)
        .on_press(Msg::TogglePin(summary.id.clone()));

    row![item, pin]
        .spacing(SPACE_XS)
        .align_y(Alignment::Center)
        .into()
}

fn short_path(p: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if let Some(rest) = p.strip_prefix(&home) {
            return format!("~{rest}");
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
        // short date
        chrono::DateTime::from_timestamp(updated as i64, 0)
            .map(|d| d.format("%b %e").to_string())
            .unwrap_or_default()
    }
}
