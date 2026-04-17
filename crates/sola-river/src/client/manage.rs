//! Manage / render sequence handlers.
//!
//! When River sends `manage_start`, we push every accumulated size from
//! `PendingUpdate` into `propose_dimensions` (and ensure borders are off).
//! When it sends `render_start`, we apply composition (`place_top`),
//! positions (`set_position`), and focus (`focus_window` / `clear_focus`).
use tracing::debug;

use crate::client::AppData;
use crate::pending::FocusAction;

pub fn handle_manage_start(state: &mut AppData) {
    let Some(wm) = state.wm.clone() else { return };

    use crate::protocol::river_window_management_v1::river_window_v1::Edges;
    for (&window_id, &(w, h)) in &state.pending.manage {
        if let Some(proxy) = state.windows_by_id.get(&window_id) {
            proxy.propose_dimensions(w, h);
            // edges=empty + width=0 disables borders.
            proxy.set_borders(Edges::empty(), 0, 0, 0, 0, 0);
        }
    }
    // Mark manage as consumed so clear() won't re-trigger.
    state.pending.manage_dirty = false;
    state.pending.manage.clear();

    wm.manage_finish();
    debug!("manage_finish sent");
}

pub fn handle_render_start(state: &mut AppData) {
    let Some(wm) = state.wm.clone() else { return };

    if let Some(order) = state.pending.composition.take() {
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

    state.pending.render_dirty = false;
    wm.render_finish();
    debug!("render_finish sent");
}
