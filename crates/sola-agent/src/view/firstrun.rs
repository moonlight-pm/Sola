//! Setup guidance when Grok is missing or initialize failed.

use iced::widget::{column, container};
use iced::{Background, Border, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{RADIUS_LG, SPACE_LG, SPACE_MD, SPACE_XL};
use sola_kit::components::text as kit_text;

use crate::{App, Msg};

pub(crate) fn view(app: &App) -> Element<'_, Msg> {
    let msg = app
        .need_setup
        .as_deref()
        .unwrap_or("Grok agent is not available.");

    let card = column![
        kit_text::heading("Set up Grok"),
        kit_text::body(msg.to_string()),
        kit_text::body(
            "sola-agent needs a shared Grok leader. \
             Ensure `grok-leader.service` is running \
             (`systemctl --user enable --now grok-leader.service`), \
             install Grok Build if needed, and run `grok login` — then Retry."
        )
        .style(kit_text::muted),
        kit_btn::labeled("Retry", kit_btn::primary).on_press(Msg::Restart),
    ]
    .spacing(SPACE_MD)
    .padding(Padding::new(SPACE_XL + SPACE_LG))
    .max_width(440.0);

    container(container(card).style(card_style))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn card_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weaker.color)),
        border: Border {
            color: p.background.stronger.color,
            width: 1.0,
            radius: RADIUS_LG.into(),
        },
        ..container::Style::default()
    }
}
