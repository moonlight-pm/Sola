//! Popover — a menu sitting next to its trigger.

use iced::Element;
use iced::widget::{column, row};

use sola_kit::components::button as kit_btn;
use sola_kit::components::popover;
use sola_kit::components::text::{body, muted};

use crate::storybook::Msg;
use crate::storybook::pages::chrome::{lede, panel};

pub fn view() -> Element<'static, Msg> {
    let trigger = kit_btn::labeled("Open menu", kit_btn::secondary).on_press(Msg::Noop);
    let menu = popover(
        column![
            body("Revert"),
            body("Duplicate"),
            body("Delete").style(muted),
        ]
        .spacing(6),
    )
    .width(180);

    column![
        lede(
            "Popover",
            "Menu chrome: raised face, hairline, tight shadow. Anchor and dismiss stay with the caller.",
        ),
        panel(row![trigger, menu].spacing(16).align_y(iced::Alignment::Start)),
    ]
    .spacing(16)
    .into()
}
