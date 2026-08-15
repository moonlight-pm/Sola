//! Readable — a measure, shown as a column of copy.

use iced::widget::column;
use iced::{Element, Length};

use sola_kit::components::readable;
use sola_kit::components::text::{body, muted};

use crate::storybook::Msg;
use crate::storybook::pages::chrome::{lede, panel};

pub fn view() -> Element<'static, Msg> {
    column![
        lede(
            "Readable",
            "Cap the measure. Long copy stays ~65ch instead of spanning the window.",
        ),
        readable(
            panel(
                column![
                    body("How this machine names you"),
                    body(
                        "This column is capped at 480px and centered. Resize the window — \
                         the measure holds until the viewport drops below the cap."
                    )
                    .style(muted),
                ]
                .spacing(8),
            )
            .width(Length::Fill),
            480.0,
        ),
    ]
    .spacing(16)
    .into()
}
