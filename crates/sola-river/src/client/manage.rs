//! Manage / render sequence handlers.
//!
//! When River sends `manage_start`, we push every accumulated size from
//! `PendingUpdate` into `propose_dimensions` (and ensure borders are off).
//! When it sends `render_start`, we apply composition (`place_top`),
//! positions (`set_position`), and focus (`focus_window` / `clear_focus`).
use tracing::debug;

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
            let app_id = state.registry.app_id_for(window_id).unwrap_or("?");
            tracing::info!(window_id, app_id, w, h, "propose_dimensions");
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
                        state.focused_window = Some(id);
                    }
                }
                FocusAction::None => {
                    seat.clear_focus();
                    state.focused_window = None;
                }
            }
        }
    }

    if let Some(pairs) = state.pending.chords.take() {
        crate::translator::apply_pending_chords(state, pairs);
    }

    let close_ids: Vec<u32> = std::mem::take(&mut state.pending.close_windows);
    let mut close_count = 0;
    for window_id in close_ids {
        if let Some(proxy) = state.windows_by_id.get(&window_id) {
            proxy.close();
            close_count += 1;
        }
    }
    if close_count > 0 {
        tracing::info!(close_count, "CloseApp: sent river_window_v1.close");
    }

    apply_fullscreen_requests(state);

    wm.manage_finish();
    debug!(pending_count, "manage_finish sent");
}

/// Honor pending fullscreen / exit-fullscreen events from `river_window_v1`.
///
/// Granting a fullscreen request keeps Xwayland clients (Wine/Proton games,
/// SDL apps, etc.) on the WM-managed surface. If we ignore the request, the
/// X client falls back to creating a separate override-redirect surface
/// for its fullscreen output — that surface bypasses the WM entirely
/// (invisible to sola-river, can't be focused or zoned), which manifests
/// as a "second render" floating above the desktop with no input routing.
fn apply_fullscreen_requests(state: &mut AppData) {
    let fullscreen_ids: Vec<u32> = std::mem::take(&mut state.pending.fullscreen_requests);
    let exit_ids: Vec<u32> = std::mem::take(&mut state.pending.exit_fullscreen_requests);

    // Sola is single-output today; we use the first bound `river_output_v1`,
    // matching the assumption already made elsewhere in sola-river (e.g.
    // `output_size`). If no output is bound yet, fullscreen requests are
    // dropped — but `exit_fullscreen` still runs, since it takes no output.
    let output = state.outputs.first().cloned();
    if !fullscreen_ids.is_empty() && output.is_none() {
        tracing::warn!(
            count = fullscreen_ids.len(),
            "fullscreen request before any river_output_v1 was bound; dropping"
        );
    }
    if let Some(output) = output {
        for window_id in fullscreen_ids {
            if let Some(proxy) = state.windows_by_id.get(&window_id) {
                proxy.fullscreen(&output);
                proxy.inform_fullscreen();
                state.currently_fullscreen.insert(window_id);
                tracing::info!(window_id, "granted fullscreen");
            }
        }
    }

    for window_id in exit_ids {
        if let Some(proxy) = state.windows_by_id.get(&window_id) {
            proxy.exit_fullscreen();
            proxy.inform_not_fullscreen();
            state.currently_fullscreen.remove(&window_id);
            tracing::info!(window_id, "exited fullscreen");
        }
    }
}

pub fn handle_render_start(state: &mut AppData) {
    let Some(wm) = state.wm.clone() else { return };
    use crate::protocol::river_window_management_v1::river_window_v1::Edges;

    let composition_len = state
        .pending
        .composition
        .as_ref()
        .map(|c| c.len())
        .unwrap_or(0);
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
            state.placed.insert(window_id);
        }
    }
    state.pending.render_positions.clear();

    apply_default_placement(state);

    state.pending.render_dirty = false;
    wm.render_finish();
    debug!(composition_len, positions_len, "render_finish sent");
}

/// Center any window that's visible in the current composition but has
/// never had an explicit position set by the shell. River otherwise
/// places unpositioned nodes at (0, 0), which parks unzoned windows in
/// the top-left corner.
fn apply_default_placement(state: &mut AppData) {
    let Some((out_w, out_h)) = state.output_size else {
        return;
    };

    // Derive the set of windows currently visible from the same source
    // `handle_render_start` uses: composition entries are shown, others
    // hidden. Placement for hidden windows would be wasted since they
    // aren't on screen.
    let visible: Vec<u32> = state.windows_by_id.keys().copied().collect();

    for window_id in visible {
        if state.placed.contains(&window_id) {
            continue;
        }
        let (w, h) = default_size_for(state, window_id, out_w, out_h);
        let x = ((out_w - w) / 2).max(0);
        let y = ((out_h - h) / 2).max(0);
        if let Some(node) = state.nodes_by_window.get(&window_id) {
            node.set_position(x, y);
            state.placed.insert(window_id);
            debug!(window_id, x, y, w, h, "default-centered unzoned window");
        }
    }
}

/// Pick a size to use for centering math. Prefer the app's `dimensions_hint`
/// max when it's set; otherwise fall back to a sane default bounded by the
/// output. We never actually propose these dimensions — they're only used
/// to compute an offset that roughly centers the window.
fn default_size_for(state: &AppData, window_id: u32, out_w: i32, out_h: i32) -> (i32, i32) {
    let hint = state
        .registry
        .get(window_id)
        .map(|e| e.max_size)
        .unwrap_or((0, 0));
    let fallback_w = (out_w / 2).clamp(800, 1600);
    let fallback_h = (out_h / 2).clamp(600, 1000);
    let w = if hint.0 > 0 { hint.0 } else { fallback_w };
    let h = if hint.1 > 0 { hint.1 } else { fallback_h };
    (w.min(out_w), h.min(out_h))
}
