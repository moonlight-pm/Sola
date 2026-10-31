//! Dispatch for `river_seat_v1` — pointer enter/leave and window interaction.

use tracing::debug;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

use sola_bus::topics::{MouseClickedPayload, MouseEnteredPayload, Topic};

use crate::client::AppData;
use crate::protocol::river_window_management_v1::river_seat_v1::RiverSeatV1;

impl Dispatch<RiverSeatV1, ()> for AppData {
    fn event(
        state: &mut Self,
        _seat: &RiverSeatV1,
        event: <RiverSeatV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_window_management_v1::river_seat_v1::Event;
        match event {
            Event::PointerEnter { window } => {
                if let Some(&id) = state.windows_by_object.get(&window.id()) {
                    let still = !state.pointer_moved;
                    debug!(window_id = id, still, "pointer_enter");
                    // Hovered window (CSD resize corner fallback).
                    state.pointer_window = Some(id);
                    state.rehover = still;
                    state.bus.emit(Topic::MouseEntered(MouseEnteredPayload {
                        window_id: id,
                        still,
                    }));
                }
            }
            Event::PointerLeave => {
                debug!("pointer_leave");
                state.pointer_window = None;
                state.bus.emit(Topic::MouseLeft);
            }
            Event::PointerPosition { x, y } => {
                // Latest pointer position; used to pick the grabbed corner when
                // an interactive resize starts.
                state.pointer_pos = Some((x, y));
                state.pointer_moved = true;
                rehover(state);
            }
            Event::OpDelta { dx, dy } => {
                // Total cumulative motion since the op started.
                crate::client::op::on_delta(state, dx, dy);
            }
            Event::OpRelease => {
                crate::client::op::on_released(state);
            }
            Event::WindowInteraction { window } => {
                if let Some(&id) = state.windows_by_object.get(&window.id()) {
                    debug!(window_id = id, "window_interaction");
                    state
                        .bus
                        .emit(Topic::MouseClicked(MouseClickedPayload { window_id: id }));
                }
            }
            _ => {}
        }
    }
}

/// The pointer moved: if focus left the window under it without the
/// pointer (a still enter, a keyboard switch), announce that window again
/// so focus can follow the pointer from here.
fn rehover(state: &mut AppData) {
    if !std::mem::take(&mut state.rehover) {
        return;
    }
    let Some(id) = state.pointer_window.filter(|&id| state.focused_window != Some(id)) else {
        return;
    };
    debug!(window_id = id, "pointer moved — re-announce hovered window");
    state.bus.emit(Topic::MouseEntered(MouseEnteredPayload {
        window_id: id,
        still: false,
    }));
}
