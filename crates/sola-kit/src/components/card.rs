//! Card — an elevated container with kit chrome (raised bg, hairline
//! border, rounded corners, kit padding).
//!
//! `card(content)` returns an `iced::widget::Container` so the caller
//! can chain further iced methods (`.width(Fill)`, `.padding(...)`,
//! `.center_x()`, …). The style fn is exposed separately for callers
//! who already have a container and only want the kit chrome.

use iced::widget::{Container, container};
use iced::{Background, Border, Element, Theme};

/// Wrap `content` in a card-styled container. Default padding is 16px;
/// override with `.padding(...)` on the returned container if needed.
pub fn card<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
) -> Container<'a, Message, Theme> {
    container(content).style(style).padding(16)
}

/// Style fn for the card container. Background is the raised panel
/// surface (BG_RAISED), border is the hairline color, corners are
/// rounded at 8px.
pub fn style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weaker.color)),
        border: Border {
            color: p.background.stronger.color,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    }
}
