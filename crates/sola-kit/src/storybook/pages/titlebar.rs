//! Titlebar — the floating window, not an isolated strip.

use iced::widget::{column, container, text};
use iced::{Element, Length};

use sola_kit::components::text::{body, muted};
use sola_kit::components::titlebar::floating_frame;

use crate::storybook::Msg;
use crate::storybook::pages::chrome::{lede, scene};

pub fn view() -> Element<'static, Msg> {
    let framed = container(floating_frame(
        "Kit",
        Msg::Noop,
        Msg::Noop,
        |_| Msg::Noop,
        container(
            column![
                body("How this machine names you"),
                body("Client decorations when the window floats. Zoned windows have none.")
                    .style(muted),
            ]
            .spacing(6),
        )
        .padding(18)
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
    ))
    .width(Length::Fixed(420.0))
    .height(Length::Fixed(200.0));

    column![
        lede(
            "Titlebar",
            "macOS-adjacent float chrome: traffic-light close, centered title, rounded frame.",
        ),
        scene("Floating frame"),
        framed,
        text("Drag and close are inert here — real behaviour lives on the host window.")
            .size(12)
            .style(muted),
    ]
    .spacing(16)
    .into()
}
