//! Card — product surfaces, not an API sampler.

use iced::widget::{column, container, row};
use iced::{Element, Length};

use sola_kit::components::button as kit_btn;
use sola_kit::components::text::{body, caption, muted};
use sola_kit::components::{card, modal, plain};
use sola_kit::components::card as card_mod;

use crate::storybook::Msg;
use crate::storybook::pages::chrome::{lede, panel, scene};

pub fn view() -> Element<'static, Msg> {
    let note = card(
        column![
            body("Note"),
            body("Default card — raised face, hairline, quiet shadow. Use for a single grouped thought.")
                .style(muted),
        ]
        .spacing(6),
    )
    .width(Length::Fill);

    let stacked = column![
        plain(
            body("Plain — raised, no border. Quieter when cards stack.")
                .style(muted),
        )
        .width(Length::Fill),
        plain(
            body("Another plain row. Hairlines would chatter here.")
                .style(muted),
        )
        .width(Length::Fill),
    ]
    .spacing(8);

    let dialog = modal(
        column![
            body("Discard unsaved theme?"),
            caption("This cannot be undone.").style(muted),
            row![
                kit_btn::labeled_sm("Cancel", kit_btn::secondary).on_press(Msg::Noop),
                kit_btn::labeled_sm("Discard", kit_btn::danger_outline).on_press(Msg::Noop),
            ]
            .spacing(8),
        ]
        .spacing(12),
    )
    .padding(18)
    .width(Length::Fixed(320.0));

    let tiles = row![
        container(caption("Selected").style(muted))
            .padding([14, 18])
            .style(card_mod::list_tile_style(true)),
        container(caption("Idle").style(muted))
            .padding([14, 18])
            .style(card_mod::list_tile_style(false)),
    ]
    .spacing(8);

    column![
        lede(
            "Card",
            "Elevation from graphite steps. Default keeps a hairline; plain drops it; modal is a dialog.",
        ),
        scene("Note"),
        note,
        scene("Stacked"),
        stacked,
        scene("Dialog"),
        dialog,
        scene("Switcher tile"),
        panel(column![
            body("Soft plate under a large icon — graphite selection, not teal.")
                .style(muted),
            tiles,
        ]
        .spacing(10)),
    ]
    .spacing(16)
    .into()
}
