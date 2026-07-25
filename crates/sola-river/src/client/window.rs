//! Dispatch for `river_window_manager_v1` and `river_window_v1`.

use tracing::{error, info, warn};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};

use sola_bus::topics::{Topic, WindowGeometry};

use crate::client::{op, AppData};
use crate::protocol::river_window_management_v1::{
    river_output_v1::RiverOutputV1, river_seat_v1::RiverSeatV1,
    river_window_manager_v1::RiverWindowManagerV1, river_window_v1::RiverWindowV1,
};

// Opcodes of the events on `river_window_manager_v1` that create new
// child objects. Order matches the XML event declarations.
const EVT_WINDOW_OPCODE: u16 = 6;
const EVT_OUTPUT_OPCODE: u16 = 7;
const EVT_SEAT_OPCODE: u16 = 8;

impl Dispatch<RiverWindowManagerV1, ()> for AppData {
    fn event(
        state: &mut Self,
        _wm: &RiverWindowManagerV1,
        event: <RiverWindowManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_window_management_v1::river_window_manager_v1::Event;
        match event {
            Event::Window { id: window } => {
                let wid = state.registry.mint();
                state.windows_by_object.insert(window.id(), wid);
                let node = window.get_node(qh, ());
                state.nodes_by_window.insert(wid, node);
                state.windows_by_id.insert(wid, window);
                // Per `river-window-management-v1`, a window is not displayed
                // until `propose_dimensions` (or `fullscreen`) runs in a manage
                // sequence and the matching render sequence finishes. Seed
                // `(0, 0)` so the client self-sizes; an inbound `Frame` topic
                // for zoned or sola-* default-framed windows will overwrite
                // this with real dimensions before the next manage_start.
                state.pending.manage.entry(wid).or_insert((0, 0));
                state.pending.manage_dirty = true;
                info!(window_id = wid, "new river window");
            }
            Event::ManageStart => {
                crate::client::manage::handle_manage_start(state);
            }
            Event::RenderStart => {
                crate::client::manage::handle_render_start(state);
            }
            Event::Seat { id: seat } => {
                if state.seat.is_none() {
                    // River's compositor-rendered cursor theme (resize
                    // cursors, wp_cursor_shape_v1 shapes, etc.) is NOT
                    // read from XCURSOR_THEME on river itself — that
                    // env var only influences cursors that child
                    // processes load themselves. The theme used for
                    // every compositor-side draw must be set per-seat
                    // via river_seat_v1::set_xcursor_theme (since v2).
                    // Without this call the cursor sticks at river's
                    // internal default no matter what the env says.
                    let theme = std::env::var("XCURSOR_THEME")
                        .unwrap_or_else(|_| "McMojave".to_string());
                    let size: u32 = std::env::var("XCURSOR_SIZE")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(24);
                    seat.set_xcursor_theme(theme.clone(), size);
                    info!(%theme, size, "set river xcursor theme");
                    state.seat = Some(seat);
                }
            }
            Event::Output { id: output } => {
                // Retain the proxy so its Dispatch impl can surface
                // Dimensions events (forwarded as OutputGeometry on the
                // bus). Dropping it would close the per-output channel.
                state.outputs.push(output);
            }
            Event::Unavailable => {
                error!("river_window_manager_v1 unavailable");
            }
            Event::Finished => {
                warn!("river_window_manager_v1 finished");
            }
            _ => {}
        }
    }

    event_created_child!(AppData, RiverWindowManagerV1, [
        EVT_WINDOW_OPCODE => (RiverWindowV1, ()),
        EVT_OUTPUT_OPCODE => (RiverOutputV1, ()),
        EVT_SEAT_OPCODE   => (RiverSeatV1, ()),
    ]);
}

impl Dispatch<RiverWindowV1, ()> for AppData {
    fn event(
        state: &mut Self,
        window: &RiverWindowV1,
        event: <RiverWindowV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_window_management_v1::river_window_v1::Event;
        let Some(&window_id) = state.windows_by_object.get(&window.id()) else {
            warn!(object = ?window.id(), "event for unknown window object");
            return;
        };
        let mut apps_dirty = false;
        match event {
            Event::AppId { app_id } => {
                let value = app_id.unwrap_or_default();
                info!(window_id, app_id = %value, "app_id set");
                state.registry.set_app_id(window_id, value);
                apps_dirty = true;
            }
            Event::Title { title } => {
                state
                    .registry
                    .set_title(window_id, title.unwrap_or_default());
                apps_dirty = true;
            }
            Event::Closed => {
                info!(window_id, "window closed");
                // River asserts in Window.destroy() that no seat is still
                // focused on the window. The closed event is followed by a
                // manage_start, so queue clear_focus now and it'll be sent
                // before river internally tears the Window down.
                if state.focused_window == Some(window_id) {
                    state.pending.set_focus(crate::pending::FocusAction::None);
                    state.focused_window = None;
                }
                state.registry.remove(window_id);
                state.windows_by_object.retain(|_, v| *v != window_id);
                state.windows_by_id.remove(&window_id);
                state.nodes_by_window.remove(&window_id);
                state.placed.remove(&window_id);
                state.currently_fullscreen.remove(&window_id);
                state.first_dimensions.remove(&window_id);
                state.deferred_size.remove(&window_id);
                state.last_proposed.remove(&window_id);
                state.last_position.remove(&window_id);
                state.floating.remove(&window_id);
                if state.pointer_window == Some(window_id) {
                    state.pointer_window = None;
                }
                // Drop the window's sticky geometry so a late subscriber can't
                // resurrect a closed window's rectangle. Retract keys on
                // window_id; the other fields are ignored.
                let _ = state.bus.retract(Topic::WindowGeometry(WindowGeometry {
                    window_id,
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                }));
                window.destroy();
                apps_dirty = true;
            }
            Event::DimensionsHint {
                max_width,
                max_height,
                ..
            } => {
                let app_id = state.registry.app_id_for(window_id).unwrap_or("?");
                info!(
                    window_id,
                    app_id,
                    max_width,
                    max_height,
                    "dimensions hint"
                );
                state
                    .registry
                    .set_max_size(window_id, max_width, max_height);
            }
            Event::UnreliablePid { unreliable_pid } => {
                if unreliable_pid > 0 {
                    state.registry.set_pid(window_id, unreliable_pid as u32);
                    apps_dirty = true;
                }
            }
            Event::FullscreenRequested { output: _ } => {
                let app_id = state.registry.app_id_for(window_id).unwrap_or("?");
                info!(window_id, app_id, "fullscreen requested (client-initiated)");
                state.pending.queue_fullscreen(window_id);
            }
            Event::ExitFullscreenRequested => {
                let app_id = state.registry.app_id_for(window_id).unwrap_or("?");
                info!(window_id, app_id, "exit fullscreen requested (client-initiated)");
                state.pending.queue_exit_fullscreen(window_id);
            }
            Event::Dimensions { width, height } => {
                let newly_initialized = !state.first_dimensions.contains(&window_id);
                if let Some((w, h)) = crate::client::manage::note_dimensions(
                    &mut state.first_dimensions,
                    &mut state.deferred_size,
                    window_id,
                ) {
                    // Surface is initialized now — apply the size we held back
                    // as a normal runtime resize. The next bus_tick (≤20ms)
                    // turns manage_dirty into a manage cycle that proposes it.
                    state.pending.manage.insert(window_id, (w, h));
                    state.pending.manage_dirty = true;
                }
                if state.registry.set_size(window_id, width, height) {
                    crate::translator::emit_geometry(state, window_id);
                }
                tracing::debug!(window_id, width, height, newly_initialized, "window dimensions");
            }
            Event::PointerMoveRequested { .. } => {
                // Client-side-decoration move (e.g. a kit titlebar drag → xdg_toplevel.move).
                // Reuse D1's move op; begin_for gates on `floating` and is a no-op for
                // tiled windows (Meta+drag still moves those). op_start_pointer is issued
                // on the manage_start that follows this event.
                op::begin_for(state, op::OpKind::Move, window_id, None);
            }
            Event::PointerResizeRequested { edges, .. } => {
                // CSD resize (edge/corner drag). `edges` is a bitfield enum arg
                // (`WEnum<Edges>`); resolve it, defaulting an unknown value to a
                // pointer-position-derived corner (None).
                let handle = edges.into_result().ok().map(op::edges_to_handle);
                op::begin_for(state, op::OpKind::Resize, window_id, handle);
            }
            _ => {}
        }
        if apps_dirty {
            crate::translator::emit_windows(state);
        }
    }
}
