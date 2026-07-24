//! Transcript turns — graphite chat bubbles, tool groups, muted thought.

use iced::widget::{column, container, row, text, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::badge::{self, Tone};
use sola_kit::components::style::{RADIUS_LG, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use crate::protocol::{ToolTurn, Turn};
use crate::view::markdown;
use crate::Msg;

const BUBBLE_MAX: f32 = 960.0;
const USER_BODY_PX: f32 = 14.0;

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
        Turn::Thought(s) => thought_block(s, theme),
        Turn::Tool(t) => tool_group_summary_from_tools(&[t], theme),
        Turn::Plan(entries) => plan_card(entries, theme),
        Turn::Error(s) => error_block(s),
    }
}

fn role_label(name: &str) -> Element<'static, Msg> {
    text(name.to_uppercase())
        .font(fonts::ui_medium())
        .size(11)
        .style(kit_text::muted)
        .into()
}

fn user_bubble(body: &str, theme: &Theme) -> Element<'static, Msg> {
    let p = theme.extended_palette();
    let selection = sola_kit::theme::selection();
    let bg = Color {
        r: selection.r * 0.55 + p.background.weaker.color.r * 0.45,
        g: selection.g * 0.55 + p.background.weaker.color.g * 0.45,
        b: selection.b * 0.55 + p.background.weaker.color.b * 0.45,
        a: 1.0,
    };
    let border = Color {
        a: 0.18,
        ..p.primary.base.color
    };
    let card = container(
        text(body.to_string())
            .font(fonts::ui())
            .size(USER_BODY_PX)
            .wrapping(iced::widget::text::Wrapping::Word),
    )
    .padding(Padding::from([12.0, 14.0]))
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

    column![role_label("You"), card]
        .spacing(6.0)
        .width(Length::Fill)
        .into()
}

fn agent_bubble(body: &str, theme: &Theme) -> Element<'static, Msg> {
    let p = theme.extended_palette();
    let bg = Color {
        a: 0.92,
        ..p.background.weaker.color
    };
    let border = Color {
        a: 0.55,
        ..p.background.stronger.color
    };
    let card = container(markdown::render(body, theme))
        .padding(Padding::from([12.0, 14.0]))
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

    column![role_label("Agent"), card]
        .spacing(6.0)
        .width(Length::Fill)
        .into()
}

fn thought_block(body: &str, theme: &Theme) -> Element<'static, Msg> {
    let mute = theme.extended_palette().secondary.base.text;
    let bar = container(Space::new().width(Length::Fixed(2.0)).height(Length::Fill))
        .width(Length::Fixed(2.0))
        .height(Length::Fill)
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(Color {
                a: 0.35,
                ..mute
            })),
            ..container::Style::default()
        });

    let content = column![
        text("THINKING")
            .font(fonts::ui_medium())
            .size(11)
            .style(kit_text::muted),
        text(body.to_string())
            .font(fonts::ui())
            .size(12.5)
            .style(kit_text::muted)
            .wrapping(iced::widget::text::Wrapping::Word),
    ]
    .spacing(SPACE_XS)
    .padding(Padding {
        top: 2.0,
        right: 0.0,
        bottom: 2.0,
        left: 12.0,
    });

    container(row![bar, content].width(Length::Fill))
        .width(Length::Fill)
        .max_width(BUBBLE_MAX)
        .into()
}

fn tool_group_summary(slice: &[Turn], theme: &Theme) -> Element<'static, Msg> {
    let tools: Vec<&ToolTurn> = slice
        .iter()
        .filter_map(|t| match t {
            Turn::Tool(tt) => Some(tt),
            _ => None,
        })
        .collect();
    tool_group_summary_from_tools(&tools, theme)
}

fn tool_group_summary_from_tools(tools: &[&ToolTurn], theme: &Theme) -> Element<'static, Msg> {
    let n = tools.len();
    let label = if n == 1 {
        tools
            .first()
            .map(|t| t.tool.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "tool".into())
    } else {
        format!("{n} tool uses")
    };

    let mut running = 0usize;
    let mut failed = 0usize;
    let mut cancelled = 0usize;
    for t in tools {
        let s = t.status.to_ascii_lowercase();
        if s.contains("fail") || s.contains("error") {
            failed += 1;
        } else if s.contains("cancel") {
            cancelled += 1;
        } else if s.contains("complet") || s == "success" || s == "ok" || s == "done" {
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
            Tone::Warning,
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

    let name = text(label)
        .font(fonts::mono())
        .size(12)
        .style(|t: &Theme| {
            let w = t.extended_palette().warning.base.color;
            iced::widget::text::Style {
                color: Some(Color {
                    r: (w.r * 0.75 + 1.0 * 0.25).min(1.0),
                    g: (w.g * 0.75 + 1.0 * 0.25).min(1.0),
                    b: (w.b * 0.75 + 1.0 * 0.25).min(1.0),
                    a: 1.0,
                }),
            }
        });

    let header = row![
        name,
        Space::new().width(Length::Fill),
        badge::badge(status_label, tone),
    ]
    .spacing(SPACE_SM)
    .align_y(Alignment::Center)
    .padding(Padding::from([8.0, 12.0]));

    let p = theme.extended_palette();
    let bg = p.background.weaker.color;
    let border = Color {
        a: 0.55,
        ..p.background.stronger.color
    };
    container(header)
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
        })
        .into()
}

fn plan_card(entries: &[crate::protocol::PlanEntry], theme: &Theme) -> Element<'static, Msg> {
    let p = theme.extended_palette();
    let accent = p.primary.base.color;
    let mut lines = column![
        text("NEXT")
            .font(fonts::ui_medium())
            .size(12)
            .style(move |_t: &Theme| iced::widget::text::Style {
                color: Some(Color {
                    r: (accent.r * 0.75 + 1.0 * 0.25).min(1.0),
                    g: (accent.g * 0.75 + 1.0 * 0.25).min(1.0),
                    b: (accent.b * 0.75 + 1.0 * 0.25).min(1.0),
                    a: 1.0,
                }),
            })
    ]
    .spacing(SPACE_SM);

    for e in entries {
        let mark = match e.status.as_str() {
            "completed" => "✓",
            "in_progress" | "in-progress" => "→",
            _ => "·",
        };
        lines = lines.push(kit_text::body(format!("{mark}  {}", e.content)));
    }

    let selection = sola_kit::theme::selection();
    let bg = Color {
        r: selection.r * 0.45 + p.background.weaker.color.r * 0.55,
        g: selection.g * 0.45 + p.background.weaker.color.g * 0.55,
        b: selection.b * 0.45 + p.background.weaker.color.b * 0.55,
        a: 1.0,
    };
    let border = Color {
        a: 0.16,
        ..accent
    };
    container(lines.padding(Padding::from([12.0, 14.0])))
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
