//! Titlebar showcase — the floating-window titlebar in isolation. Drag/close
//! are inert here (map to `Noop`); the real behaviour lives in a consumer app.

use iced::widget::{column, container, text};
use iced::{Element, Length};

use sola_kit::components::text::{caption, muted};
use sola_kit::components::titlebar::titlebar;

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    let demo = container(titlebar("Floating Window", Msg::Noop, Msg::Noop))
        .width(Length::Fixed(420.0));

    column![
        text("Titlebar").size(20),
        caption("Drawn in-window by a floating kit app. Bar drags to move; ✕ closes.")
            .style(muted),
        demo,
    ]
    .spacing(12)
    .into()
}
