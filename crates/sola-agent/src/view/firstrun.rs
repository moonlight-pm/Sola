//! Setup guidance when Grok is missing or initialize failed.

use iced::widget::{column, container, text};
use iced::{Element, Length, Padding};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_XL};
use sola_kit::components::text as kit_text;

use crate::{App, Msg};

pub(crate) fn view(app: &App) -> Element<'_, Msg> {
    let msg = app
        .need_setup
        .as_deref()
        .unwrap_or("Grok agent is not available.");
    container(
        column![
            kit_text::heading("Set up Grok"),
            text(msg).size(13),
            text(
                "Install Grok Build (https://x.ai/cli), run `grok login`, \
                 or set XAI_API_KEY. Then click Retry."
            )
            .size(12),
            kit_btn::labeled("Retry", kit_btn::primary).on_press(Msg::Restart),
        ]
        .spacing(SPACE_MD)
        .padding(Padding::new(SPACE_XL + SPACE_LG)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
