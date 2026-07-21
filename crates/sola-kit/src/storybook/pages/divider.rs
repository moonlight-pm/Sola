//! Divider showcase — three-band hit strip (a | line | b) with a 1px
//! hairline. Static showcase for v0 — clicking the divider does nothing
//! because the storybook doesn't carry the drag state the consumer would.

use iced::widget::{column, container, row};
use iced::{Element, Length};

use sola_kit::components::card::style as card_style;
use sola_kit::components::text::{body, code, heading, muted};
use sola_kit::components::{DividerColors, horizontal_divider, vertical_divider_with};

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    let chrome = DividerColors::raised(&sola_kit::theme::default_theme());

    let left = panel("Left panel");
    let right = panel("Right panel");

    let vertical_demo = container(
        row![
            container(left).width(Length::Fixed(220.0)).height(Length::Fill),
            vertical_divider_with::<Msg>(Msg::Noop, chrome),
            container(right).width(Length::Fill).height(Length::Fill),
        ]
        .height(Length::Fill),
    )
    .style(card_style)
    .height(Length::Fixed(240.0))
    .width(Length::Fill);

    let top = panel("Top section");
    let bottom = panel("Bottom section");

    let horizontal_demo = container(
        column![
            container(top).height(Length::Fixed(60.0)).width(Length::Fill),
            horizontal_divider::<Msg>(),
            container(bottom).height(Length::Fixed(60.0)).width(Length::Fill),
        ],
    )
    .style(card_style)
    .width(Length::Fill);

    column![
        heading("Divider"),
        body(
            "Draggable column divider: 8px hit strip, 1px hairline, consumer-owned \
             a | line | b colours. col-resize cursor; consumer wires drag via cursor \
             motion + release."
        )
        .style(muted),
        vertical_demo,
        code("vertical_divider_with(on_press, DividerColors::raised(theme))").style(muted),
        body("1px horizontal hairline. Non-interactive — no resize semantics.")
            .style(muted),
        horizontal_demo,
        code("horizontal_divider()").style(muted),
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
