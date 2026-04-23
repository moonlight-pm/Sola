//! Dispatch for `river_xkb_binding_v1`.

use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

use sola_bus::topics::{ChordEvent, Topic};

use crate::client::AppData;
use crate::protocol::river_xkb_bindings_v1::river_xkb_binding_v1::RiverXkbBindingV1;

impl Dispatch<RiverXkbBindingV1, (u32, u32)> for AppData {
    fn event(
        state: &mut Self,
        _: &RiverXkbBindingV1,
        event: <RiverXkbBindingV1 as Proxy>::Event,
        &(keysym, modifiers): &(u32, u32),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_xkb_bindings_v1::river_xkb_binding_v1::Event;
        match event {
            Event::Pressed => {
                tracing::debug!(keysym, modifiers, "chord pressed");
                state
                    .bus
                    .emit(Topic::Chord(ChordEvent { keysym, modifiers }));
            }
            Event::Released => {
                tracing::debug!(keysym, modifiers, "chord released");
                state
                    .bus
                    .emit(Topic::ChordReleased(ChordEvent { keysym, modifiers }));
            }
            _ => {}
        }
    }
}
