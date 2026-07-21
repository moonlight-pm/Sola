//! Split / column dividers — hairline paint, fat hit target.
//!
//! The draggable dividers used by [`crate::components::split`] (and the
//! kit sidebar resize edge) reserve [`DIVIDER_HIT_PX`] of layout so
//! nested terminal splits stay easy to grab. Only a centered 1px
//! hairline is painted; the rest of the strip is the canvas background
//! so it does not read as a darker gutter. macOS Terminal / Xcode
//! inspector splits are the reference: quiet line, resize cursor on
//! hover, no hover fill.
//!
//! **Layout note:** do not call [`iced::widget::Container::center_x`] /
//! `center_y` with `Length::Fill` to center the hairline — those helpers
//! *replace* width/height with the argument, which collapses a fixed hit
//! strip to a flex `Fill` (≈1px between `FillPortion` panes). Use
//! [`align_x`] / [`align_y`] with an explicit `Fixed(DIVIDER_HIT_PX)`.
//!
//! Drag state stays with the caller (iced has no pointer-capture): the
//! divider emits `on_press`, and the consumer listens for that plus
//! global cursor motion / release — see `sola-terminal` and
//! `sola-monitor` for the canonical pattern.
//!
//! Cursor interaction is `ResizingColumn` / `ResizingRow` (not the
//! generic horizontal/vertical resize shapes) because those map via
//! winit→sctk to `col-resize` / `row-resize` XDG names. The generic
//! `ew-resize` / `ns-resize` shapes are absent from most cursor themes
//! (McMojave included), and wlroots silently substitutes default when
//! the requested name isn't found.

use iced::alignment::{Horizontal, Vertical};
use iced::widget::{Space, container, mouse_area};
use iced::{Background, Border, Element, Length, Theme, mouse};

/// Layout thickness of the draggable divider strip (logical px). The
/// visible hairline is 1px centered inside this; consumers that compute
/// pane rects from a split tree (terminal) must use the same value.
pub const DIVIDER_HIT_PX: f32 = 8.0;

const LINE_PX: f32 = 1.0;

/// Style for the 1px painted hairline — border atom, no radius.
pub fn line_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.stronger.color)),
        border: Border::default(),
        ..container::Style::default()
    }
}

/// Hit-strip chrome — canvas base so the reserved strip is not a darker
/// transparent hole between panes.
fn hit_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weakest.color)),
        border: Border::default(),
        ..container::Style::default()
    }
}

/// Draggable vertical divider: [`DIVIDER_HIT_PX`]-wide hit strip with a
/// centered 1px hairline. Caller wires `on_press` and tracks cursor
/// motion / release at the application level.
pub fn vertical_divider<'a, Message>(on_press: Message) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    let line = container(Space::new().width(Length::Fill).height(Length::Fill))
        .style(line_style)
        .width(Length::Fixed(LINE_PX))
        .height(Length::Fill);

    mouse_area(
        container(line)
            .style(hit_style)
            .width(Length::Fixed(DIVIDER_HIT_PX))
            .height(Length::Fill)
            .align_x(Horizontal::Center),
    )
    .interaction(mouse::Interaction::ResizingColumn)
    .on_press(on_press)
    .into()
}

/// Draggable horizontal divider — row counterpart of
/// [`vertical_divider`], for stacked (column) splits. Same consumer-
/// managed drag contract and hairline-in-hit-strip geometry.
pub fn horizontal_divider_drag<'a, Message>(on_press: Message) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    let line = container(Space::new().width(Length::Fill).height(Length::Fill))
        .style(line_style)
        .width(Length::Fill)
        .height(Length::Fixed(LINE_PX));

    mouse_area(
        container(line)
            .style(hit_style)
            .width(Length::Fill)
            .height(Length::Fixed(DIVIDER_HIT_PX))
            .align_y(Vertical::Center),
    )
    .interaction(mouse::Interaction::ResizingRow)
    .on_press(on_press)
    .into()
}

/// A non-interactive 1px horizontal divider line, the same hairline
/// colour as kit borders. The horizontal counterpart of the split
/// dividers without the drag affordance.
pub fn horizontal_divider<'a, Message: 'a>() -> Element<'a, Message, Theme> {
    container(Space::new().width(Length::Fill).height(Length::Fixed(LINE_PX)))
        .width(Length::Fill)
        .height(Length::Fixed(LINE_PX))
        .style(line_style)
        .into()
}
