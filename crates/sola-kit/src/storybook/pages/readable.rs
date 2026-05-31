//! Readable showcase — a max-width centered column for long-form / form
//! content.

use iced::widget::column;
use iced::{Element, Length};

use sola_kit::components::card;
use sola_kit::components::readable;
use sola_kit::components::text::{body, code, heading, muted};

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    let demo = readable(
        card(body(
            "This column is capped at 480px and centered in whatever space \
             it's given, so long-form text keeps a comfortable measure \
             instead of spanning the full window width. Resize the window \
             and the column stays put until the viewport drops below the cap.",
        ))
        .width(Length::Fill),
        480.0,
    );

    column![
        heading("Readable"),
        body("Max-width centered column — the readable-measure primitive apps were hand-rolling.")
            .style(muted),
        demo,
        code("readable(content, 480.0)").style(muted),
    ]
    .spacing(16)
    .into()
}
