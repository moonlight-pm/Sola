//! Theme page — palette swatches for every atom in
//! `sola_kit::theme::hex`. Quick visual check that the hex values
//! still mean what they should, plus a reference for which iced slot
//! each atom binds to.
//!
//! Future work: when `Topic::Theme` is wired into the iced kit, this
//! page will render the live theme from the bus instead of the
//! compile-time constants.

use iced::widget::{column, row};
use iced::Element;

use sola_kit::components::swatch::swatch_sized;
use sola_kit::components::text::{body, caption, code, heading, muted};
use sola_kit::theme::{self, hex};

use crate::storybook::Msg;

const SWATCH_SIZE: f32 = 56.0;

pub fn view() -> Element<'static, Msg> {
    let atoms: &[(&str, &str, &str)] = &[
        ("BG",        hex::BG,        "background.base / weakest"),
        ("BG_RAISED", hex::BG_RAISED, "background.weaker / weak"),
        ("BG_HOVER",  hex::BG_HOVER,  "background.neutral / strong"),
        ("BORDER",    hex::BORDER,    "background.stronger / strongest"),
        ("FG",        hex::FG,        "palette.text"),
        ("FG_MUTED",  hex::FG_MUTED,  "secondary.base.text"),
        ("ACCENT",    hex::ACCENT,    "primary.base"),
        ("SUCCESS",   hex::SUCCESS,   "success.base"),
        ("WARNING",   hex::WARNING,   "warning.base"),
        ("DANGER",    hex::DANGER,    "danger.base"),
    ];

    let grid = atoms.chunks(5).fold(column![].spacing(16), |col, chunk| {
        let r = chunk.iter().fold(row![].spacing(16), |r, (name, hex_str, slot)| {
            r.push(swatch_tile(name, hex_str, slot))
        });
        col.push(r)
    });

    column![
        heading("Theme"),
        body(
            "Canonical palette atoms. Component style fns read these via \
             theme.extended_palette() — the binding from atom to iced slot \
             lives in sola_kit::theme::sola_extended."
        )
        .style(muted),
        grid,
    ]
    .spacing(16)
    .into()
}

fn swatch_tile<'a>(name: &'a str, hex_str: &'a str, slot: &'a str) -> Element<'a, Msg> {
    column![
        swatch_sized(theme::parse(hex_str), SWATCH_SIZE),
        body(name),
        code(hex_str).style(muted),
        caption(slot).style(muted),
    ]
    .spacing(4)
    .width(iced::Length::Fixed(SWATCH_SIZE + 16.0))
    .into()
}
