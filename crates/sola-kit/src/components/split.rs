//! Split — orientation-parameterized two-pane split that draws the
//! kit's draggable divider between the panes. `dir` chooses side-by-side
//! (`Vertical` → a `row!` with a vertical divider, new pane on the
//! right) or stacked (`Horizontal` → a `column!` with a horizontal
//! divider, new pane below). `ratio` is pane `a`'s fraction of the
//! split's main axis, mapped to `FillPortion` weights so the layout
//! reflows when the window resizes.
//!
//! The divider is a three-band hit strip (`a | line | b`) with a true
//! 1px centre hairline — see [`crate::components::divider`]. Pass
//! [`DividerColors`] via [`split_with`] so the side bands match the
//! surfaces being split.
//!
//! **Bordered parents:** the split fills its layout box edge-to-edge
//! (correct for terminal panes). If you wrap it in a card or other
//! hairline-bordered container, inset the content by the border width
//! (e.g. `.padding(1.0)`) so the divider does not paint over the outer
//! stroke and notch the outline at the T-junction.
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
//!
//! // Match both panes' background so only the hairline shows:
//! split_with(dir, left, ratio, Msg::Drag(id), right, DividerColors::uniform(bg, line))
//! ```

use iced::widget::{column, container, row};
use iced::{Element, Length, Theme};

use sola_bus::topics::SplitDir;

use crate::components::divider::DividerColors;
use crate::components::{
    horizontal_divider_drag, horizontal_divider_drag_with, vertical_divider,
    vertical_divider_with,
};

/// Build a two-pane split with theme-default divider colours
/// (canvas | border | canvas). Prefer [`split_with`] when adjacent
/// surfaces are not the window canvas.
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
    split_inner(dir, a.into(), ratio, on_drag, b.into(), None)
}

/// Build a two-pane split with explicit divider band colours.
pub fn split_with<'a, Message>(
    dir: SplitDir,
    a: impl Into<Element<'a, Message, Theme>>,
    ratio: f32,
    on_drag: Message,
    b: impl Into<Element<'a, Message, Theme>>,
    colors: DividerColors,
) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    split_inner(dir, a.into(), ratio, on_drag, b.into(), Some(colors))
}

fn split_inner<'a, Message>(
    dir: SplitDir,
    a: Element<'a, Message, Theme>,
    ratio: f32,
    on_drag: Message,
    b: Element<'a, Message, Theme>,
    colors: Option<DividerColors>,
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

    let divider: Element<'a, Message, Theme> = match (dir, colors) {
        (SplitDir::Vertical, None) => vertical_divider(on_drag),
        (SplitDir::Vertical, Some(c)) => vertical_divider_with(on_drag, c),
        (SplitDir::Horizontal, None) => horizontal_divider_drag(on_drag),
        (SplitDir::Horizontal, Some(c)) => horizontal_divider_drag_with(on_drag, c),
    };

    match dir {
        SplitDir::Vertical => {
            let a = container(a)
                .width(Length::FillPortion(wa))
                .height(Length::Fill);
            let b = container(b)
                .width(Length::FillPortion(wb))
                .height(Length::Fill);
            row![a, divider, b]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
        SplitDir::Horizontal => {
            let a = container(a)
                .height(Length::FillPortion(wa))
                .width(Length::Fill);
            let b = container(b)
                .height(Length::FillPortion(wb))
                .width(Length::Fill);
            column![a, divider, b]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
    }
}
