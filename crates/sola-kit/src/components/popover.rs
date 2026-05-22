//! Popover — visual chrome for a floating panel.
//!
//! v0 ships *only* the chrome (raised bg, hairline border, drop
//! shadow, padding). Show/hide and anchoring are the caller's
//! responsibility — iced 0.14's `widget::Stack` + `widget::float` (or
//! a plain `Stack` with conditional rendering) handles that better
//! than the kit could prescribe.
//!
//! Once we have a kit consumer that wants the full
//! trigger+anchor+dismiss pattern boxed up, we'll grow this into a
//! stateful widget. Today the consumer composes the chrome with its
//! own positioning logic.

use iced::widget::{Container, container};
use iced::{Background, Border, Element, Shadow, Theme, Vector};

/// Wrap `content` in a popover-styled container. Default padding is
/// 12px; override with `.padding(...)` if needed.
pub fn popover<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
) -> Container<'a, Message, Theme> {
    container(content).style(style).padding(12)
}

/// Style fn for the popover surface. Background is BG_RAISED, border
/// is the hairline color, and a soft drop shadow lifts the panel off
/// whatever surface is behind it. Corner radius matches the kit card.
pub fn style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weaker.color)),
        border: Border {
            color: p.background.stronger.color,
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Shadow {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 16.0,
        },
        ..container::Style::default()
    }
}
