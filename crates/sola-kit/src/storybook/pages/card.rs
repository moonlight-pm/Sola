//! Card showcase — elevated panel chrome on top of the canvas BG.

use iced::widget::column;
use iced::{Element, Length};

use sola_kit::components::card;
use sola_kit::components::text::{body, code, heading, muted};

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    let demo = card(
        column![
            body("This is a card."),
            body(
                "Default padding is 16px; chain .padding(...) on the returned \
                 container to override. Background, border, and radius are \
                 themed via the card style fn."
            )
            .style(muted),
        ]
        .spacing(8),
    )
    .width(Length::Fill);

    column![
        heading("Card"),
        body("Container with BG_RAISED, 1px BORDER, 8px corner radius.").style(muted),
        demo,
        code("card(content)").style(muted),
    ]
    .spacing(16)
    .into()
}
