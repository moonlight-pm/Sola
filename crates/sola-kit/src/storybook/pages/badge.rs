//! Badge — status in a product row, not a tone catalog.

use iced::widget::{column, row};
use iced::Element;

use sola_kit::components::text::{body, muted};
use sola_kit::components::{Tone, badge};

use crate::storybook::Msg;
use crate::storybook::pages::chrome::{lede, panel};

pub fn view() -> Element<'static, Msg> {
    column![
        lede(
            "Badge",
            "Status pills. Accent is neon type on graphite — never a darkened-cyan fill.",
        ),
        panel(
            column![
                body("Session").style(muted),
                row![
                    badge::<Msg>("DEFAULT", Tone::Accent),
                    badge::<Msg>("CLEAN", Tone::Success),
                    badge::<Msg>("DIRTY", Tone::Warning),
                    badge::<Msg>("ERROR", Tone::Danger),
                    badge::<Msg>("IDLE", Tone::Neutral),
                ]
                .spacing(8),
            ]
            .spacing(10),
        ),
    ]
    .spacing(16)
    .into()
}
