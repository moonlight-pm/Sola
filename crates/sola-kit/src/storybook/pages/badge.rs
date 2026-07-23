//! Badge showcase — every tone, in a row.

use iced::widget::{column, row};
use iced::Element;

use sola_kit::components::card;
use sola_kit::components::text::{body, code, heading, muted};
use sola_kit::components::{Tone, badge};

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    let demo = card(
        row![
            badge::<Msg>("Neutral", Tone::Neutral),
            badge::<Msg>("Accent", Tone::Accent),
            badge::<Msg>("Ready", Tone::Success),
            badge::<Msg>("Pending", Tone::Warning),
            badge::<Msg>("Error", Tone::Danger),
        ]
        .spacing(8),
    );

    column![
        heading("Badge"),
        body(
            "Status pills — 10px medium, pad [2, 8]. Neutral is quiet grey + muted \
             text; status tones keep scanable fills."
        )
        .style(muted),
        demo,
        code("badge(\"Ready\", Tone::Success) · Neutral → background.strong + muted")
            .style(muted),
    ]
    .spacing(16)
    .into()
}
