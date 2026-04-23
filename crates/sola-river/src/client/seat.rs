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
                    debug!(window_id = id, "pointer_enter");
                    state
                        .bus
                        .emit(Topic::MouseEntered(MouseEnteredPayload { window_id: id }));
                }
            }
            Event::PointerLeave => {
                debug!("pointer_leave");
                state.bus.emit(Topic::MouseLeft);
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
