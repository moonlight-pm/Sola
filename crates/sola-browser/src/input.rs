//! Shared input scaffolding for the browser shader / chrome.
//!
//! The engine keeps keymaps and native event constructors; this module
//! owns the cursor vocabulary and coordinate projection helpers.

use iced::{Point, Rectangle, mouse};

/// Cursor shape carried across the worker→iced boundary as a plain `u32`
/// (via `AtomicU32`). Discriminants are stable; new variants append.
#[repr(u32)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum CursorKind {
    #[default]
    Default = 0,
    Pointer = 1,
    Text = 2,
    Grab = 3,
    Grabbing = 4,
    Crosshair = 5,
    Move = 6,
    NotAllowed = 7,
    ResizingHorizontally = 8,
    ResizingVertically = 9,
    Working = 10,
}

impl CursorKind {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => CursorKind::Pointer,
            2 => CursorKind::Text,
            3 => CursorKind::Grab,
            4 => CursorKind::Grabbing,
            5 => CursorKind::Crosshair,
            6 => CursorKind::Move,
            7 => CursorKind::NotAllowed,
            8 => CursorKind::ResizingHorizontally,
            9 => CursorKind::ResizingVertically,
            10 => CursorKind::Working,
            _ => CursorKind::Default,
        }
    }

    pub fn to_iced(self) -> mouse::Interaction {
        match self {
            CursorKind::Default => mouse::Interaction::default(),
            CursorKind::Pointer => mouse::Interaction::Pointer,
            CursorKind::Text => mouse::Interaction::Text,
            CursorKind::Grab => mouse::Interaction::Grab,
            CursorKind::Grabbing => mouse::Interaction::Grabbing,
            CursorKind::Crosshair => mouse::Interaction::Crosshair,
            CursorKind::Move => mouse::Interaction::Move,
            CursorKind::NotAllowed => mouse::Interaction::NotAllowed,
            CursorKind::ResizingHorizontally => mouse::Interaction::ResizingHorizontally,
            CursorKind::ResizingVertically => mouse::Interaction::ResizingVertically,
            CursorKind::Working => mouse::Interaction::Wait,
        }
    }
}

/// Project a window-local cursor point into the webview's device-pixel
/// space (the size last sent via `Cmd::Resize`).
pub fn project_cursor_f64(point: Point, bounds: Rectangle, scale: f32) -> (f64, f64) {
    let x = ((point.x - bounds.x).max(0.0) * scale) as f64;
    let y = ((point.y - bounds.y).max(0.0) * scale) as f64;
    (x, y)
}

/// Same as [`project_cursor_f64`] but integer pixels.
pub fn project_cursor_i32(point: Point, bounds: Rectangle, scale: f32) -> (i32, i32) {
    let (x, y) = project_cursor_f64(point, bounds, scale);
    (x as i32, y as i32)
}

/// Derive the scale factor the shader last requested from widget bounds
/// width vs last physical size.
pub fn scale_from_last_size(bounds: Rectangle, last_req_w: u32, fallback: f32) -> f32 {
    if bounds.width > 0.0 {
        (last_req_w as f32 / bounds.width).max(0.5)
    } else {
        fallback.max(0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_roundtrip() {
        assert_eq!(CursorKind::from_u32(1), CursorKind::Pointer);
        assert_eq!(CursorKind::from_u32(99), CursorKind::Default);
    }
}
