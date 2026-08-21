//! Readable — center content in a max-width column: the "don't let prose
//! or a form span 2000px" primitive that apps were hand-rolling.
//!
//! Wraps `content` in a horizontally-centered container capped at a
//! maximum width; narrower viewports just use the full width.

use iced::widget::{Container, container};
use iced::{Element, Length, Theme};

/// Center `content` horizontally and cap it at `max_width` logical px.
/// Returns a `Container` so the caller can still chain `.padding(..)` /
/// `.height(..)` (matches the kit's container-shaped return-type rule).
pub fn readable<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
    max_width: f32,
) -> Container<'a, Message, Theme> {
    container(content)
        .max_width(max_width)
        .center_x(Length::Fill)
}
