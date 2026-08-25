//! Interactive move/resize of floating windows.
//!
//! River drives this through `river_seat_v1.op_start_pointer`: the WM starts an
//! op during a manage sequence, receives `op_delta` events giving the total
//! cumulative pointer motion since the start, sets the window's position /
//! proposes its dimensions from those deltas, and ends the op with `op_end`
//! once `op_release` arrives. Move follows the pointer; resize drags the
//! grabbed edge or corner, pinning the opposite side(s).
//!
//! Only floating windows participate in move/resize, and only via CSD
//! (`pointer_move_requested` / `pointer_resize_requested` from a kit
//! titlebar). Super+left/right are **not** bound — they reach clients
//! (⌘-click in the browser).

use crate::client::AppData;
use crate::protocol::river_window_management_v1::river_window_v1::Edges;
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

/// Edge or corner an interactive resize is dragging. The opposite side(s)
/// stay pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeHandle {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

/// An in-flight interactive move/resize.
#[derive(Debug, Clone)]
pub struct OpState {
    pub kind: OpKind,
    pub window_id: u32,
    /// The window's rectangle at the moment the grab started. All deltas are
    /// applied against this (op_delta is cumulative-from-start).
    pub start: Rect,
    /// The grabbed edge/corner, for a resize. `None` for a move.
    pub handle: Option<ResizeHandle>,
    /// `op_start_pointer` has been issued (in a manage sequence).
    pub started: bool,
    /// `op_release` received; `op_end` is pending on the next manage sequence.
    pub released: bool,
}

/// Minimum width/height a resize can shrink a window to, so a drag can't
/// collapse it to nothing.
pub const MIN_DIM: i32 = 100;

/// Top of the usable float area = bottom of the shell menubar.
/// Keep in sync with `sola_shell::zoning::MENUBAR_HEIGHT` (28).
pub const MENUBAR_HEIGHT: i32 = 28;

// --- Pure geometry -------------------------------------------------------

/// New top-left position for a move: the start position shifted by the
/// cumulative pointer delta, clamped so the titlebar cannot cover the menubar.
pub fn moved(start: Rect, dx: i32, dy: i32) -> (i32, i32) {
    (start.x + dx, (start.y + dy).max(MENUBAR_HEIGHT))
}

/// Pick the corner nearest the grab point: which horizontal and vertical half
/// of the window the pointer sits in. Defaults to the bottom-right when the
/// pointer position is unknown. Used by Meta+RightDrag (corner-only).
pub fn pick_corner(start: Rect, pointer: Option<(i32, i32)>) -> ResizeHandle {
    let Some((px, py)) = pointer else {
        return ResizeHandle::SouthEast;
    };
    let left = px < start.x + start.w / 2;
    let top = py < start.y + start.h / 2;
    match (top, left) {
        (true, true) => ResizeHandle::NorthWest,
        (true, false) => ResizeHandle::NorthEast,
        (false, true) => ResizeHandle::SouthWest,
        (false, false) => ResizeHandle::SouthEast,
    }
}

/// New rectangle for a resize: the grabbed edge/corner moves by the cumulative
/// delta, the opposite side(s) stay pinned, each axis is clamped to `MIN_DIM`,
/// and the top edge cannot climb above the menubar.
pub fn resized(start: Rect, handle: ResizeHandle, dx: i32, dy: i32) -> Rect {
    let (move_left, move_right, move_top, move_bottom) = match handle {
        ResizeHandle::North => (false, false, true, false),
        ResizeHandle::South => (false, false, false, true),
        ResizeHandle::East => (false, true, false, false),
        ResizeHandle::West => (true, false, false, false),
        ResizeHandle::NorthEast => (false, true, true, false),
        ResizeHandle::NorthWest => (true, false, true, false),
        ResizeHandle::SouthEast => (false, true, false, true),
        ResizeHandle::SouthWest => (true, false, false, true),
    };

    let (x, w) = if move_left {
        let right = start.x + start.w; // pinned
        let nx = start.x + dx;
        let nw = right - nx;
        if nw < MIN_DIM {
            (right - MIN_DIM, MIN_DIM)
        } else {
            (nx, nw)
        }
    } else if move_right {
        (start.x, (start.w + dx).max(MIN_DIM))
    } else {
        (start.x, start.w)
    };

    let (y, h) = if move_top {
        let bottom = start.y + start.h; // pinned
        let ny = start.y + dy;
        let nh = bottom - ny;
        if nh < MIN_DIM {
            (bottom - MIN_DIM, MIN_DIM)
        } else {
            (ny, nh)
        }
    } else if move_bottom {
        (start.y, (start.h + dy).max(MIN_DIM))
    } else {
        (start.y, start.h)
    };

    // Keep the window below the menubar (move-up clamp for top-edge resize
    // and for windows that somehow started above the bar).
    let (y, h) = if y < MENUBAR_HEIGHT {
        let bottom = y + h;
        let y2 = MENUBAR_HEIGHT;
        let h2 = (bottom - y2).max(MIN_DIM);
        (y2, h2)
    } else {
        (y, h)
    };

    Rect { x, y, w, h }
}

// --- Lifecycle (folds into AppData) --------------------------------------

/// Map an xdg-shell resize `edges` bitfield to our resize handle.
/// The protocol guarantees `edges` never sets both top+bottom or both
/// left+right; a single edge maps to that edge alone.
pub fn edges_to_handle(edges: Edges) -> ResizeHandle {
    let top = edges.contains(Edges::Top);
    let bottom = edges.contains(Edges::Bottom);
    let left = edges.contains(Edges::Left);
    let right = edges.contains(Edges::Right);
    match (top, bottom, left, right) {
        (true, false, true, false) => ResizeHandle::NorthWest,
        (true, false, false, true) => ResizeHandle::NorthEast,
        (false, true, true, false) => ResizeHandle::SouthWest,
        (false, true, false, true) => ResizeHandle::SouthEast,
        (true, false, false, false) => ResizeHandle::North,
        (false, true, false, false) => ResizeHandle::South,
        (false, false, true, false) => ResizeHandle::West,
        (false, false, false, true) => ResizeHandle::East,
        // empty / unexpected → south-east default
        _ => ResizeHandle::SouthEast,
    }
}

/// Begin an interactive op on an explicit window. Used by the CSD-request
/// path (`pointer_move_requested` / `pointer_resize_requested`). Floating-gated.
///
/// `handle`: `Some(h)` uses that edge/corner (resize from requested edges);
/// `None` on a resize falls back to `pick_corner` from the pointer position;
/// ignored for a move.
pub fn begin_for(state: &mut AppData, kind: OpKind, window_id: u32, handle: Option<ResizeHandle>) {
    if state.op.is_some() {
        return;
    }
    if !state.floating.contains(&window_id) {
        tracing::debug!(
            window_id,
            ?kind,
            "interactive op ignored: window not floating"
        );
        return; // move/resize is floating-only
    }
    let Some(g) = state.registry.geometry(window_id) else {
        tracing::debug!(window_id, "interactive op ignored: geometry unknown");
        return;
    };
    let start = Rect {
        x: g.x,
        y: g.y,
        w: g.width,
        h: g.height,
    };
    let handle = match kind {
        OpKind::Resize => handle.or_else(|| Some(pick_corner(start, state.pointer_pos))),
        OpKind::Move => None,
    };
    tracing::info!(window_id, ?kind, ?handle, ?start, "begin interactive op");
    state.op = Some(OpState {
        kind,
        window_id,
        start,
        handle,
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
            let handle = op.handle.unwrap_or(ResizeHandle::SouthEast);
            let r = resized(op.start, handle, dx, dy);
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
    let (released, started, kind, handle, wid) = match state.op.as_ref() {
        Some(op) => (op.released, op.started, op.kind, op.handle, op.window_id),
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
        set_cursor(state, kind, handle);
        tracing::debug!(window_id = wid, ?kind, "op_start_pointer");
    }
}

/// Cursor-shape device for CSD move/resize feedback. Idempotent. Safe
/// inside a manage sequence (no pointer bindings — Super+click reaches clients).
pub fn ensure_op_cursor(state: &mut AppData) {
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

/// The cursor shape for an op: a move cursor for a move, or the resize cursor
/// matching the grabbed edge/corner.
fn shape_for(kind: OpKind, handle: Option<ResizeHandle>) -> Shape {
    match kind {
        OpKind::Move => Shape::Move,
        OpKind::Resize => match handle.unwrap_or(ResizeHandle::SouthEast) {
            ResizeHandle::North | ResizeHandle::South => Shape::NsResize,
            ResizeHandle::East | ResizeHandle::West => Shape::EwResize,
            ResizeHandle::NorthWest | ResizeHandle::SouthEast => Shape::NwseResize,
            ResizeHandle::NorthEast | ResizeHandle::SouthWest => Shape::NeswResize,
        },
    }
}

/// Show the move/resize cursor for the duration of the op. River ignores the
/// `set_shape` serial for the WM during an op (seat v4), so 0 is fine.
fn set_cursor(state: &mut AppData, kind: OpKind, handle: Option<ResizeHandle>) {
    if let Some(dev) = state.cursor_device.as_ref() {
        dev.set_shape(0, shape_for(kind, handle));
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
    fn move_clamps_y_to_menubar() {
        assert_eq!(
            moved(rect(100, 50, 800, 600), 0, -100),
            (100, MENUBAR_HEIGHT)
        );
        // Already at the floor — further up stays put.
        assert_eq!(
            moved(rect(0, MENUBAR_HEIGHT, 400, 300), 0, -50),
            (0, MENUBAR_HEIGHT)
        );
    }

    #[test]
    fn pick_corner_by_quadrant() {
        let r = rect(0, 0, 100, 100); // center (50, 50)
        assert_eq!(pick_corner(r, Some((10, 10))), ResizeHandle::NorthWest);
        assert_eq!(pick_corner(r, Some((90, 10))), ResizeHandle::NorthEast);
        assert_eq!(pick_corner(r, Some((10, 90))), ResizeHandle::SouthWest);
        assert_eq!(pick_corner(r, Some((90, 90))), ResizeHandle::SouthEast);
        // Unknown pointer position → bottom-right default.
        assert_eq!(pick_corner(r, None), ResizeHandle::SouthEast);
    }

    #[test]
    fn resize_bottom_right_grows_from_pinned_top_left() {
        let r = resized(rect(100, 100, 400, 300), ResizeHandle::SouthEast, 50, -20);
        assert_eq!(r, rect(100, 100, 450, 280));
    }

    #[test]
    fn resize_top_left_moves_and_pins_bottom_right() {
        // start (100,100,400,300): bottom-right pinned at (500,400).
        let r = resized(rect(100, 100, 400, 300), ResizeHandle::NorthWest, 30, 40);
        assert_eq!(r, rect(130, 140, 370, 260));
        assert_eq!((r.x + r.w, r.y + r.h), (500, 400));
    }

    #[test]
    fn resize_clamps_to_min_and_keeps_pinned_corner() {
        // Drag the top-left far past the minimum: the bottom-right pin holds.
        let r = resized(
            rect(100, 100, 400, 300),
            ResizeHandle::NorthWest,
            1000,
            1000,
        );
        assert_eq!((r.w, r.h), (MIN_DIM, MIN_DIM));
        assert_eq!((r.x + r.w, r.y + r.h), (500, 400));
    }

    #[test]
    fn resize_north_edge_only_moves_top() {
        let r = resized(rect(100, 100, 400, 300), ResizeHandle::North, 999, 40);
        // dx ignored; top moves down by 40, width unchanged.
        assert_eq!(r, rect(100, 140, 400, 260));
    }

    #[test]
    fn resize_east_edge_only_moves_right() {
        let r = resized(rect(100, 100, 400, 300), ResizeHandle::East, 50, 999);
        assert_eq!(r, rect(100, 100, 450, 300));
    }

    #[test]
    fn resize_top_clamps_to_menubar() {
        // Drag top edge up past y=0 → clamp at menubar, bottom stays pinned.
        let r = resized(rect(100, 50, 400, 300), ResizeHandle::North, 0, -100);
        assert_eq!(r.y, MENUBAR_HEIGHT);
        assert_eq!(r.y + r.h, 350); // bottom pinned at 50+300
    }

    #[test]
    fn edges_map_to_handles() {
        assert_eq!(
            edges_to_handle(Edges::Top | Edges::Left),
            ResizeHandle::NorthWest
        );
        assert_eq!(
            edges_to_handle(Edges::Top | Edges::Right),
            ResizeHandle::NorthEast
        );
        assert_eq!(
            edges_to_handle(Edges::Bottom | Edges::Left),
            ResizeHandle::SouthWest
        );
        assert_eq!(
            edges_to_handle(Edges::Bottom | Edges::Right),
            ResizeHandle::SouthEast
        );
        // single-edge requests keep a pure edge handle
        assert_eq!(edges_to_handle(Edges::Top), ResizeHandle::North);
        assert_eq!(edges_to_handle(Edges::Bottom), ResizeHandle::South);
        assert_eq!(edges_to_handle(Edges::Left), ResizeHandle::West);
        assert_eq!(edges_to_handle(Edges::Right), ResizeHandle::East);
        assert_eq!(edges_to_handle(Edges::empty()), ResizeHandle::SouthEast);
    }
}
