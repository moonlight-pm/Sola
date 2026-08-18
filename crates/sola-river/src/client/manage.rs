//! Manage / render sequence handlers.
//!
//! When River sends `manage_start`, we push every accumulated size from
//! `PendingUpdate` into `propose_dimensions` (and ensure borders are off).
//! When it sends `render_start`, we apply composition (`place_top`),
//! positions (`set_position`), and focus (`focus_window` / `clear_focus`).
use std::collections::{HashMap, HashSet};

use tracing::debug;

use crate::client::AppData;
use crate::pending::FocusAction;

// Per `river-window-management-v1.xml`, each request is marked as
// belonging to either the manage or render sequence:
//   Manage:  propose_dimensions, focus_window, clear_focus
//   Render:  node.set_position, node.place_top, window.set_borders
// Modifying state outside the right sequence triggers
// "invalid modification of window management state".

/// Outcome of the first-`dimensions` gate for one window in a manage cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SizeDecision {
    /// Propose this size now.
    Propose(i32, i32),
    /// Self-size now (propose `(0, 0)`) and hold this size until the
    /// window's first `dimensions` event proves the surface is initialized.
    Defer(i32, i32),
}

/// Default host size for nested gamescope when the shell has not framed it
/// yet and the client self-size path yields `(0, 0)`. Matches Arcade's
/// default nest (`-W 1920 -H 1080`).
pub(crate) const GAMESCOPE_DEFAULT_W: i32 = 1920;
pub(crate) const GAMESCOPE_DEFAULT_H: i32 = 1080;

/// Decide what to propose for a window this manage cycle.
///
/// `river-window-management-v1` guarantees a window is not displayed until
/// its first `dimensions` event. Sending a *sizing* configure before that
/// event can invalidate a client's swapchain mid-init — UnrealEditor
/// (Vulkan/SDL3) dies exactly this way. So any real size requested before
/// initialization is deferred; the window self-sizes first and takes the
/// real size as a normal runtime resize one cycle later. A `(0, 0)` request
/// is "client decides its own size" and is always safe **except for
/// gamescope**: its xdg host never self-sizes from `(0, 0)` — it stays
/// mapped with no visible surface (switcher shows "Gamescope", pixels
/// never appear). Force a real host configure for that app_id.
///
/// After first dimensions, gamescope is treated like any other window:
/// shell Frames (zone / float) set host size freely. Nested game content
/// is letterbox-scaled by gamescope `-S fit` into that host; game-internal
/// resolution is independent of the host size.
pub(crate) fn size_decision(
    requested: (i32, i32),
    initialized: bool,
    app_id: &str,
) -> SizeDecision {
    if crate::proc_identity::is_gamescope_app_id(app_id) {
        // Pre-init: pin a real host size so the nest can commit a first
        // buffer (self-size `(0,0)` never maps pixels under River).
        // Post-init: honor shell Frames (zone/float) like any app.
        if !initialized {
            return SizeDecision::Propose(GAMESCOPE_DEFAULT_W, GAMESCOPE_DEFAULT_H);
        }
        let (w, h) = if requested.0 > 0 && requested.1 > 0 {
            requested
        } else {
            (GAMESCOPE_DEFAULT_W, GAMESCOPE_DEFAULT_H)
        };
        return SizeDecision::Propose(w, h);
    }
    if !initialized && requested != (0, 0) {
        SizeDecision::Defer(requested.0, requested.1)
    } else {
        SizeDecision::Propose(requested.0, requested.1)
    }
}

/// Record that `window_id` received its first `dimensions` event and return
/// any size that was deferred waiting for it, so the caller can re-queue it
/// for the next manage cycle. Returns `None` if nothing was deferred.
pub(crate) fn note_dimensions(
    first_dimensions: &mut HashSet<u32>,
    deferred_size: &mut HashMap<u32, (i32, i32)>,
    window_id: u32,
) -> Option<(i32, i32)> {
    first_dimensions.insert(window_id);
    deferred_size.remove(&window_id)
}


/// Whether a size or position must be forwarded to River.
///
/// `last` is the value we most recently sent for this window (`None` if we
/// have sent nothing yet). We only forward a value that differs from the
/// last one: River keeps a window at its current dimensions/position without
/// a fresh request, so re-sending an identical `propose_dimensions` or
/// `set_position` is pure churn. That churn matters — every redundant
/// `propose_dimensions` makes River send the client another configure, and
/// those pile up against a client that is busy and not servicing its socket
/// (UnrealEditor blocks its Wayland thread for seconds during a project
/// load), which can overflow the connection and get the client dropped.
pub(crate) fn should_send(last: Option<(i32, i32)>, requested: (i32, i32)) -> bool {
    last != Some(requested)
}

/// Minimum interval between **size** configures for gamescope hosts.
///
/// Shell zone Frames rebroadcast often; rapid host resizes under the wayland
/// backend have been observed to abort gamescope's input thread
/// (`CWaylandInputThread::ThreadFunc` → SIGABRT) while Steam is still in
/// prepare/shader phases. Coalesce size proposes; position still updates.
pub(crate) const GAMESCOPE_SIZE_DEBOUNCE: std::time::Duration =
    std::time::Duration::from_millis(750);

/// Hold the first real host size this long after first `dimensions` before
/// accepting a different shell Frame size. Gives Steam cold-start + shader
/// interstitial time a stable nest; zoning still applies after the hold.
/// Keep short so float/zone after prepare are not delayed for a long time.
pub(crate) const GAMESCOPE_SIZE_HOLD: std::time::Duration =
    std::time::Duration::from_secs(12);

/// Decide whether to apply a new gamescope host size now, or keep `last`.
///
/// Pure helper for unit tests — call sites pass wall-clock `now` and the
/// instant of first dimensions / last size send.
pub(crate) fn gamescope_size_gate(
    requested: (i32, i32),
    last: Option<(i32, i32)>,
    first_dimensions_at: Option<std::time::Instant>,
    last_size_sent_at: Option<std::time::Instant>,
    now: std::time::Instant,
) -> (i32, i32) {
    // No prior size: take the request (or nest default).
    let Some(last) = last else {
        return if requested.0 > 0 && requested.1 > 0 {
            requested
        } else {
            (GAMESCOPE_DEFAULT_W, GAMESCOPE_DEFAULT_H)
        };
    };
    if requested == last {
        return last;
    }
    // Hold period after first map: keep last (stable nest for Steam prepare).
    if let Some(t0) = first_dimensions_at {
        if now.duration_since(t0) < GAMESCOPE_SIZE_HOLD {
            return last;
        }
    }
    // Debounce rapid zone rebroadcasts.
    if let Some(t_sent) = last_size_sent_at {
        if now.duration_since(t_sent) < GAMESCOPE_SIZE_DEBOUNCE {
            return last;
        }
    }
    if requested.0 > 0 && requested.1 > 0 {
        requested
    } else {
        last
    }
}

pub fn handle_manage_start(state: &mut AppData) {
    let Some(wm) = state.wm.clone() else { return };

    // Layer-shell default output (manage-sequence-only request).
    crate::client::layer_shell::on_manage_start(state);

    // Drain into an owned vec so we can mutate `state.deferred_size` inside
    // the loop without aliasing `state.pending.manage`.
    let manage: Vec<(u32, (i32, i32))> = state.pending.manage.drain().collect();
    let pending_count = manage.len();
    for (window_id, (w, h)) in manage {
        let Some(proxy) = state.windows_by_id.get(&window_id).cloned() else {
            continue;
        };
        let app_id = state
            .registry
            .app_id_for(window_id)
            .unwrap_or("?")
            .to_string();
        let initialized = state.first_dimensions.contains(&window_id);
        let mut propose = match size_decision((w, h), initialized, &app_id) {
            SizeDecision::Propose(pw, ph) => (pw, ph),
            SizeDecision::Defer(dw, dh) => {
                tracing::info!(
                    window_id,
                    %app_id,
                    w = dw,
                    h = dh,
                    "deferring size until first dimensions; self-sizing"
                );
                state.deferred_size.insert(window_id, (dw, dh));
                // Self-size now; the held size is re-queued on first dimensions.
                (0, 0)
            }
        };
        // gamescope: hold + debounce host size so Steam prepare / shader
        // interstitials run on a stable nest (input-thread abort under thrash).
        // Skip entirely during interactive CSD resize/move — those
        // must apply every delta or the gesture feels axis-locked / stuck.
        let op_on_this = state
            .op
            .as_ref()
            .is_some_and(|op| op.window_id == window_id);
        if crate::proc_identity::is_gamescope_app_id(&app_id) && initialized && !op_on_this {
            let now = std::time::Instant::now();
            let gated = gamescope_size_gate(
                propose,
                state.last_proposed.get(&window_id).copied(),
                state.gamescope_first_dim_at.get(&window_id).copied(),
                state.gamescope_last_size_at.get(&window_id).copied(),
                now,
            );
            if gated != propose {
                tracing::debug!(
                    window_id,
                    from = ?propose,
                    keep = ?gated,
                    "gamescope size hold/debounce"
                );
            }
            propose = gated;
        }
        // Skip re-proposing an unchanged size: re-sending an identical
        // configure only adds Wayland traffic that piles up against a busy
        // client. See `should_send`.
        if should_send(state.last_proposed.get(&window_id).copied(), propose) {
            tracing::info!(window_id, %app_id, w = propose.0, h = propose.1, "propose_dimensions");
            proxy.propose_dimensions(propose.0, propose.1);
            state.last_proposed.insert(window_id, propose);
            if crate::proc_identity::is_gamescope_app_id(&app_id) {
                state
                    .gamescope_last_size_at
                    .insert(window_id, std::time::Instant::now());
            }
        }
    }
    state.pending.manage_dirty = false;
    // `pending.manage` was drained above; no separate clear needed.

    // While a layer surface holds exclusive focus (e.g. sola-kvm edge
    // capture), River ignores focus_window/clear_focus. Keep the pending
    // action so we re-apply it on focus_none.
    if !state.layer_shell.exclusive_focus {
        if let Some(focus) = state.pending.focus.take() {
            if let Some(seat) = state.seat.as_ref() {
                match focus {
                    FocusAction::Window(id) => {
                        // Re-calling focus_window on the already-focused
                        // surface can make clients (Electron) re-enter
                        // keyboard focus and flash. Skip when seat state
                        // already matches the request.
                        if state.focused_window == Some(id) {
                            tracing::debug!(
                                window_id = id,
                                "skip seat.focus_window (already focused)"
                            );
                        } else if let Some(proxy) = state.windows_by_id.get(&id) {
                            tracing::debug!(
                                window_id = id,
                                prev = ?state.focused_window,
                                "seat.focus_window"
                            );
                            seat.focus_window(proxy);
                            state.focused_window = Some(id);
                        }
                    }
                    FocusAction::None => {
                        if state.focused_window.is_none() {
                            tracing::debug!("skip seat.clear_focus (already clear)");
                        } else {
                            tracing::debug!(
                                prev = ?state.focused_window,
                                "seat.clear_focus"
                            );
                            seat.clear_focus();
                            state.focused_window = None;
                        }
                    }
                }
            }
        }
    }

    if let Some(pairs) = state.pending.chords.take() {
        crate::translator::apply_pending_chords(state, pairs);
    }

    // After chord set updates: if a layer client has exclusive focus
    // (sola-kvm capture), keep shell Meta chords disabled so keys reach
    // the Mac. Newly added bindings from apply_pending_chords are
    // enabled by default — re-suppress them here.
    crate::client::layer_shell::sync_chord_suppression(state);

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

    // CSD move/resize: cursor device + any pending op_start_pointer/op_end.
    // No Super+button pointer bindings — those clicks reach clients.
    crate::client::op::ensure_op_cursor(state);
    crate::client::op::drive(state);

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
        //
        // gamescope is treated like any other app: stack order is solely the
        // shell's Composition list (MRU + overlays). We used to never-hide and
        // re-`place_top` gamescope after the stack walk so the nest couldn't
        // be buried during early paint races — that locked the host on top of
        // every normal app and contributed to z-order thrash / flicker.
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

    // Drain into an owned Vec first: the body below mutates `state` (registry +
    // bus via `emit_geometry`), which can't coexist with an immutable borrow of
    // `state.pending.render_positions`.
    let positions: Vec<(u32, (i32, i32))> = state
        .pending
        .render_positions
        .iter()
        .map(|(&id, &xy)| (id, xy))
        .collect();
    state.pending.render_positions.clear();
    for (window_id, (x, y)) in positions {
        if !state.nodes_by_window.contains_key(&window_id) {
            continue;
        }
        // Skip repositioning a window that has not moved — the shell
        // re-broadcasts frames for every window on any change, so without
        // this we re-issue set_position for windows that stayed put. See
        // `should_send`.
        if should_send(state.last_position.get(&window_id).copied(), (x, y)) {
            if let Some(node) = state.nodes_by_window.get(&window_id) {
                node.set_position(x, y);
            }
            state.last_position.insert(window_id, (x, y));
            // Record actual position and publish geometry on change. (node
            // borrow above is dropped before this whole-state mutation.)
            if state.registry.set_position(window_id, x, y) {
                crate::translator::emit_geometry(state, window_id);
            }
        }
        state.placed.insert(window_id);
    }

    apply_default_placement(state);

    // Floating drop shadows: decoration-below surfaces, offset outside the
    // content rect. Must run in the render sequence (set_offset /
    // sync_next_commit are render-state). Before render_finish so River
    // can apply the decoration commit atomically with this frame.
    crate::client::shadow::sync_on_render(state);

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
        if state.nodes_by_window.contains_key(&window_id) {
            if let Some(node) = state.nodes_by_window.get(&window_id) {
                node.set_position(x, y);
            }
            state.placed.insert(window_id);
            debug!(window_id, x, y, w, h, "default-centered unzoned window");
            // Record actual position and publish geometry on change.
            if state.registry.set_position(window_id, x, y) {
                crate::translator::emit_geometry(state, window_id);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn uninitialized_real_size_is_deferred() {
        assert_eq!(
            size_decision((800, 600), false, "UnrealEditor"),
            SizeDecision::Defer(800, 600)
        );
    }

    #[test]
    fn initialized_real_size_is_proposed() {
        assert_eq!(
            size_decision((800, 600), true, "UnrealEditor"),
            SizeDecision::Propose(800, 600)
        );
    }

    #[test]
    fn self_size_is_always_proposed() {
        // (0,0) means "client decides" — safe pre-init and post-init.
        assert_eq!(
            size_decision((0, 0), false, "Helium"),
            SizeDecision::Propose(0, 0)
        );
        assert_eq!(
            size_decision((0, 0), true, "Helium"),
            SizeDecision::Propose(0, 0)
        );
    }

    #[test]
    fn gamescope_pre_init_pins_nest_default_post_init_honors_frame() {
        // Pre-init always 1920×1080 so the host can map.
        assert_eq!(
            size_decision((5020, 2032), false, "gamescope"),
            SizeDecision::Propose(GAMESCOPE_DEFAULT_W, GAMESCOPE_DEFAULT_H)
        );
        assert_eq!(
            size_decision((0, 0), false, "gamescope"),
            SizeDecision::Propose(GAMESCOPE_DEFAULT_W, GAMESCOPE_DEFAULT_H)
        );
        // Post-init: zone / float Frames set host size freely (gate is separate).
        assert_eq!(
            size_decision((1434, 2132), true, "gamescope"),
            SizeDecision::Propose(1434, 2132)
        );
        assert_eq!(
            size_decision((1280, 720), true, "Gamescope"),
            SizeDecision::Propose(1280, 720)
        );
    }

    #[test]
    fn gamescope_size_gate_holds_then_allows() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let last = (1920, 1080);
        // During hold: keep last even if zone asks for something else.
        let during = gamescope_size_gate(
            (2253, 2132),
            Some(last),
            Some(t0),
            Some(t0),
            t0 + Duration::from_secs(10),
        );
        assert_eq!(during, last);
        // After hold + debounce: accept zone size.
        let after = gamescope_size_gate(
            (2253, 2132),
            Some(last),
            Some(t0),
            Some(t0),
            t0 + GAMESCOPE_SIZE_HOLD + GAMESCOPE_SIZE_DEBOUNCE + Duration::from_millis(50),
        );
        assert_eq!(after, (2253, 2132));
    }

    #[test]
    fn note_dimensions_hands_back_deferred_size_once() {
        let mut first = HashSet::new();
        let mut deferred = HashMap::new();
        deferred.insert(7u32, (1280, 720));

        // First dimensions event: marks initialized, returns the held size.
        assert_eq!(note_dimensions(&mut first, &mut deferred, 7), Some((1280, 720)));
        assert!(first.contains(&7));
        assert!(deferred.is_empty());

        // Second event: already initialized, nothing left to apply.
        assert_eq!(note_dimensions(&mut first, &mut deferred, 7), None);
        assert!(first.contains(&7));
    }


    #[test]
    fn first_value_is_always_sent() {
        // Nothing sent yet for this window — the first propose/position must
        // go through, even the self-size (0, 0) that the protocol requires
        // before a window is displayed.
        assert!(should_send(None, (0, 0)));
        assert!(should_send(None, (2253, 2132)));
    }

    #[test]
    fn unchanged_value_is_skipped() {
        // Re-sending an identical size/position is pure churn: River keeps
        // the window as-is, and an identical configure only piles up Wayland
        // traffic against clients that are busy and not reading their socket.
        assert!(!should_send(Some((0, 0)), (0, 0)));
        assert!(!should_send(Some((2253, 2132)), (2253, 2132)));
    }

    #[test]
    fn changed_value_is_sent() {
        // Deferred self-size (0, 0) → real size must propose; a real resize
        // must propose.
        assert!(should_send(Some((0, 0)), (2253, 2132)));
        assert!(should_send(Some((2253, 2132)), (1000, 1000)));
    }
}
