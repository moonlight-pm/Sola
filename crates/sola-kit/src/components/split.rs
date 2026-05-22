//! Split — two-pane row layout with the kit's draggable divider in
//! between. Left pane is fixed-width; right pane fills the remainder.
//!
//! Drag state stays with the caller (iced has no pointer-capture).
//! The pattern is:
//!
//! ```ignore
//! enum Msg { DividerDrag, Other }
//!
//! split(left_view, state.left_w, Msg::DividerDrag, right_view)
//! ```
//!
//! The consumer's update fn listens for `DividerDrag` + cursor motion
//! to compute the new width — see `sola-monitor::App` for the
//! canonical implementation.

use iced::widget::row;
use iced::{Element, Length, Theme};

use crate::components::vertical_divider;

/// Build a horizontal two-pane split. `left_width` is the fixed pixel
/// width of the left pane; the divider sits at that boundary; the
/// right pane fills the remainder.
pub fn split<'a, Message>(
    left: impl Into<Element<'a, Message, Theme>>,
    left_width: f32,
    divider_msg: Message,
    right: impl Into<Element<'a, Message, Theme>>,
) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    let left = iced::widget::container(left.into())
        .width(Length::Fixed(left_width))
        .height(Length::Fill);
    let right = iced::widget::container(right.into())
        .width(Length::Fill)
        .height(Length::Fill);
    row![left, vertical_divider(divider_msg), right]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
