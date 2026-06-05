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

    let list_items = row![
        button(text("Selected row")).style(kit_btn::list_item(true)).on_press(Msg::Noop).width(200),
        button(text("Unselected row")).style(kit_btn::list_item(false)).on_press(Msg::Noop).width(200),
    ]
    .spacing(8);

    // Menubar demo: dark bar container with rest / active buttons.
    let menubar_bar = container(
        row![
            button(text("Apple")).style(kit_btn::menubar(false)).on_press(Msg::Noop),
            button(text("File")).style(kit_btn::menubar(false)).on_press(Msg::Noop),
            button(text("Edit")).style(kit_btn::menubar(true)).on_press(Msg::Noop),
        ]
        .spacing(2),
    )
    .style(|_theme| iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color::BLACK)),
        ..Default::default()
    })
    .padding(4)
    .width(Length::Fill);

    let demo = container(
        column![
            caption("Interactive").style(muted),
            buttons,
            caption("Disabled (no on_press)").style(muted),
            disabled,
            caption("List item — selected / unselected").style(muted),
            list_items,
            caption("Menubar — rest / active (\"Edit\" is open)").style(muted),
            menubar_bar,
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
