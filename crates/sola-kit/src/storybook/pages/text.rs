//! Typography showcase — every size + tone helper.

use iced::widget::column;
use iced::Element;

use sola_kit::components::card;
use sola_kit::components::text::{
    accent, body, caption, code, danger, heading, muted, subheading, success, warning,
};

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    let sizes = card(
        column![
            heading("Heading — 24px condensed-bold"),
            subheading("Subheading — 18px condensed-bold"),
            body("Body — 14px normal"),
            caption("Caption — 11px normal").style(muted),
            code("Code — 12px JetBrains Mono"),
        ]
        .spacing(8),
    );

    let tones = card(
        column![
            body("Default body color"),
            body("Muted text").style(muted),
            body("Accent text").style(accent),
            body("Success text").style(success),
            body("Warning text").style(warning),
            body("Danger text").style(danger),
        ]
        .spacing(4),
    );

    column![
        heading("Text"),
        body("Typography helpers paired with named color style fns.").style(muted),
        sizes,
        tones,
    ]
    .spacing(16)
    .into()
}
