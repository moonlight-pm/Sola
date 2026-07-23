//! Transcript turn bubbles.

use iced::widget::{column, container, text};
use iced::{Element, Length, Padding, Theme};
use sola_kit::components::style::{SPACE_SM, SPACE_MD};
use sola_kit::components::text as kit_text;

use crate::protocol::Turn;

pub(crate) fn turn_view<'a>(turn: &'a Turn, _theme: &Theme) -> Element<'a, crate::Msg> {
    match turn {
        Turn::User(s) => bubble("You", s.to_string(), false),
        Turn::Assistant(s) => bubble("Agent", s.to_string(), false),
        Turn::Thought(s) => bubble("Thinking", s.to_string(), true),
        Turn::Tool(t) => {
            let body = format!(
                "{}\n{}\n{}",
                t.tool,
                pretty_args(&t.args),
                if t.output.is_empty() {
                    t.status.clone()
                } else {
                    truncate(&t.output, 2000)
                }
            );
            bubble("Tool", body, true)
        }
        Turn::Plan(entries) => {
            let body: String = entries
                .iter()
                .map(|e| format!("• [{}] {}", e.status, e.content))
                .collect::<Vec<_>>()
                .join("\n");
            bubble("Plan", body, true)
        }
        Turn::Error(s) => bubble("Error", s.to_string(), false),
    }
}

fn bubble(role: &'static str, body: String, muted: bool) -> Element<'static, crate::Msg> {
    let role_el: Element<'static, crate::Msg> = if muted {
        kit_text::caption(role).into()
    } else {
        kit_text::subheading(role).into()
    };
    container(
        column![role_el, text(body).size(13)]
            .spacing(SPACE_SM)
            .padding(Padding::new(SPACE_MD)),
    )
    .width(Length::Fill)
    .into()
}

fn pretty_args(v: &serde_json::Value) -> String {
    if v.is_null() {
        String::new()
    } else {
        truncate(&v.to_string(), 400)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{t}…")
    }
}
