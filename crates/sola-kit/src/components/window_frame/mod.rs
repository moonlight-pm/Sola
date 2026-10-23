//! The frame every kit window draws (Next, boards 01 and 02): a 38 px
//! titlebar carrying the app's menus, 12 px corners, and a 1 px rim that
//! lights while the window has focus.
//!
//! - **Tiled:** opaque. The corners outside the radius are painted the
//!   ground ([`GROUND`]), so the surface keeps its full opaque region
//!   (River scanout, `PERFORMANCE.md`).
//! - **Floating:** the corners are cleared (the swapchain is ARGB and the
//!   theme is [`crate::theme::overlay`]), the edges resize, the titlebar
//!   drags.
//! - **Bare:** content only, square (Cinema).
//!
//! The titlebar row, left to right: the close dot, the app's first menu
//! under its name in bold, its other menus, the Window menu, then the
//! window's title in tertiary ink, right-aligned. Menus come from
//! [`crate::menu::current`]; a choice goes through [`crate::menu::choose`].
//! A menu's dropdown hangs under its title inside the window. Spec:
//! `docs/specs/2026-10-21-next-tab-bar-design.md`, slice 6.

mod bar;
mod dropdown;
mod frame;
mod grips;
mod island;
mod paint;

use iced::window::Direction;
use iced::{Color, Element, Theme};

pub use frame::WindowFrame;
pub use island::island;

/// Titlebar height, logical px.
pub const HEIGHT: f32 = 38.0;

/// Corner radius of a window (and of the shell's bar).
pub const RADIUS: f32 = 12.0;

/// What shows between windows: the compositor's clear colour. Tiles paint
/// their corners with it so they can stay opaque.
pub const GROUND: Color = Color::BLACK;

/// How a window is shown, which decides its frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chrome {
    /// In a zone: opaque, ground-painted corners.
    Tiled,
    /// Over the tiles: transparent corners, resize edges, a drag handle.
    Floating,
    /// The whole output (Cinema): no titlebar, square.
    Bare,
}

impl From<bool> for Chrome {
    /// `true` is floating.
    fn from(floating: bool) -> Self {
        if floating {
            Chrome::Floating
        } else {
            Chrome::Tiled
        }
    }
}

/// Frame `content` with the titlebar and this app's published menus.
///
/// `on_drag` starts a move ([`crate::float::drag`]) and is only sent while
/// floating; `on_close` is the close dot; `on_resize` starts an edge resize
/// ([`crate::float::drag_resize`]) while floating.
pub fn window_frame<'a, Message>(
    chrome: impl Into<Chrome>,
    title: impl Into<String>,
    on_drag: Message,
    on_close: Message,
    on_resize: impl Fn(Direction) -> Message + 'a,
    content: Element<'a, Message, Theme>,
) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    WindowFrame::new(chrome, content)
        .title(title)
        .on_drag(on_drag)
        .on_close(on_close)
        .on_resize(on_resize)
        .into()
}
