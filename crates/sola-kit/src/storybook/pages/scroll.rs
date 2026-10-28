//! Scroll — the kit's scrollbar: an accent pill inset in a dark channel.

use iced::widget::{column, container, text};
use iced::{Element, Length};

use sola_kit::components::scroll::{self, INSET, MARGIN, RAIL_W, THUMB_W};
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
             the pill takes the theme's accent and brightens under the pointer and \
             while held.",
        ),
        body(format!(
            "Dark channel {RAIL_W} px with a faint rim, {MARGIN} px from the edge; pill {THUMB_W} px, \
             {INSET} px in from the channel on every side; ends rounded."
        ))
        .style(muted),
        panel(container(scroll::scrollable(rows)).height(Length::Fixed(260.0)))
            .width(Length::Fill),
    ]
    .spacing(16)
    .into()
}
