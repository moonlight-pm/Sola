//! Shared storybook page chrome — heading + lede + quiet panel.

use iced::widget::{column, container, text};
use iced::{Border, Color, Element, Length, Theme};

use sola_kit::components::style::{RADIUS_XL, bevel_frame, stage_fill};
use sola_kit::components::text::{body, heading, muted};

/// Page title + one muted sentence.
pub fn lede<'a, Message: 'a>(
    title: &'static str,
    blurb: &'static str,
) -> iced::widget::Column<'a, Message, Theme> {
    column![heading(title), body(blurb).style(muted)].spacing(8)
}

/// Quiet raised panel (same whisper as Overview desks).
pub fn panel<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
) -> iced::widget::Container<'a, Message, Theme> {
    let face = container(content)
        .padding(18)
        .width(Length::Fill)
        .style(|theme: &Theme| {
            let p = theme.extended_palette();
            iced::widget::container::Style {
                background: Some(stage_fill(
                    p.background.base.color,
                    p.background.weaker.color,
                    p.primary.base.color,
                )),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: RADIUS_XL.into(),
                },
                ..Default::default()
            }
        });
    container(face)
        .padding(1)
        .width(Length::Fill)
        .style(|theme: &Theme| {
            bevel_frame(theme.extended_palette().background.weaker.color, RADIUS_XL)
        })
}

/// Small section label inside a page.
pub fn scene<'a>(label: &'static str) -> iced::widget::Text<'a, Theme> {
    text(label).font(sola_kit::fonts::ui_medium()).size(13)
}
