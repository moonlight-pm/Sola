//! Transcript turns — chat bubbles, collapsed tool groups, muted thought, errors.

use iced::widget::{column, container, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::badge::{self, Tone};
use sola_kit::components::style::{RADIUS_LG, RADIUS_MD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use crate::protocol::Turn;
use crate::view::markdown;
use crate::Msg;

/// Comfortable bubble max on wide panes (Phase E raised from ~560/620).
const BUBBLE_MAX: f32 = 960.0;
const BODY_PX: f32 = 15.0;

/// Render turns, collapsing contiguous tool uses into a single summary line.
pub(crate) fn turns_view<'a>(turns: &'a [Turn], theme: &Theme) -> Vec<Element<'a, Msg>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < turns.len() {
        if matches!(&turns[i], Turn::Tool(_)) {
            let start = i;
            while i < turns.len() && matches!(&turns[i], Turn::Tool(_)) {
                i += 1;
            }
            out.push(tool_group_summary(&turns[start..i], theme));
        } else {
            out.push(turn_view(&turns[i], theme));
            i += 1;
        }
    }
    out
}

fn turn_view<'a>(turn: &'a Turn, theme: &Theme) -> Element<'a, Msg> {
    match turn {
        Turn::User(s) => user_bubble(s, theme),
        Turn::Assistant(s) => agent_bubble(s, theme),
        Turn::Thought(s) => thought_block(s),
        // Contiguous tools are handled in `turns_view`; this is a fallback only.
        Turn::Tool(t) => tool_group_line(1, &[t.status.as_str()], theme),
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
                .size(BODY_PX)
                .wrapping(iced::widget::text::Wrapping::Word),
        ]
        .spacing(SPACE_SM),
    )
    .padding(Padding::from([SPACE_MD, SPACE_LG]))
    .max_width(BUBBLE_MAX)
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
            markdown::render(body, theme),
        ]
        .spacing(SPACE_SM),
    )
    .padding(Padding::from([SPACE_MD, SPACE_LG]))
    .width(Length::Fill)
    .max_width(BUBBLE_MAX)
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
    .max_width(BUBBLE_MAX)
    .into()
}

/// One compact line for N contiguous tool uses — no args/output body.
/// Count updates live as more tool turns land in the contiguous run.
fn tool_group_summary(slice: &[Turn], theme: &Theme) -> Element<'static, Msg> {
    let statuses: Vec<&str> = slice
        .iter()
        .filter_map(|t| match t {
            Turn::Tool(tt) => Some(tt.status.as_str()),
            _ => None,
        })
        .collect();
    tool_group_line(statuses.len(), &statuses, theme)
}

fn tool_group_line(n: usize, statuses: &[&str], theme: &Theme) -> Element<'static, Msg> {
    let label = if n == 1 {
        "1 tool use".to_string()
    } else {
        format!("{n} tool uses")
    };

    let mut running = 0usize;
    let mut failed = 0usize;
    let mut cancelled = 0usize;
    for status in statuses {
        let s = status.to_lowercase();
        if s.contains("fail") || s.contains("error") {
            failed += 1;
        } else if s.contains("cancel") {
            cancelled += 1;
        } else if s.is_empty()
            || s == "running"
            || s == "pending"
            || s.contains("in_progress")
            || s.contains("in-progress")
            || s == "inprogress"
        {
            running += 1;
        }
    }

    let (status_label, tone) = if running > 0 {
        (
            if running == n {
                "running".to_string()
            } else {
                format!("{running} running")
            },
            Tone::Accent,
        )
    } else if failed > 0 {
        (
            if failed == n {
                "failed".to_string()
            } else {
                format!("{failed} failed")
            },
            Tone::Danger,
        )
    } else if cancelled == n && n > 0 {
        ("cancelled".to_string(), Tone::Neutral)
    } else {
        ("done".to_string(), Tone::Success)
    };

    let header = row![
        kit_text::body(label).style(kit_text::muted),
        iced::widget::Space::new().width(Length::Fill),
        badge::badge(status_label, tone),
    ]
    .spacing(SPACE_SM)
    .align_y(Alignment::Center);

    let bg = theme.extended_palette().background.weaker.color;
    let border = theme.extended_palette().background.stronger.color;
    container(header.padding(Padding::from([SPACE_SM, SPACE_MD])))
        .width(Length::Fill)
        .max_width(BUBBLE_MAX)
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
        .max_width(BUBBLE_MAX)
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
    .max_width(BUBBLE_MAX)
    .into()
}


