//! Icon showcase — themed SVG icons at a few sizes, plus the
//! explicit-color override.
//!
//! Icons resolve from `/opt/sola/share/icons/<pack>/<name>.svg` at call
//! time; a missing file renders as an empty box of the requested size
//! (so this page degrades gracefully on a box without the icon pack
//! synced). For repeatedly-rendered icons, prefer
//! `icon_handle` + `icon_svg` over `icon` to avoid per-frame disk reads.

use iced::widget::{column, row};
use iced::{Color, Element};

use sola_kit::components::text::{body, code, heading, muted, subheading};
use sola_kit::components::{card, icon, icon_colored};

use crate::storybook::Msg;

const NAMES: &[&str] = &[
    "lucide/settings",
    "lucide/search",
    "lucide/menu",
    "lucide/check",
    "lucide/x",
    "lucide/bell",
];

pub fn view() -> Element<'static, Msg> {
    let sizes = card(
        row(NAMES.iter().map(|&n| icon::<Msg>(n, 24)))
            .spacing(16)
            .align_y(iced::Alignment::Center),
    );

    let scale = card(
        row![
            icon::<Msg>("lucide/settings", 16),
            icon::<Msg>("lucide/settings", 24),
            icon::<Msg>("lucide/settings", 32),
            icon::<Msg>("lucide/settings", 48),
        ]
        .spacing(16)
        .align_y(iced::Alignment::Center),
    );

    let tinted = card(
        row![
            icon::<Msg>("lucide/bell", 24),
            icon_colored::<Msg>("lucide/bell", 24, Color::from_rgb8(0x3f, 0xb9, 0x50)),
            icon_colored::<Msg>("lucide/bell", 24, Color::from_rgb8(0xd2, 0x99, 0x22)),
            icon_colored::<Msg>("lucide/bell", 24, Color::from_rgb8(0xf8, 0x51, 0x49)),
        ]
        .spacing(16)
        .align_y(iced::Alignment::Center),
    );

    column![
        heading("Icon"),
        body(
            "SVG icons tinted with the active theme's foreground by \
             default; `icon_colored` overrides the tint."
        )
        .style(muted),
        subheading("Default tint"),
        sizes,
        code("icon(\"lucide/settings\", 24)").style(muted),
        subheading("Sizes"),
        scale,
        code("icon(name, 16 | 24 | 32 | 48)").style(muted),
        subheading("Explicit color"),
        tinted,
        code("icon_colored(\"lucide/bell\", 24, color)").style(muted),
    ]
    .spacing(16)
    .into()
}
