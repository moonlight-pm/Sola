//! Draggable column divider — the 8px gutter sola-monitor uses
//! between its messages table and its sticky-topics sidebar.
//!
//! The cursor interaction is `ResizingColumn` (not
//! `ResizingHorizontally`) because that's what maps via
//! winit→sctk to `Shape::ColResize` whose XDG cursor name is
//! `col-resize`. The generic `ew-resize` shape that
//! `ResizingHorizontally` requests is absent from most cursor
//! themes (McMojave included), and wlroots silently substitutes
//! default when the requested name isn't found.

use iced::widget::{Space, container, mouse_area};
use iced::{Border, Element, Length, mouse};

use crate::theme;

/// Container style for the divider's visible track. References the
/// kit's BORDER atom so the divider blends with the surrounding
/// hairlines. Pass via `.style(divider_style)`.
pub fn divider_style(_t: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(theme::parse(theme::hex::BORDER).into()),
        border: Border::default(),
        ..container::Style::default()
    }
}

/// 8px-wide vertical draggable divider. Caller wires the
/// `on_press` message and listens for cursor motion / release at
/// the application level — the divider itself doesn't track drag
/// state because iced has no pointer-capture API; the consumer
/// already needs to manage cursor + release events to handle the
/// race where the cursor escapes the divider's hit-rect during a
/// fast drag (see sola-monitor's `App` for the canonical pattern).
pub fn vertical_divider<'a, Message>(on_press: Message) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    mouse_area(
        container(Space::new().width(Length::Fill).height(Length::Fill))
            .style(divider_style)
            .width(Length::Fixed(8.0))
            .height(Length::Fill),
    )
    .interaction(mouse::Interaction::ResizingColumn)
    .on_press(on_press)
    .into()
}
