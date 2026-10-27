//! Scroll — the kit's scrollbar: a pill in a rounded channel.

use iced::widget::{column, container, text};
use iced::{Element, Length};

use sola_kit::components::scroll::{self, MARGIN, RAIL_W, THUMB_W};
use sola_kit::components::text::{body, muted};

use crate::storybook::Msg;
use crate::storybook::pages::chrome::{lede, panel};

pub fn view() -> Element<'static, Msg> {
    let rows = column((1..=40).map(|i| {
        text(format!("Row {i} — scroll to see the pill slide in its channel"))
            .size(13)
            .into()
    }))
    .spacing(8)
    .width(Length::Fill);
    column![
        lede(
            "Scroll",
            "scroll::scrollable — the channel shows only while the content overflows; \
             the pill brightens under the pointer and while held.",
        ),
        body(format!(
            "Channel {RAIL_W} px, pill {THUMB_W} px, {MARGIN} px from the edge; ends rounded."
        ))
        .style(muted),
        panel(container(scroll::scrollable(rows)).height(Length::Fixed(260.0)))
            .width(Length::Fill),
    ]
    .spacing(16)
    .into()
}
