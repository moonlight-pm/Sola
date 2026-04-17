//! Manage / render sequence handlers.
//!
//! When River sends `manage_start`, we push every accumulated size from
//! `PendingUpdate` into `propose_dimensions` (and ensure borders are off).
//! When it sends `render_start`, we apply composition (`place_top`),
//! positions (`set_position`), and focus (`focus_window` / `clear_focus`).
use tracing::info;

use crate::client::AppData;
use crate::pending::FocusAction;

// Per `river-window-management-v1.xml`, each request is marked as
// belonging to either the manage or render sequence:
//   Manage:  propose_dimensions, focus_window, clear_focus
//   Render:  node.set_position, node.place_top, window.set_borders
// Modifying state outside the right sequence triggers
// "invalid modification of window management state".

pub fn handle_manage_start(state: &mut AppData) {
    let Some(wm) = state.wm.clone() else { return };

    let pending_count = state.pending.manage.len();
    for (&window_id, &(w, h)) in &state.pending.manage {
        if let Some(proxy) = state.windows_by_id.get(&window_id) {
            proxy.propose_dimensions(w, h);
        }
    }
    state.pending.manage_dirty = false;
    state.pending.manage.clear();

    if let Some(focus) = state.pending.focus.take() {
        if let Some(seat) = state.seat.as_ref() {
            match focus {
                FocusAction::Window(id) => {
                    if let Some(proxy) = state.windows_by_id.get(&id) {
                        seat.focus_window(proxy);
                    }
                }
                FocusAction::None => seat.clear_focus(),
            }
        }
    }

    if let Some(pairs) = state.pending.chords.take() {
        crate::translator::apply_pending_chords(state, pairs);
    }

    wm.manage_finish();
    info!(pending_count, "manage_finish sent");
}

pub fn handle_render_start(state: &mut AppData) {
    let Some(wm) = state.wm.clone() else { return };
    use crate::protocol::river_window_management_v1::river_window_v1::Edges;

    let composition_len = state.pending.composition.as_ref().map(|c| c.len()).unwrap_or(0);
    let positions_len = state.pending.render_positions.len();

    // Disable borders for any window we've seen but not yet decorated.
    // Cheap to send repeatedly — River only diffs the resulting state.
    for proxy in state.windows_by_id.values() {
        proxy.set_borders(Edges::empty(), 0, 0, 0, 0, 0);
    }

    if let Some(order) = state.pending.composition.take() {
        // Anything in the order is visible; anything else hides. River's
        // `hide`/`show` are idempotent (no-op if already in that state).
        let visible: std::collections::HashSet<u32> = order.iter().copied().collect();
        for (&id, proxy) in &state.windows_by_id {
            if visible.contains(&id) {
                proxy.show();
            } else {
                proxy.hide();
            }
        }
        for &window_id in &order {
            if let Some(node) = state.nodes_by_window.get(&window_id) {
                node.place_top();
            }
        }
    }

    for (&window_id, &(x, y)) in &state.pending.render_positions {
        if let Some(node) = state.nodes_by_window.get(&window_id) {
            node.set_position(x, y);
        }
    }
    state.pending.render_positions.clear();

    state.pending.render_dirty = false;
    wm.render_finish();
    info!(composition_len, positions_len, "render_finish sent");
}
