//! Split — orientation-parameterized two-pane split that draws the
//! kit's draggable divider between the panes. `dir` chooses side-by-side
//! (`Vertical` → a `row!` with a vertical divider, new pane on the
//! right) or stacked (`Horizontal` → a `column!` with a horizontal
//! divider, new pane below). `ratio` is pane `a`'s fraction of the
//! split's main axis, mapped to `FillPortion` weights so the layout
//! reflows when the window resizes.
//!
//! Drag state stays with the caller (iced has no pointer-capture): the
//! divider emits `on_drag` on press, and the consumer's update fn
//! listens for that plus global cursor motion / release to compute the
//! new `ratio` — see `sola-monitor::App` and `sola-terminal` for the
//! canonical pattern.
//!
//! ```ignore
//! enum Msg { DividerDrag(SplitId), Other }
//!
//! split(SplitDir::Vertical, left, state.ratio, Msg::DividerDrag(id), right)
//! ```

use iced::widget::{column, container, row};
use iced::{Element, Length, Theme};

use sola_bus::topics::SplitDir;

use crate::components::{horizontal_divider_drag, vertical_divider};

/// Build a two-pane split in the given orientation. `ratio` is pane
/// `a`'s fraction of the split's main axis in `(0, 1)` — clamped to a
/// sane band and converted to integer `FillPortion` weights so the
/// split reflows on resize. The divider emits `on_drag` on press; the
/// caller tracks the cursor to turn the drag into a new `ratio`.
pub fn split<'a, Message>(
    dir: SplitDir,
    a: impl Into<Element<'a, Message, Theme>>,
    ratio: f32,
    on_drag: Message,
    b: impl Into<Element<'a, Message, Theme>>,
) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    // Convert `ratio` into integer FillPortion weights. The clamp keeps
    // both panes visible even if a caller passes an extreme ratio; the
    // authoritative minimum-pane clamp lives in the consumer (which
    // knows pixel/cell sizes).
    let r = ratio.clamp(0.05, 0.95);
    let wa = (r * 1000.0).round() as u16;
    let wb = 1000u16.saturating_sub(wa).max(1);

    match dir {
        SplitDir::Vertical => {
            let a = container(a.into())
                .width(Length::FillPortion(wa))
                .height(Length::Fill);
            let b = container(b.into())
                .width(Length::FillPortion(wb))
                .height(Length::Fill);
            row![a, vertical_divider(on_drag), b]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
        SplitDir::Horizontal => {
            let a = container(a.into())
                .height(Length::FillPortion(wa))
                .width(Length::Fill);
            let b = container(b.into())
                .height(Length::FillPortion(wb))
                .width(Length::Fill);
            column![a, horizontal_divider_drag(on_drag), b]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
    }
}
