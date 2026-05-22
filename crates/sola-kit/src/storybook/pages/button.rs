//! Button showcase — every kit-named style fn shown side-by-side so
//! the visual hierarchy reads at a glance.

use iced::widget::{button, column, container, row, text};
use iced::{Element, Length};

use sola_kit::components::button as kit_btn;
use sola_kit::components::card::style as card_style;
use sola_kit::components::text::{body, caption, code, heading, muted};

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    let buttons = row![
        button(text("Primary")).style(kit_btn::primary).on_press(Msg::Noop),
        button(text("Secondary")).style(kit_btn::secondary).on_press(Msg::Noop),
        button(text("Ghost")).style(kit_btn::ghost).on_press(Msg::Noop),
        button(text("Danger")).style(kit_btn::danger).on_press(Msg::Noop),
    ]
    .spacing(8);

    let disabled = row![
        button(text("Primary")).style(kit_btn::primary),
        button(text("Secondary")).style(kit_btn::secondary),
        button(text("Ghost")).style(kit_btn::ghost),
        button(text("Danger")).style(kit_btn::danger),
    ]
    .spacing(8);

    let demo = container(
        column![
            caption("Interactive").style(muted),
            buttons,
            caption("Disabled (no on_press)").style(muted),
            disabled,
        ]
        .spacing(12),
    )
    .style(card_style)
    .padding(16)
    .width(Length::Fill);

    column![
        heading("Button"),
        body("Named style fns — same shape as iced's built-in button::primary.").style(muted),
        demo,
        code("button(label).style(sola_kit::components::button::primary)").style(muted),
    ]
    .spacing(16)
    .into()
}
