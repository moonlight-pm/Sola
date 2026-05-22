//! Divider showcase — the 8px draggable column divider that
//! `sola-monitor` uses between its messages table and topics sidebar.
//!
//! Static showcase for v0 — clicking the divider does nothing because
//! the storybook doesn't carry the drag state the consumer would.
//! That's the canonical pattern though: the divider emits an
//! `on_press` and the consumer manages cursor motion / release at the
//! application level.

use iced::widget::{column, container, row};
use iced::{Element, Length};

use sola_kit::components::card::style as card_style;
use sola_kit::components::text::{body, code, heading, muted};
use sola_kit::components::vertical_divider;

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    let left = panel("Left panel");
    let right = panel("Right panel");

    let demo = container(
        row![
            container(left).width(Length::Fixed(220.0)).height(Length::Fill),
            vertical_divider::<Msg>(Msg::Noop),
            container(right).width(Length::Fill).height(Length::Fill),
        ]
        .height(Length::Fill),
    )
    .style(card_style)
    .height(Length::Fixed(240.0))
    .width(Length::Fill);

    column![
        heading("Divider"),
        body(
            "8px-wide draggable column divider with the col-resize cursor. \
             Consumer wires drag state via cursor motion + release at the \
             application level."
        )
        .style(muted),
        demo,
        code("vertical_divider(on_press)").style(muted),
    ]
    .spacing(16)
    .into()
}

fn panel(label: &str) -> Element<'static, Msg> {
    container(body(label.to_string()))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
