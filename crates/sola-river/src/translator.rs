//! Bus ↔ River translation helpers.
//!
//! Everything here operates on `AppData` directly — this is the only file
//! in the crate that imports both the bus and wayland sides.
use sola_bus::topics::Topic;
use tracing::debug;
use wayland_client::{Proxy, QueueHandle};

use crate::client::AppData;
use crate::registry::chord_diff;

pub fn emit_windows(state: &mut AppData) {
    let windows = state.registry.as_windows();
    debug!(count = windows.len(), "emitting Windows");
    state.bus.emit_sticky(Topic::Windows(windows));
}

/// Apply a chord-set update from `pending.chords`. MUST be called
/// during a manage sequence: `xkb_binding_v1.enable` and `disable` are
/// both manage-sequence requests per River's protocol.
pub fn apply_pending_chords(state: &mut AppData, new_pairs: Vec<(u32, u32)>) {
    let Some(qh) = state.qh.clone() else { return };
    let Some(xb) = state.xkb_bindings.clone() else {
        return;
    };
    let Some(river_seat) = state.seat.clone() else {
        return;
    };

    let old_pairs: Vec<(u32, u32)> = state.chords.by_chord.keys().copied().collect();
    let (added, removed) = chord_diff(&old_pairs, &new_pairs);

    for pair in removed {
        if let Some(b) = state.chords.by_chord.remove(&pair) {
            state.chords.by_object.retain(|_, v| *v != pair);
            b.disable();
            b.destroy();
        }
    }

    for (keysym, modifiers) in added {
        let binding = bind_chord(&xb, &river_seat, keysym, modifiers, &qh);
        binding.enable();
        state
            .chords
            .by_object
            .insert(binding.id(), (keysym, modifiers));
        state.chords.by_chord.insert((keysym, modifiers), binding);
    }
}

fn bind_chord(
    xb: &crate::protocol::river_xkb_bindings_v1::river_xkb_bindings_v1::RiverXkbBindingsV1,
    seat: &crate::protocol::river_window_management_v1::river_seat_v1::RiverSeatV1,
    keysym: u32,
    modifiers: u32,
    qh: &QueueHandle<AppData>,
) -> crate::protocol::river_xkb_bindings_v1::river_xkb_binding_v1::RiverXkbBindingV1 {
    use crate::protocol::river_window_management_v1::river_seat_v1::Modifiers;
    let m = Modifiers::from_bits_retain(modifiers);
    xb.get_xkb_binding(seat, keysym, m, qh, (keysym, modifiers))
}
