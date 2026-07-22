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
            heading("Heading — 22px display"),
            subheading("Subheading — 15px display"),
            body("Body — 13px UI"),
            caption("Caption — 11px UI").style(muted),
            code("Code — 12px mono"),
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
