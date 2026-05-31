//! Card — an elevated container with kit chrome (raised bg, hairline
//! border, rounded corners, kit padding).
//!
//! `card(content)` returns an `iced::widget::Container` so the caller
//! can chain further iced methods (`.width(Fill)`, `.padding(...)`,
//! `.center_x()`, …). The style fn is exposed separately for callers
//! who already have a container and only want the kit chrome.

use iced::widget::{Container, container};
use iced::{Background, Element, Theme};

use crate::components::style::{hairline, RADIUS_LG, SPACE_XL};

/// Wrap `content` in a card-styled container. Default padding is 16px;
/// override with `.padding(...)` on the returned container if needed.
pub fn card<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
) -> Container<'a, Message, Theme> {
    container(content).style(style).padding(SPACE_XL)
}

pub fn style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weaker.color)),
        border: hairline(p, RADIUS_LG),
        ..container::Style::default()
    }
}
