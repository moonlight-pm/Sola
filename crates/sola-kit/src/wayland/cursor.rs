//! Cursor-shape-v1 integration. CEF emits a cursor-change event via
//! its DisplayHandler whenever CSS `cursor:` changes inside a kit
//! webview. We translate the CEF cursor type to a wp_cursor_shape_v1
//! `Shape` and the next Wayland dispatch tick applies it to the
//! active pointer via `wp_cursor_shape_device_v1.set_shape`.
//!
//! Known limitation: river+bundled-Adwaita currently renders only
//! `text` and `pointer` shapes; others silently fall back to default.
//! The pipeline here is verified correct — see
//! `docs/manual/cursor-theme-loading.md` before touching this file.
//!
//! The CEF UI thread and the Wayland event loop are the same thread
//! in our setup (see `cef/handlers.rs`), so a thread-local `Cell`
//! is the cheapest possible producer→consumer channel — no locks,
//! no allocations, single-frame coalescing falls out for free.
//!
//! Out of scope (per design):
//!   - XCursor fallback when wp_cursor_shape_v1 isn't advertised.
//!     We bind the manager optionally and warn once on init; if it's
//!     missing every cursor change is silently dropped (same effect
//!     as today).
//!   - Custom cursors (`cursor: url(...)`). Cursor-shape-v1 only
//!     handles named shapes; the CEF `CT_CUSTOM` path is ignored.

use std::cell::Cell;

use wayland_client::delegate_noop;
use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1},
    wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
};

use crate::wayland::WaylandClient;

thread_local! {
    /// Most recent cursor shape requested by CEF for which we have
    /// not yet issued a `set_shape`. A flurry of CSS hovers in the
    /// same frame collapses to a single wire request.
    static PENDING: Cell<Option<Shape>> = const { Cell::new(None) };
}

/// Producer side — called from the CEF DisplayHandler callback.
pub fn set_pending(shape: Shape) {
    tracing::debug!(?shape, "cursor: pending");
    PENDING.with(|p| p.set(Some(shape)));
}

/// Consumer side — called by the Wayland event loop on each tick.
pub fn take_pending() -> Option<Shape> {
    PENDING.with(|p| p.take())
}

/// Map CEF's `cef_cursor_type_t` to a `wp_cursor_shape_v1` shape.
/// Returns `None` for cursor types with no direct shape equivalent
/// (the no-cursor `CT_NONE`, the caller-supplied `CT_CUSTOM`, and
/// any future variant we haven't mapped yet).
pub fn cef_to_shape(t: cef::CursorType) -> Option<Shape> {
    use cef::sys::cef_cursor_type_t::*;
    match *t.as_ref() {
        CT_POINTER => Some(Shape::Default),
        CT_CROSS => Some(Shape::Crosshair),
        CT_HAND => Some(Shape::Pointer),
        CT_IBEAM => Some(Shape::Text),
        CT_VERTICALTEXT => Some(Shape::VerticalText),
        CT_WAIT => Some(Shape::Wait),
        CT_HELP => Some(Shape::Help),
        CT_PROGRESS => Some(Shape::Progress),
        CT_CELL => Some(Shape::Cell),
        CT_CONTEXTMENU => Some(Shape::ContextMenu),
        CT_ALIAS => Some(Shape::Alias),
        CT_COPY => Some(Shape::Copy),
        CT_MOVE => Some(Shape::Move),
        CT_NODROP => Some(Shape::NoDrop),
        CT_NOTALLOWED => Some(Shape::NotAllowed),
        CT_GRAB => Some(Shape::Grab),
        CT_GRABBING => Some(Shape::Grabbing),
        CT_ZOOMIN => Some(Shape::ZoomIn),
        CT_ZOOMOUT => Some(Shape::ZoomOut),
        CT_EASTRESIZE => Some(Shape::EResize),
        CT_NORTHRESIZE => Some(Shape::NResize),
        CT_NORTHEASTRESIZE => Some(Shape::NeResize),
        CT_NORTHWESTRESIZE => Some(Shape::NwResize),
        CT_SOUTHRESIZE => Some(Shape::SResize),
        CT_SOUTHEASTRESIZE => Some(Shape::SeResize),
        CT_SOUTHWESTRESIZE => Some(Shape::SwResize),
        CT_WESTRESIZE => Some(Shape::WResize),
        CT_NORTHSOUTHRESIZE => Some(Shape::NsResize),
        CT_EASTWESTRESIZE => Some(Shape::EwResize),
        CT_NORTHEASTSOUTHWESTRESIZE => Some(Shape::NeswResize),
        CT_NORTHWESTSOUTHEASTRESIZE => Some(Shape::NwseResize),
        CT_COLUMNRESIZE => Some(Shape::ColResize),
        CT_ROWRESIZE => Some(Shape::RowResize),
        // Panning cursors all collapse to all-scroll — Wayland's
        // cursor-shape vocabulary doesn't distinguish directions.
        CT_MIDDLEPANNING
        | CT_EASTPANNING
        | CT_NORTHPANNING
        | CT_NORTHEASTPANNING
        | CT_NORTHWESTPANNING
        | CT_SOUTHPANNING
        | CT_SOUTHEASTPANNING
        | CT_SOUTHWESTPANNING
        | CT_WESTPANNING
        | CT_MIDDLE_PANNING_VERTICAL
        | CT_MIDDLE_PANNING_HORIZONTAL => Some(Shape::AllScroll),
        // CT_NONE = hide cursor, CT_CUSTOM = bitmap supplied via
        // custom_cursor_info, CT_DND_* = drag-and-drop overlays.
        // None of these are supported per the design.
        _ => None,
    }
}

// Both wp_cursor_shape interfaces are request-only — the server
// never emits events on them — so the Dispatch impls are trivial
// no-ops. `delegate_noop!` handles that boilerplate.
delegate_noop!(WaylandClient: ignore WpCursorShapeManagerV1);
delegate_noop!(WaylandClient: ignore WpCursorShapeDeviceV1);
