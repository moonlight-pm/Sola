//! Card showcase — elevated panel chrome on top of the canvas BG.

use iced::widget::column;
use iced::{Element, Length};

use sola_kit::components::{accent_backplate, backplate, card, modal};
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

    let custom_backplate_demo = backplate(
        body("Parameterized backplate — caller-supplied fill/border (gold @ 0.20).")
            .style(muted),
        iced::Color::from_rgba(1.0, 0.72, 0.0, 0.20),
        iced::Color::from_rgba(1.0, 0.72, 0.0, 0.40),
    )
    .padding(24)
    .width(Length::Fill);

    let tile_selected = iced::widget::container(body("Selected tile"))
        .padding([16, 20])
        .style(card::list_tile_style(true));
    let tile_unselected = iced::widget::container(body("Unselected tile"))
        .padding([16, 20])
        .style(card::list_tile_style(false));

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
        body("Parameterized backplate — shell switcher uses this with shell-* tokens.").style(muted),
        custom_backplate_demo,
        code("backplate(content, fill, border)").style(muted),
        body("Selectable tile — container analog of button::list_item for mouse_area tiles.").style(muted),
        iced::widget::row![tile_selected, tile_unselected].spacing(8),
        code("container(content).style(card::list_tile_style(selected))").style(muted),
    ]
    .spacing(16)
    .into()
}
