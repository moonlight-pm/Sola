//! Badge — status in a product row, not a tone catalog.

use iced::alignment::{Horizontal, Vertical};
use iced::widget::{column, container, row, stack};
use iced::{Element, Length};

use sola_kit::components::text::{body, muted};
use sola_kit::components::{Tone, badge, count_mark, icon};

use crate::storybook::Msg;
use crate::storybook::pages::chrome::{lede, panel};

pub fn view() -> Element<'static, Msg> {
    let slot = Length::Fixed(72.0);
    let marked: Element<'_, Msg> = stack![
        container(icon::<Msg>("lucide/bell", 48))
            .width(slot)
            .height(slot)
            .center_x(slot)
            .center_y(slot),
        container(count_mark::<Msg>(12))
            .width(slot)
            .height(slot)
            .align_x(Horizontal::Right)
            .align_y(Vertical::Top)
            .padding(2),
    ]
    .into();

    column![
        lede(
            "Badge",
            "Status pills stay graphite with neon type. Count marks are a filled accent disc — switcher icons and notification groups.",
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
        panel(
            column![
                body("Count mark").style(muted),
                row![
                    count_mark::<Msg>(1),
                    count_mark::<Msg>(9),
                    count_mark::<Msg>(12),
                    count_mark::<Msg>(99),
                    count_mark::<Msg>(140),
                    marked,
                ]
                .spacing(12)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(10),
        ),
    ]
    .spacing(16)
    .into()
}
