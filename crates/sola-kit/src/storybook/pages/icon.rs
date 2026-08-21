//! Icon — a toolbar strip, then scale and semantic tints.

use iced::Element;
use iced::widget::{column, row};

use sola_kit::components::text::{body, muted};
use sola_kit::components::{icon, icon_colored};
use sola_kit::theme;

use crate::storybook::Msg;
use crate::storybook::pages::chrome::{lede, panel, scene};

const NAMES: &[&str] = &[
    "lucide/settings",
    "lucide/search",
    "lucide/menu",
    "lucide/check",
    "lucide/x",
    "lucide/bell",
];

pub fn view() -> Element<'static, Msg> {
    let atoms = theme::Atoms::default();
    column![
        lede(
            "Icon",
            "Tinted with the theme foreground. Semantic color only when the status is the point.",
        ),
        scene("Toolbar"),
        panel(
            row(NAMES.iter().map(|&n| icon::<Msg>(n, 18)))
                .spacing(14)
                .align_y(iced::Alignment::Center),
        ),
        scene("Scale"),
        panel(
            row![
                icon::<Msg>("lucide/settings", 16),
                icon::<Msg>("lucide/settings", 20),
                icon::<Msg>("lucide/settings", 24),
                icon::<Msg>("lucide/settings", 32),
            ]
            .spacing(16)
            .align_y(iced::Alignment::Center),
        ),
        scene("Status tint"),
        panel(
            column![
                body("Kit atoms — not one-off hex.").style(muted),
                row![
                    icon::<Msg>("lucide/bell", 22),
                    icon_colored::<Msg>("lucide/bell", 22, atoms.success),
                    icon_colored::<Msg>("lucide/bell", 22, atoms.warning),
                    icon_colored::<Msg>("lucide/bell", 22, atoms.danger),
                    icon_colored::<Msg>("lucide/bell", 22, atoms.accent),
                ]
                .spacing(16)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(10),
        ),
    ]
    .spacing(16)
    .into()
}
