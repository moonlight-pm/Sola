//! Titlebar showcase — the floating-window titlebar in isolation. Drag/close
//! are inert here (map to `Noop`); the real behaviour lives in a consumer app.

use iced::widget::{column, container, text};
use iced::{Element, Length};

use sola_kit::components::text::{caption, muted};
use sola_kit::components::titlebar::{floating_frame, titlebar};

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    let strip = container(titlebar("Floating Window", Msg::Noop, Msg::Noop))
        .width(Length::Fixed(420.0));

    let framed = container(floating_frame(
        "Floating Window",
        Msg::Noop,
        Msg::Noop,
        container(text("Content area").size(13))
            .width(Length::Fill)
            .height(Length::Fixed(160.0))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into(),
    ))
    .width(Length::Fixed(420.0))
    .height(Length::Fixed(200.0));

    column![
        text("Titlebar").size(20),
        caption(
            "macOS-adjacent float chrome: taller bar, left traffic-light close, centered title. \
             floating_frame rounds the window corners.",
        )
        .style(muted),
        text("Strip only").size(14),
        strip,
        text("Floating frame").size(14),
        framed,
    ]
    .spacing(12)
    .into()
}
