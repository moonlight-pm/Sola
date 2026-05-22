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
        body("Status pills — condensed-bold label, rounded ends.").style(muted),
        demo,
        code("badge(\"Ready\", Tone::Success)").style(muted),
    ]
    .spacing(16)
    .into()
}
