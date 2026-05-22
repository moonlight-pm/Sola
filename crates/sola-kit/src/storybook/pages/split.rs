//! Split showcase — two-pane layout with the kit divider in between.
//! The position is static in the showcase (consumer manages drag state
//! in a real app); the visual is what matters here.

use iced::widget::{column, container};
use iced::{Element, Length};

use sola_kit::components::card::style as card_style;
use sola_kit::components::split;
use sola_kit::components::text::{body, code, heading, muted};

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    let left = container(body("Left pane — fixed width").style(muted))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill);
    let right = container(body("Right pane — fills remainder").style(muted))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill);

    let demo = container(split(left, 240.0, Msg::Noop, right))
        .style(card_style)
        .height(Length::Fixed(240.0))
        .width(Length::Fill);

    column![
        heading("Split"),
        body("Two-pane row with the kit divider — consumer manages drag state.").style(muted),
        demo,
        code("split(left, left_w, on_drag, right)").style(muted),
    ]
    .spacing(16)
    .into()
}
