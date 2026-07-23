//! Transcript turns — chat bubbles, tool cards, muted thought, errors.

use iced::widget::{column, container, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::badge::{self, Tone};
use sola_kit::components::style::{RADIUS_LG, RADIUS_MD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use crate::protocol::Turn;
use crate::Msg;

pub(crate) fn turn_view<'a>(turn: &'a Turn, theme: &Theme) -> Element<'a, Msg> {
    match turn {
        Turn::User(s) => user_bubble(s, theme),
        Turn::Assistant(s) => agent_bubble(s, theme),
        Turn::Thought(s) => thought_block(s),
        Turn::Tool(t) => tool_card(t, theme),
        Turn::Plan(entries) => plan_card(entries, theme),
        Turn::Error(s) => error_block(s),
    }
}

fn user_bubble(body: &str, theme: &Theme) -> Element<'static, Msg> {
    let bg = theme.extended_palette().primary.weak.color;
    let border = theme.extended_palette().primary.base.color;
    let border = Color {
        a: 0.25,
        ..border
    };
    let card = container(
        column![
            kit_text::caption("You").style(kit_text::muted),
            text(body.to_string())
                .font(fonts::ui())
                .size(13)
                .wrapping(iced::widget::text::Wrapping::Word),
        ]
        .spacing(SPACE_SM),
    )
    .padding(Padding::from([SPACE_MD, SPACE_LG]))
    .max_width(560.0)
    .style(move |_t: &Theme| container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: border,
            width: 1.0,
            radius: RADIUS_LG.into(),
        },
        ..container::Style::default()
    });

    container(card)
        .width(Length::Fill)
        .align_x(Alignment::End)
        .into()
}

fn agent_bubble(body: &str, theme: &Theme) -> Element<'static, Msg> {
    let bg = theme.extended_palette().background.weaker.color;
    let border = theme.extended_palette().background.stronger.color;
    let card = container(
        column![
            kit_text::caption("Grok").style(kit_text::muted),
            text(body.to_string())
                .font(fonts::ui())
                .size(13)
                .wrapping(iced::widget::text::Wrapping::Word),
        ]
        .spacing(SPACE_SM),
    )
    .padding(Padding::from([SPACE_MD, SPACE_LG]))
    .max_width(620.0)
    .style(move |_t: &Theme| container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: border,
            width: 1.0,
            radius: RADIUS_LG.into(),
        },
        ..container::Style::default()
    });

    container(card)
        .width(Length::Fill)
        .align_x(Alignment::Start)
        .into()
}

fn thought_block(body: &str) -> Element<'static, Msg> {
    container(
        column![
            kit_text::caption("Thinking").style(kit_text::muted),
            kit_text::body(body.to_string()).style(kit_text::muted),
        ]
        .spacing(SPACE_XS)
        .padding(Padding::from([SPACE_SM, SPACE_MD])),
    )
    .width(Length::Fill)
    .max_width(620.0)
    .into()
}

fn tool_card(t: &crate::protocol::ToolTurn, theme: &Theme) -> Element<'static, Msg> {
    let status = t.status.to_lowercase();
    let tone = if status.contains("fail") || status.contains("error") {
        Tone::Danger
    } else if status.contains("complet") || status == "success" {
        Tone::Success
    } else if status.contains("cancel") {
        Tone::Neutral
    } else {
        Tone::Accent
    };
    let status_label = if t.status.is_empty() {
        "running".to_string()
    } else {
        t.status.clone()
    };

    let header = row![
        kit_text::caption("Tool").style(kit_text::muted),
        iced::widget::Space::new().width(Length::Fill),
        badge::badge(status_label, tone),
    ]
    .spacing(SPACE_SM)
    .align_y(Alignment::Center);

    let mut body = column![header, kit_text::body(t.tool.clone())].spacing(SPACE_SM);

    let args = pretty_args(&t.args);
    if !args.is_empty() {
        body = body.push(
            text(args)
                .font(fonts::mono())
                .size(11)
                .style(kit_text::muted)
                .wrapping(iced::widget::text::Wrapping::Word),
        );
    }
    if !t.output.is_empty() {
        body = body.push(
            text(truncate(&t.output, 1200))
                .font(fonts::mono())
                .size(11)
                .style(kit_text::muted)
                .wrapping(iced::widget::text::Wrapping::Word),
        );
    }

    let bg = theme.extended_palette().background.weaker.color;
    let border = theme.extended_palette().background.stronger.color;
    container(body.padding(Padding::from([SPACE_MD, SPACE_LG])))
        .width(Length::Fill)
        .max_width(620.0)
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: border,
                width: 1.0,
                radius: RADIUS_MD.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn plan_card(entries: &[crate::protocol::PlanEntry], theme: &Theme) -> Element<'static, Msg> {
    let mut lines = column![kit_text::caption("Plan").style(kit_text::muted)].spacing(SPACE_SM);
    for e in entries {
        let mark = match e.status.as_str() {
            "completed" => "✓",
            "in_progress" | "in-progress" => "→",
            _ => "·",
        };
        lines = lines.push(kit_text::body(format!("{mark}  {}", e.content)));
    }
    let bg = theme.extended_palette().background.weaker.color;
    let border = theme.extended_palette().background.stronger.color;
    container(lines.padding(Padding::from([SPACE_MD, SPACE_LG])))
        .width(Length::Fill)
        .max_width(620.0)
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: border,
                width: 1.0,
                radius: RADIUS_MD.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn error_block(msg: &str) -> Element<'static, Msg> {
    container(
        column![
            kit_text::caption("Error").style(kit_text::danger),
            kit_text::body(msg.to_string()).style(kit_text::danger),
        ]
        .spacing(SPACE_SM)
        .padding(Padding::from([SPACE_MD, SPACE_LG])),
    )
    .width(Length::Fill)
    .max_width(620.0)
    .into()
}

fn pretty_args(v: &serde_json::Value) -> String {
    if v.is_null() {
        return String::new();
    }
    match serde_json::to_string_pretty(v) {
        Ok(s) => truncate(&s, 400),
        Err(_) => truncate(&v.to_string(), 400),
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
