//! Titlebar — the floating window, not an isolated strip.

use iced::widget::{column, container, text};
use iced::{Background, Border, Color, Element, Length, Theme};

use sola_kit::components::text::{body, muted};
use sola_kit::components::titlebar::floating_frame;

use crate::storybook::Msg;
use crate::storybook::pages::chrome::{lede, scene};

pub fn view() -> Element<'static, Msg> {
    // Full-bleed list + bottom band: would square the bottom corners if
    // the face only AABB-clipped. The rounded punch should keep the fill
    // inside the curve against the page behind the demo.
    let bleed = container(
        column![
            row_block("Inbox"),
            row_block("Drafts"),
            row_block("Sent"),
            container(text("").size(1))
                .width(Length::Fill)
                .height(Length::Fill),
            container(body("Last row meets the curve"))
                .padding([10, 14])
                .width(Length::Fill)
                .style(|theme: &Theme| {
                    let p = theme.extended_palette();
                    iced::widget::container::Style {
                        background: Some(Background::Color(p.primary.base.color)),
                        text_color: Some(p.primary.base.text),
                        ..Default::default()
                    }
                }),
        ]
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|theme: &Theme| {
        let p = theme.extended_palette();
        iced::widget::container::Style {
            background: Some(Background::Color(p.background.strong.color)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        }
    });

    let framed = container(floating_frame(
        "Kit",
        Msg::Noop,
        Msg::Noop,
        |_| Msg::Noop,
        bleed.into(),
    ))
    .width(Length::Fixed(420.0))
    .height(Length::Fixed(220.0));

    column![
        lede(
            "Titlebar",
            "macOS-adjacent float chrome: traffic-light close, centered title, rounded rectangle on all four corners. The body is full-bleed on purpose — if the clip were only the AABB, the accent band would square the bottom ears.",
        ),
        scene("Floating frame"),
        framed,
        text("Drag and close are inert here — real behaviour lives on the host window.")
            .size(12)
            .style(muted),
    ]
    .spacing(16)
    .into()
}

fn row_block(label: &'static str) -> Element<'static, Msg> {
    container(body(label))
        .padding([10, 14])
        .width(Length::Fill)
        .into()
}
