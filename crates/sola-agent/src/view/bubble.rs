use iced::widget::{column, container};
use iced::{Alignment, Background, Border, Color, Element, Length, Theme};
use sola_kit::components::style::{RADIUS_LG, SPACE_LG, SPACE_MD, SPACE_SM};
use sola_kit::components::text as kit_text;

use super::tool;
use crate::{Msg, Turn};

pub(crate) fn turn_view<'a>(turn: &'a Turn, theme: &Theme) -> Element<'a, Msg> {
    match turn {
        Turn::User(t) => bubble("You", t.as_str(), Alignment::End, role_bg(theme, true), theme),
        Turn::Assistant { text: body, .. } => {
            bubble("Agent", body.as_str(), Alignment::Start, role_bg(theme, false), theme)
        }
        Turn::Reasoning(t) => reasoning(t.as_str()),
        Turn::Tool(tt) => tool::tool_view(tt, theme),
        Turn::Error(m) => error_view(m.as_str()),
    }
}

fn role_bg(theme: &Theme, user: bool) -> Color {
    let p = theme.extended_palette();
    if user {
        p.primary.weak.color
    } else {
        p.background.weak.color
    }
}

fn bubble<'a>(
    label: &'a str,
    body: &str,
    align: Alignment,
    bg: Color,
    theme: &Theme,
) -> Element<'a, Msg> {
    let border = theme.extended_palette().background.strong.color;
    let inner = column![
        kit_text::caption(label.to_string()),
        kit_text::body(body.to_string()),
    ]
    .spacing(SPACE_SM);
    let card = container(inner)
        .padding(SPACE_LG)
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
    container(card).width(Length::Fill).align_x(align).into()
}

fn reasoning<'a>(body: &str) -> Element<'a, Msg> {
    container(
        column![
            kit_text::caption("Reasoning").style(kit_text::muted),
            kit_text::body(body.to_string()).style(kit_text::muted),
        ]
        .spacing(SPACE_SM)
        .padding(SPACE_MD + SPACE_SM),
    )
    .width(Length::Fill)
    .into()
}

fn error_view<'a>(msg: &str) -> Element<'a, Msg> {
    container(
        column![
            kit_text::caption("Error").style(kit_text::danger),
            kit_text::body(msg.to_string()).style(kit_text::danger),
        ]
        .spacing(SPACE_SM)
        .padding(SPACE_MD + SPACE_SM),
    )
    .width(Length::Fill)
    .into()
}
