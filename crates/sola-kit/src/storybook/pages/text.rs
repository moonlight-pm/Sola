//! Typography — roles in a reading column, tones on a second.

use iced::widget::column;
use iced::Element;

use sola_kit::components::text::{
    accent, body, caption, code, danger, heading, muted, prose, subheading, success, warning,
};

use crate::storybook::Msg;
use crate::storybook::pages::chrome::{lede, panel, scene};

pub fn view() -> Element<'static, Msg> {
    column![
        lede(
            "Text",
            "Roles, not ad-hoc sizes. Display for rare emphasis; UI for everything else; mono for data.",
        ),
        scene("Roles"),
        panel(column![
            heading("Heading · 22 display"),
            subheading("Subheading · 15 display"),
            prose("Prose · 14 — mail bodies, long-form reading"),
            body("Body · 13 — settings rows, dialogs, lists"),
            caption("Caption · 11 — help, secondary labels").style(muted),
            code("Code · 12 mono — IDs, hex, detail panels"),
        ]
        .spacing(8)),
        scene("Tones"),
        panel(column![
            body("Primary"),
            body("Muted").style(muted),
            body("Accent — neon, full chroma").style(accent),
            body("Success").style(success),
            body("Warning").style(warning),
            body("Danger").style(danger),
        ]
        .spacing(6)),
    ]
    .spacing(16)
    .into()
}
