//! Card showcase — elevated panel chrome on top of the canvas BG.

use iced::widget::column;
use iced::{Element, Length};

use sola_kit::components::{accent_backplate, card, modal};
use sola_kit::components::text::{body, code, heading, muted};

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    let demo = card(
        column![
            body("This is a card."),
            body(
                "Default padding is 16px; chain .padding(...) on the returned \
                 container to override. Background, border, and radius are \
                 themed via the card style fn."
            )
            .style(muted),
        ]
        .spacing(8),
    )
    .width(Length::Fill);

    let modal_demo = modal(
        body("Modal card — opaque weaker bg, hairline at RADIUS_XL (14px), deep shadow.")
            .style(muted),
    )
    .padding(24)
    .width(Length::Fill);

    let backplate_demo = accent_backplate(
        body("Accent backplate — primary-tinted fill and border at 16px, deep shadow.")
            .style(muted),
    )
    .padding(24)
    .width(Length::Fill);

    column![
        heading("Card"),
        body("Container with BG_RAISED, 1px BORDER, 8px corner radius.").style(muted),
        demo,
        code("card(content)").style(muted),
        body("Modal card chrome — opaque panel lifted over a dimmed backdrop.").style(muted),
        modal_demo,
        code("modal(content)").style(muted),
        body("Accent backplate — primary-tinted switcher frame.").style(muted),
        backplate_demo,
        code("accent_backplate(content)").style(muted),
    ]
    .spacing(16)
    .into()
}
