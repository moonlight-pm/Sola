//! Divider — two panes, one hairline.

use iced::widget::{column, container, row};
use iced::{Element, Length};

use sola_kit::components::text::body;
use sola_kit::components::{DividerColors, horizontal_divider, vertical_divider_with};

use crate::storybook::Msg;
use crate::storybook::pages::chrome::{lede, panel, scene};

pub fn view() -> Element<'static, Msg> {
    let chrome = DividerColors::raised(&sola_kit::theme::default_theme());

    let vertical = container(
        row![
            container(body("Inbox"))
                .padding(16)
                .width(Length::Fixed(200.0))
                .height(Length::Fill),
            vertical_divider_with::<Msg>(Msg::Noop, chrome),
            container(body("Message"))
                .padding(16)
                .width(Length::Fill)
                .height(Length::Fill),
        ]
        .height(Length::Fill),
    )
    .height(Length::Fixed(200.0))
    .width(Length::Fill);

    let horizontal = column![
        container(body("List")).padding(16).width(Length::Fill),
        horizontal_divider::<Msg>(),
        container(body("Detail")).padding(16).width(Length::Fill),
    ];

    column![
        lede(
            "Divider",
            "8px hit strip, 1px hairline. Vertical is draggable in apps; this page is static.",
        ),
        scene("Columns"),
        panel(vertical),
        scene("Rows"),
        panel(horizontal),
    ]
    .spacing(16)
    .into()
}
