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
use iced::{Border, Element, Length, Theme, mouse};

/// Container style for the divider's visible track — the hairline
/// color from the kit palette so it blends with surrounding 1px
/// borders. Pass via `.style(sola_kit::components::divider::style)`.
pub fn style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(p.background.stronger.color.into()),
        border: Border::default(),
        ..container::Style::default()
    }
}

/// 8px-wide vertical draggable divider. Caller wires the `on_press`
/// message and listens for cursor motion / release at the application
/// level — the divider itself doesn't track drag state because iced
/// has no pointer-capture API; the consumer already needs to manage
/// cursor + release events to handle the race where the cursor
/// escapes the divider's hit-rect during a fast drag (see
/// sola-monitor's `App` for the canonical pattern).
pub fn vertical_divider<'a, Message>(on_press: Message) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    mouse_area(
        container(Space::new().width(Length::Fill).height(Length::Fill))
            .style(style)
            .width(Length::Fixed(8.0))
            .height(Length::Fill),
    )
    .interaction(mouse::Interaction::ResizingColumn)
    .on_press(on_press)
    .into()
}
