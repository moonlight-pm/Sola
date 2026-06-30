//! Interactive move/resize of floating windows.
//!
//! River drives this through `river_seat_v1.op_start_pointer`: the WM starts an
//! op during a manage sequence, receives `op_delta` events giving the total
//! cumulative pointer motion since the start, sets the window's position /
//! proposes its dimensions from those deltas, and ends the op with `op_end`
//! once `op_release` arrives. Move follows the pointer; resize drags the corner
//! nearest where the grab started, pinning the opposite corner.
//!
//! Only floating windows participate — `on_pressed` ignores a press over a
//! non-floating window (or empty space). The geometry math
//! (`moved`/`pick_corner`/`resized`) is pure and unit-tested; the lifecycle
//! helpers fold state into `AppData` and are exercised by the build + manual
//! smoke, like the rest of the wayland wiring.

use crate::client::AppData;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;

/// Which interactive operation a pointer binding triggers. Doubles as the
/// pointer binding's user-data, so `pressed`/`released` know which fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Move,
    Resize,
}

/// A rectangle in compositor logical coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Which corner an interactive resize is dragging. The opposite corner is
/// pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// An in-flight interactive move/resize.
#[derive(Debug, Clone)]
pub struct OpState {
    pub kind: OpKind,
    pub window_id: u32,
    /// The window's rectangle at the moment the grab started. All deltas are
    /// applied against this (op_delta is cumulative-from-start).
    pub start: Rect,
    /// The grabbed corner, for a resize. `None` for a move.
    pub corner: Option<Corner>,
    /// `op_start_pointer` has been issued (in a manage sequence).
    pub started: bool,
    /// `op_release` received; `op_end` is pending on the next manage sequence.
    pub released: bool,
}

/// Minimum width/height a resize can shrink a window to, so a drag can't
/// collapse it to nothing.
pub const MIN_DIM: i32 = 100;

// --- Pure geometry -------------------------------------------------------

/// New top-left position for a move: the start position shifted by the
/// cumulative pointer delta.
pub fn moved(start: Rect, dx: i32, dy: i32) -> (i32, i32) {
    (start.x + dx, start.y + dy)
}

/// Pick the corner nearest the grab point: which horizontal and vertical half
/// of the window the pointer sits in. Defaults to the bottom-right when the
/// pointer position is unknown.
pub fn pick_corner(start: Rect, pointer: Option<(i32, i32)>) -> Corner {
    let Some((px, py)) = pointer else {
        return Corner::BottomRight;
    };
    let left = px < start.x + start.w / 2;
    let top = py < start.y + start.h / 2;
    match (top, left) {
        (true, true) => Corner::TopLeft,
        (true, false) => Corner::TopRight,
        (false, true) => Corner::BottomLeft,
        (false, false) => Corner::BottomRight,
    }
}

/// New rectangle for a resize: the grabbed corner moves by the cumulative
/// delta, the opposite corner stays pinned, and each axis is clamped to
/// `MIN_DIM` (clamping freezes the pinned edge so it doesn't drift).
pub fn resized(start: Rect, corner: Corner, dx: i32, dy: i32) -> Rect {
    let (left, top) = match corner {
        Corner::TopLeft => (true, true),
        Corner::TopRight => (false, true),
        Corner::BottomLeft => (true, false),
        Corner::BottomRight => (false, false),
    };
    let (x, w) = if left {
        let right = start.x + start.w; // pinned
        let nx = start.x + dx;
        let nw = right - nx;
        if nw < MIN_DIM {
            (right - MIN_DIM, MIN_DIM)
        } else {
            (nx, nw)
        }
    } else {
        (start.x, (start.w + dx).max(MIN_DIM))
    };
    let (y, h) = if top {
        let bottom = start.y + start.h; // pinned
        let ny = start.y + dy;
        let nh = bottom - ny;
        if nh < MIN_DIM {
            (bottom - MIN_DIM, MIN_DIM)
        } else {
            (ny, nh)
        }
    } else {
        (start.y, (start.h + dy).max(MIN_DIM))
    };
    Rect { x, y, w, h }
}

// --- Lifecycle (folds into AppData) --------------------------------------

pub fn on_pressed(state: &mut AppData, kind: OpKind) {
    // A bound press over a non-floating window (or empty space) is a normal,
    // frequent gesture — Meta+click is a reserved WM gesture that we simply
    // swallow — so the gate logging is debug, not info. The op lifecycle itself
    // (`begin interactive op`) is the info-level signal.
    tracing::debug!(
        ?kind,
        pointer_window = ?state.pointer_window,
        floating = ?state.floating,
        op_active = state.op.is_some(),
        "Meta-drag pointer binding pressed"
    );
    if state.op.is_some() {
        return;
    }
    let Some(wid) = state.pointer_window else {
        tracing::debug!("Meta-drag ignored: no window under pointer");
        return;
    };
    if !state.floating.contains(&wid) {
        tracing::debug!(window_id = wid, "Meta-drag ignored: window not floating");
        return; // move/resize is floating-only
    }
    let Some(g) = state.registry.geometry(wid) else {
        tracing::debug!(window_id = wid, "Meta-drag ignored: geometry unknown");
        return;
    };
    let start = Rect {
        x: g.x,
        y: g.y,
        w: g.width,
        h: g.height,
    };
    let corner = match kind {
        OpKind::Resize => Some(pick_corner(start, state.pointer_pos)),
        OpKind::Move => None,
    };
    tracing::info!(window_id = wid, ?kind, ?corner, ?start, "begin interactive op");
    state.op = Some(OpState {
        kind,
        window_id: wid,
        start,
        corner,
        started: false,
        released: false,
    });
}

/// The bound button (or all op input) was released. Mark the op so the next
/// manage sequence ends it. No-op when no op is running.
pub fn on_released(state: &mut AppData) {
    if let Some(op) = state.op.as_mut() {
        op.released = true;
    }
}

/// A cumulative pointer delta arrived. Update the window live: a move sets its
/// position; a resize proposes its new rectangle.
pub fn on_delta(state: &mut AppData, dx: i32, dy: i32) {
    let Some(op) = state.op.clone() else {
        return;
    };
    if op.released {
        return;
    }
    let wid = op.window_id;
    match op.kind {
        OpKind::Move => {
            let (x, y) = moved(op.start, dx, dy);
            state.pending.render_positions.insert(wid, (x, y));
            state.pending.render_dirty = true;
        }
        OpKind::Resize => {
            let corner = op.corner.unwrap_or(Corner::BottomRight);
            let r = resized(op.start, corner, dx, dy);
            // frame() sets both the proposed size and the position (the latter
            // moves when a left/top edge is dragged).
            state.pending.frame(wid, r.x, r.y, r.w, r.h);
        }
    }
}

/// Issue the pending op request for this manage sequence: start a freshly-armed
/// op, or end one whose input has been released. Must be called inside a manage
/// sequence (before `manage_finish`), like every other op/seat request.
pub fn drive(state: &mut AppData) {
    let Some(seat) = state.seat.clone() else {
        return;
    };
    let (released, started, kind, corner, wid) = match state.op.as_ref() {
        Some(op) => (op.released, op.started, op.kind, op.corner, op.window_id),
        None => return,
    };
    if released {
        seat.op_end();
        // Clear `op` BEFORE emitting so the gate in `emit_geometry` lets this
        // one through — a single final WindowGeometry for the whole drag.
        state.op = None;
        clear_cursor(state);
        crate::translator::emit_geometry(state, wid);
        tracing::info!(window_id = wid, "ended interactive op");
    } else if !started {
        if let Some(op) = state.op.as_mut() {
            op.started = true;
        }
        seat.op_start_pointer();
        set_cursor(state, kind, corner);
        tracing::debug!(window_id = wid, ?kind, "op_start_pointer");
    }
}

/// Create and enable the move/resize pointer bindings once the seat is ready.
/// Idempotent. `enable` is a manage-sequence request, so this must be called
/// from within a manage sequence.
pub fn ensure_pointer_bindings(state: &mut AppData) {
    use crate::protocol::river_window_management_v1::river_seat_v1::Modifiers;
    // Linux input-event-codes.
    const BTN_LEFT: u32 = 0x110;
    const BTN_RIGHT: u32 = 0x111;
    let Some(seat) = state.seat.clone() else {
        return;
    };
    let Some(qh) = state.qh.clone() else {
        return;
    };
    if state.move_binding.is_none() {
        let b = seat.get_pointer_binding(BTN_LEFT, Modifiers::Mod4, &qh, OpKind::Move);
        b.enable();
        state.move_binding = Some(b);
        tracing::info!("enabled Meta+LeftDrag move binding");
    }
    if state.resize_binding.is_none() {
        let b = seat.get_pointer_binding(BTN_RIGHT, Modifiers::Mod4, &qh, OpKind::Resize);
        b.enable();
        state.resize_binding = Some(b);
        tracing::info!("enabled Meta+RightDrag resize binding");
    }
    ensure_cursor_device(state);
}

/// Create the cursor-shape device for the seat's pointer once both the seat and
/// the cursor-shape manager have been advertised. Idempotent. During an op
/// river uses the WM's pointer cursor (no client holds focus), so this device's
/// `set_shape` drives the move/resize cursor.
fn ensure_cursor_device(state: &mut AppData) {
    if state.cursor_device.is_some() {
        return;
    }
    let (Some(seat), Some(mgr), Some(qh)) = (
        state.wl_seat.clone(),
        state.cursor_shape_manager.clone(),
        state.qh.clone(),
    ) else {
        return;
    };
    let pointer = seat.get_pointer(&qh, ());
    let device = mgr.get_pointer(&pointer, &qh, ());
    state.wl_pointer = Some(pointer);
    state.cursor_device = Some(device);
    tracing::info!("created cursor-shape device for move/resize feedback");
}

/// The cursor shape for an op: a move cursor for a move, or the diagonal resize
/// cursor matching the grabbed corner.
fn shape_for(kind: OpKind, corner: Option<Corner>) -> Shape {
    match kind {
        OpKind::Move => Shape::Move,
        OpKind::Resize => match corner.unwrap_or(Corner::BottomRight) {
            Corner::TopLeft | Corner::BottomRight => Shape::NwseResize,
            Corner::TopRight | Corner::BottomLeft => Shape::NeswResize,
        },
    }
}

/// Show the move/resize cursor for the duration of the op. River ignores the
/// `set_shape` serial for the WM during an op (seat v4), so 0 is fine.
fn set_cursor(state: &mut AppData, kind: OpKind, corner: Option<Corner>) {
    if let Some(dev) = state.cursor_device.as_ref() {
        dev.set_shape(0, shape_for(kind, corner));
    }
}

/// Restore the default cursor when the op ends.
fn clear_cursor(state: &mut AppData) {
    if let Some(dev) = state.cursor_device.as_ref() {
        dev.set_shape(0, Shape::Default);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn move_offsets_position() {
        assert_eq!(moved(rect(100, 200, 800, 600), 25, -10), (125, 190));
    }

    #[test]
    fn pick_corner_by_quadrant() {
        let r = rect(0, 0, 100, 100); // center (50, 50)
        assert_eq!(pick_corner(r, Some((10, 10))), Corner::TopLeft);
        assert_eq!(pick_corner(r, Some((90, 10))), Corner::TopRight);
        assert_eq!(pick_corner(r, Some((10, 90))), Corner::BottomLeft);
        assert_eq!(pick_corner(r, Some((90, 90))), Corner::BottomRight);
        // Unknown pointer position → bottom-right default.
        assert_eq!(pick_corner(r, None), Corner::BottomRight);
    }

    #[test]
    fn resize_bottom_right_grows_from_pinned_top_left() {
        let r = resized(rect(100, 100, 400, 300), Corner::BottomRight, 50, -20);
        assert_eq!(r, rect(100, 100, 450, 280));
    }

    #[test]
    fn resize_top_left_moves_and_pins_bottom_right() {
        // start (100,100,400,300): bottom-right pinned at (500,400).
        let r = resized(rect(100, 100, 400, 300), Corner::TopLeft, 30, 40);
        assert_eq!(r, rect(130, 140, 370, 260));
        assert_eq!((r.x + r.w, r.y + r.h), (500, 400));
    }

    #[test]
    fn resize_clamps_to_min_and_keeps_pinned_corner() {
        // Drag the top-left far past the minimum: the bottom-right pin holds.
        let r = resized(rect(100, 100, 400, 300), Corner::TopLeft, 1000, 1000);
        assert_eq!((r.w, r.h), (MIN_DIM, MIN_DIM));
        assert_eq!((r.x + r.w, r.y + r.h), (500, 400));
    }
}
