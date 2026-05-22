//! Popover showcase — the floating-panel chrome rendered inline.
//! v0 doesn't ship anchor/show-hide behavior, so the popover is shown
//! statically next to its trigger to demonstrate the visual chrome.

use iced::widget::{button, column, container, row, text};
use iced::{Element, Length};

use sola_kit::components::button as kit_btn;
use sola_kit::components::card::style as card_style;
use sola_kit::components::popover;
use sola_kit::components::text::{body, code, heading, muted};

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    let trigger = button(text("Open menu")).style(kit_btn::secondary).on_press(Msg::Noop);

    let menu = popover(
        column![
            body("Item one"),
            body("Item two"),
            body("Item three").style(muted),
        ]
        .spacing(6),
    );

    let demo = container(
        row![trigger, menu].spacing(16),
    )
    .style(card_style)
    .padding(16)
    .width(Length::Fill);

    column![
        heading("Popover"),
        body(
            "Floating-panel chrome — raised bg, hairline border, soft drop \
             shadow. v0 ships visual chrome only; anchor/show-hide logic is \
             the caller's concern (use iced::widget::Stack or Float)."
        )
        .style(muted),
        demo,
        code("popover(content)").style(muted),
    ]
    .spacing(16)
    .into()
}
