//! Welcome / index page — first thing the storybook shows on launch.
//!
//! Stays intentionally text-only. The sidebar lists every component
//! page so a user lands here, reads the orientation, then clicks
//! through.

use iced::widget::column;
use iced::Element;

use sola_kit::components::text::{body, heading, muted};

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    column![
        heading("Welcome"),
        body(
            "sola-kit ships reusable iced widgets, named style fns, the \
             canonical palette, and the boilerplate every iced app would \
             otherwise repeat (bus connect, app menu, font registration, \
             window settings)."
        ),
        body(
            "The storybook dogfoods every component. Pick a page on the \
             left to see it rendered against the current theme."
        )
        .style(muted),
    ]
    .spacing(12)
    .into()
}
