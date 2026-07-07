use iced::widget::{column, container, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};

use crate::{Msg, Turn};

pub(crate) fn turn_view<'a>(turn: &'a Turn, theme: &Theme) -> Element<'a, Msg> {
    match turn {
        Turn::User(t) => bubble("You", t.as_str(), Alignment::End, role_bg(theme, true), theme),
        Turn::Assistant { text: body, .. } => {
            bubble("Agent", body.as_str(), Alignment::Start, role_bg(theme, false), theme)
        }
        Turn::Reasoning(t) => reasoning(t.as_str()),
        // Placeholder tool render; Task 28 swaps this for tool::tool_view.
        Turn::Tool(tt) => bubble(
            "Tool",
            &format!("{}\n{}", tt.tool, tt.output),
            Alignment::Start,
            role_bg(theme, false),
            theme,
        ),
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
        text(label.to_string()).font(sola_kit::fonts::ui_medium()).size(12),
        text(body.to_string()).size(15),
    ]
    .spacing(4);
    let card = container(inner)
        .padding(Padding::new(12.0))
        .max_width(560.0)
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: border,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..container::Style::default()
        });
    container(card).width(Length::Fill).align_x(align).into()
}

fn reasoning<'a>(body: &str) -> Element<'a, Msg> {
    container(
        column![
            text("Reasoning")
                .font(sola_kit::fonts::ui_medium())
                .size(11)
                .style(sola_kit::components::text::muted),
            text(body.to_string()).size(13).style(sola_kit::components::text::muted),
        ]
        .spacing(4)
        .padding(Padding::new(10.0)),
    )
    .width(Length::Fill)
    .into()
}

fn error_view<'a>(msg: &str) -> Element<'a, Msg> {
    container(
        column![
            text("Error")
                .font(sola_kit::fonts::ui_medium())
                .size(12)
                .style(sola_kit::components::text::danger),
            text(msg.to_string()).size(14).style(sola_kit::components::text::danger),
        ]
        .spacing(4)
        .padding(Padding::new(10.0)),
    )
    .width(Length::Fill)
    .into()
}
