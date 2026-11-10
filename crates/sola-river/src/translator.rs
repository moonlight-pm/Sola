//! Bus ↔ River translation helpers.
//!
//! Everything here operates on `AppData` directly — this is the only file
//! in the crate that imports both the bus and wayland sides.
use std::collections::HashMap;

use sola_bus::topics::{Topic, WindowFacts};
use tracing::debug;
use wayland_client::{Proxy, QueueHandle};

use crate::client::AppData;
use crate::registry::chord_diff;

pub fn emit_windows(state: &mut AppData) {
    // Until River's first manage sequence the windows it already had are
    // still arriving, one at a time: a list now would tell the shell that
    // the rest had closed (a restarted sola-river), and a list per window
    // floods it. The first sequence tells them all at once.
    if !state.windows_known {
        return;
    }
    // A window River has not named yet is not told: it may take back the
    // id it had ([`crate::window_ids`]).
    let mut windows = state.registry.as_windows();
    windows.retain(|w| !state.unidentified.contains(&w.window_id));
    debug!(count = windows.len(), "emitting Windows");
    state.bus.emit(Topic::Windows(windows));
    emit_window_tabs(state);
}

/// Tell the shell, once each, which tab a told window is (in its
/// `WindowFacts`); `crate::client::window::drop_window` retracts the record.
fn emit_window_tabs(state: &mut AppData) {
    let new: Vec<(u32, String)> = state
        .registry
        .tabs()
        .filter(|(id, _)| !state.unidentified.contains(id) && !state.tabs_told.contains(id))
        .map(|(id, tab)| (id, tab.to_string()))
        .collect();
    for (window_id, tab) in new {
        state.tabs_told.insert(window_id);
        tell_facts(state, window_id, |f| f.tab = Some(tab));
    }
}

/// Change one thing in a window's facts; when that changes them, send the
/// whole record. A window with no facts yet is sent nothing.
pub fn tell_facts(state: &mut AppData, window_id: u32, change: impl FnOnce(&mut WindowFacts)) {
    if let Some(facts) = changed_facts(&mut state.facts, window_id, change) {
        state.bus.emit(Topic::WindowFacts(facts));
    }
}

/// Apply `change` to `window_id`'s facts in `known`; the record to send, if
/// it differs from the one last sent (and is not empty before it was first).
fn changed_facts(
    known: &mut HashMap<u32, WindowFacts>,
    window_id: u32,
    change: impl FnOnce(&mut WindowFacts),
) -> Option<WindowFacts> {
    let mut facts = known.get(&window_id).cloned().unwrap_or(WindowFacts {
        window_id,
        rect: None,
        tab: None,
        parent: None,
    });
    change(&mut facts);
    if known.get(&window_id) == Some(&facts) {
        return None;
    }
    if !known.contains_key(&window_id)
        && facts.rect.is_none()
        && facts.tab.is_none()
        && facts.parent.is_none()
    {
        return None;
    }
    known.insert(window_id, facts.clone());
    Some(facts)
}

/// Retract a closed window's facts, once, if any were sent.
pub fn retract_facts(state: &mut AppData, window_id: u32) {
    if let Some(facts) = state.facts.remove(&window_id) {
        let _ = state.bus.retract(Topic::WindowFacts(facts));
    }
}

/// Tell the shell the window's current rectangle (in its `WindowFacts`).
/// No-op until both size and position are known. Callers gate on the registry
/// setter returning `true` (changed) so this fires only on a real move/resize.
pub fn emit_geometry(state: &mut AppData, window_id: u32) {
    // During an interactive move/resize we drive River live but stay quiet on
    // the bus, then emit once when the op ends (op::drive clears `op` first).
    // This keeps the shell from persisting FloatGeometry on every drag frame.
    if state.op.is_some() {
        return;
    }
    let Some(g) = state.registry.geometry(window_id) else {
        return;
    };
    debug!(
        window_id,
        x = g.x,
        y = g.y,
        width = g.width,
        height = g.height,
        "emitting WindowFacts rect"
    );
    tell_facts(state, window_id, |f| f.rect = Some(g));
}

/// Apply a chord-set update from `pending.chords`. MUST be called
/// during a manage sequence: `xkb_binding_v1.enable` and `disable` are
/// both manage-sequence requests per River's protocol.
pub fn apply_pending_chords(state: &mut AppData, new_pairs: Vec<(u32, u32)>) {
    let Some(qh) = state.qh.clone() else {
        tracing::warn!(n = new_pairs.len(), "drop RegisteredChords: no qh");
        return;
    };
    let Some(xb) = state.xkb_bindings.clone() else {
        tracing::warn!(
            n = new_pairs.len(),
            "drop RegisteredChords: no xkb_bindings"
        );
        return;
    };
    let Some(river_seat) = state.seat.clone() else {
        tracing::warn!(n = new_pairs.len(), "drop RegisteredChords: no seat");
        return;
    };

    let old_pairs: Vec<(u32, u32)> = state.chords.by_chord.keys().copied().collect();
    let (added, removed) = chord_diff(&old_pairs, &new_pairs);
    let added_n = added.len();
    let removed_n = removed.len();

    for pair in &removed {
        if let Some(b) = state.chords.by_chord.remove(pair) {
            state.chords.by_object.retain(|_, v| *v != *pair);
            b.disable();
            b.destroy();
        }
    }

    for (keysym, modifiers) in added {
        let binding = bind_chord(&xb, &river_seat, keysym, modifiers, &qh);
        // When a layer surface holds exclusive focus (sola-kvm capture),
        // leave new bindings disabled until focus is released. Otherwise
        // a RegisteredChords refresh mid-capture would re-steal Meta keys.
        if !state.layer_shell.exclusive_focus {
            binding.enable();
        }
        state
            .chords
            .by_object
            .insert(binding.id(), (keysym, modifiers));
        state.chords.by_chord.insert((keysym, modifiers), binding);
    }
    if state.layer_shell.exclusive_focus && added_n > 0 {
        state.layer_shell.chords_suppressed = true;
    }
    tracing::info!(
        total = state.chords.by_chord.len(),
        added = added_n,
        removed = removed_n,
        exclusive_focus = state.layer_shell.exclusive_focus,
        chords_suppressed = state.layer_shell.chords_suppressed,
        "applied RegisteredChords"
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use sola_bus::topics::WindowRect;

    fn rect(x: i32) -> WindowRect {
        WindowRect { x, y: 0, width: 10, height: 10 }
    }

    #[test]
    fn facts_are_sent_whole_when_one_changes_and_not_while_empty() {
        let mut known = HashMap::new();
        // Nothing known yet: nothing to say.
        assert_eq!(changed_facts(&mut known, 4, |f| f.parent = None), None);
        let first = changed_facts(&mut known, 4, |f| f.rect = Some(rect(1))).unwrap();
        assert_eq!(first.rect, Some(rect(1)));
        // Same again: quiet.
        assert_eq!(changed_facts(&mut known, 4, |f| f.rect = Some(rect(1))), None);
        // The tab joins the rect in one record.
        let second = changed_facts(&mut known, 4, |f| f.tab = Some("t".into())).unwrap();
        assert_eq!((second.rect, second.tab.as_deref()), (Some(rect(1)), Some("t")));
        // A parent that goes is cleared, not retracted, and the rest stays.
        changed_facts(&mut known, 4, |f| f.parent = Some(2)).unwrap();
        let cleared = changed_facts(&mut known, 4, |f| f.parent = None).unwrap();
        assert_eq!(cleared.parent, None);
        assert_eq!(cleared.tab.as_deref(), Some("t"));
        // Closing takes the record once.
        assert!(known.remove(&4).is_some());
        assert!(known.remove(&4).is_none());
    }
}
