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
use iced::{Background, Element, Shadow, Theme, Vector};

use crate::components::style::{hairline, RADIUS_LG, SPACE_LG};

/// Wrap `content` in a popover-styled container. Default padding is
/// 12px; override with `.padding(...)` if needed.
pub fn popover<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
) -> Container<'a, Message, Theme> {
    container(content).style(style).padding(SPACE_LG)
}

pub fn style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weaker.color)),
        border: hairline(p, RADIUS_LG),
        // Escape hatch: iced's palette vocabulary carries no shadow
        // token, so the drop shadow is a fixed translucent black rather
        // than a themed atom. This is the one non-palette colour in the
        // kit's components (see the convention note in `mod.rs`); a
        // floating panel needs the lift regardless of theme.
        shadow: Shadow {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 16.0,
        },
        ..container::Style::default()
    }
}
