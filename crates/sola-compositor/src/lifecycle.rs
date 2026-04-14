/// Main event loop and shutdown logic.
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use smithay::utils::{Size, SERIAL_COUNTER};

use crate::error::CompositorError;
use crate::output::render;
use crate::state::{self, State};

/// Run the main event loop until `state.running` becomes false.
pub fn run_loop(
    state: &mut State,
    display: &mut Display<State>,
    event_loop: &mut EventLoop<'static, State>,
) -> Result<(), CompositorError> {
    tracing::info!("entering event loop");

    while state.running {
        // Match pending surfaces to window policies and map them.
        apply_pending_surfaces(state);

        // Apply geometries stored from previous bus messages whose
        // windows have since appeared via the Wayland protocol.
        apply_pending_geometries(state);

        state.space.refresh();

        display
            .dispatch_clients(state)
            .map_err(|e| CompositorError::Display(e.to_string()))?;

        // Sync MRU after dispatch — set_app_id may have arrived after
        // the set_focus that triggered focus_changed, so retry now.
        sync_mru(state);

        display
            .flush_clients()
            .map_err(|e| CompositorError::Display(e.to_string()))?;

        render::render_all(state);

        event_loop
            .dispatch(Some(std::time::Duration::from_millis(16)), state)
            .map_err(|e| CompositorError::EventLoop(e.to_string()))?;
    }

    Ok(())
}

/// Ensure the MRU list reflects the current keyboard focus.
///
/// `focus_changed` can miss updates because `set_app_id` arrives after
/// `set_focus` in the Wayland protocol stream. This runs after
/// `dispatch_clients` when all protocol state is settled.
fn sync_mru(state: &mut State) {
    use smithay::wayland::seat::WaylandFocus;

    if state.input_grab.is_some() { return; }

    let keyboard = state.seat.get_keyboard().unwrap();
    let Some(focused) = keyboard.current_focus() else { return };

    let app_id = state.space.elements().find_map(|window| {
        window.wl_surface()
            .filter(|s| s.as_ref() == &focused)
            .and_then(|_| State::app_id(window))
    });

    let Some(app_id) = app_id else { return };

    // Already at the front — nothing to do.
    if state.mru_apps.first().is_some_and(|f| f == &app_id) { return; }

    state.mru_apps.retain(|id| id != &app_id);
    state.mru_apps.insert(0, app_id.clone());

    // Emit FocusChanged that seat::focus_changed missed due to the
    // set_app_id / set_focus race.
    use sola_bus::topics::Topic;
    let _ = state.bus.emit_sticky(Topic::FocusChanged(app_id));
}

/// Drain and dispatch all pending bus messages.
///
/// Called from the calloop bus event source when the notification fd
/// signals readable. Not called from the frame loop.
pub(crate) fn dispatch_bus(state: &mut State) {
    use sola_bus::topics::Topic;

    state.bus.drain_notify();

    let mut messages = Vec::new();
    while let Some(msg) = state.bus.try_recv() {
        messages.push(msg);
    }

    for msg in &messages {
        let Some(topic) = Topic::parse(msg) else {
            tracing::debug!(topic = %msg.topic, "unknown bus topic");
            continue;
        };

        match topic {
            Topic::SetWindowPolicy(payload) => handle_set_window_policy(state, payload),
            Topic::GrabInput(target) => handle_grab_input(state, &target),
            Topic::ReleaseInput => handle_release_input(state),
            Topic::RaiseApp(app_id) => handle_raise_app(state, &app_id),
            Topic::SetWindowGeometry(geo) => handle_set_window_geometry(state, &geo),
            Topic::ListApps => handle_list_apps(state),
            _ => {
                tracing::debug!(topic = %msg.topic, "unhandled bus topic");
            }
        }
    }
}

/// Set exclusive input grab for the target app.
///
/// Always sets the grab immediately so key routing switches away from the bus.
/// If the target window exists, raise and focus it. If not (race with window
/// mapping), `new_toplevel` will handle raise+focus when the window appears.
fn handle_grab_input(state: &mut State, target: &str) {
    tracing::info!(target, "grabbing input");
    state.input_grab = Some(target.to_string());

    if let Some(window) = state.window_by_app_id(target) {
        use smithay::wayland::seat::WaylandFocus;
        state.space.raise_element(&window, true);
        if let Some(surface) = window.wl_surface() {
            let serial = SERIAL_COUNTER.next_serial();
            let keyboard = state.seat.get_keyboard().unwrap();
            keyboard.set_focus(state, Some(surface.into_owned()), serial);
        }
    } else {
        tracing::debug!(target, "window not yet mapped, will focus on arrival");
    }
}

/// Release the input grab.
///
/// Callers must emit RaiseApp before ReleaseInput so that a window has
/// focus when the grab clears. The grabbed surface stays mapped — the
/// raised app covers it, and the shell app clears its own UI.
fn handle_release_input(state: &mut State) {
    let Some(target) = state.input_grab.take() else {
        return;
    };
    tracing::info!(target = %target, "releasing input");
}

/// Raise all windows belonging to the given app_id.
fn handle_raise_app(state: &mut State, app_id: &str) {
    let windows = state.windows_by_app_id(app_id);
    if windows.is_empty() {
        tracing::warn!(app_id, "RaiseApp: no windows found");
        return;
    }

    tracing::info!(app_id, count = windows.len(), "raising app");

    // Raise each window, maintaining their relative z-order.
    // The last one raised gets focus.
    for window in &windows {
        state.space.raise_element(window, true);
    }

    // Focus the topmost window of the raised app.
    use smithay::wayland::seat::WaylandFocus;
    if let Some(window) = windows.last() {
        if let Some(surface) = window.wl_surface() {
            let serial = SERIAL_COUNTER.next_serial();
            let keyboard = state.seat.get_keyboard().unwrap();
            keyboard.set_focus(state, Some(surface.into_owned()), serial);
        }
    }
}

/// Apply any pending geometries whose windows now exist in the Space.
fn apply_pending_geometries(state: &mut State) {
    use sola_bus::topics::WindowGeometry;

    let matches: Vec<WindowGeometry> = state
        .pending_geometries
        .iter()
        .filter_map(|(app_id, geo)| {
            if state.window_by_app_id(app_id).is_some() {
                Some(geo.clone())
            } else {
                None
            }
        })
        .collect();

    for geo in matches {
        if let Some(window) = state.window_by_app_id(&geo.app_id) {
            state.space.map_element(window.clone(), (geo.x, geo.y), false);

            if let Some(toplevel) = window.toplevel() {
                toplevel.with_pending_state(|s| {
                    s.size = Some((geo.width, geo.height).into());
                });
                toplevel.send_pending_configure();
            }
        }
        state.pending_geometries.remove(&geo.app_id);
    }
}

/// Reposition and resize a window based on geometry from the bus.
/// If the window doesn't exist yet, store the geometry for later.
fn handle_set_window_geometry(state: &mut State, geo: &sola_bus::topics::WindowGeometry) {
    if let Some(window) = state.window_by_app_id(&geo.app_id) {
        state.space.map_element(window.clone(), (geo.x, geo.y), false);

        // Configure the toplevel with the target size so the client resizes.
        if let Some(toplevel) = window.toplevel() {
            toplevel.with_pending_state(|s| {
                s.size = Some((geo.width, geo.height).into());
            });
            toplevel.send_pending_configure();
        }
    }
    // Always store — the window might appear later via new_toplevel.
    state.pending_geometries.insert(geo.app_id.clone(), geo.clone());
}

/// Respond to a ListApps request.
///
/// Scans all mapped windows for app_ids, deduplicates, and orders by MRU.
/// Excludes the current input grab target (e.g., the switcher itself).
fn handle_list_apps(state: &mut State) {
    use sola_bus::topics::App;
    use std::collections::HashMap;

    // Count windows per app_id from the Space.
    let mut counts: HashMap<String, u32> = HashMap::new();
    for window in state.space.elements() {
        if let Some(app_id) = State::app_id(window) {
            *counts.entry(app_id).or_default() += 1;
        }
    }

    // Exclude the grab target (e.g., the switcher).
    if let Some(ref target) = state.input_grab {
        counts.remove(target);
    }

    // Order by MRU position (known apps first), then alphabetical for unknown.
    let mut app_ids: Vec<String> = counts.keys().cloned().collect();
    app_ids.sort_by_key(|id| {
        state.mru_apps.iter().position(|m| m == id).unwrap_or(usize::MAX)
    });

    let apps: Vec<App> = app_ids.into_iter().map(|app_id| {
        let window_count = counts[&app_id];
        App {
            name: app_id.clone(),
            icon: "app".into(),
            window_count,
            app_id,
        }
    }).collect();

    tracing::debug!(count = apps.len(), "responding to ListApps");

    use sola_bus::topics::Topic;
    let _ = state.bus.emit(Topic::Apps(apps));
}

fn handle_set_window_policy(
    state: &mut State,
    payload: sola_bus::topics::WindowPolicyPayload,
) {
    tracing::info!(
        app_id = %payload.app_id,
        windows = payload.windows.len(),
        "registered window policy"
    );
    state
        .window_policies
        .insert(payload.app_id.clone(), payload.windows);
}

/// Map pending surfaces whose app_id is known and can be matched to a policy.
///
/// - Surfaces with a matching policy: apply the declared sizing and focus rules.
/// - Surfaces with a known app_id but no policy: apply defaults (full size, auto-focus).
/// - Surfaces with no app_id yet: keep pending (retry next frame).
fn apply_pending_surfaces(state: &mut State) {
    use smithay::wayland::seat::WaylandFocus;

    let mut still_pending = Vec::new();
    let mut to_map: Vec<(
        smithay::desktop::Window,
        String,
        Option<sola_bus::topics::WindowPolicy>,
    )> = Vec::new();

    for window in state.pending_surfaces.drain(..) {
        let app_id = State::app_id(&window);
        let Some(app_id) = app_id else {
            still_pending.push(window);
            continue;
        };

        let title = state::window_title(&window);
        let policy = state.window_policies.get(&app_id).and_then(|policies| {
            title
                .as_ref()
                .and_then(|t| policies.iter().find(|p| p.title == *t))
                .cloned()
        });

        tracing::info!(
            app_id = %app_id,
            title = ?title,
            has_policy = policy.is_some(),
            "mapping surface"
        );

        to_map.push((window, app_id, policy));
    }

    state.pending_surfaces = still_pending;

    for (window, _app_id, policy) in to_map {
        let should_focus = policy.as_ref().map_or(true, |p| p.auto_focus);

        match policy {
            Some(ref p) if !p.zoned => {
                let pos = p.position.unwrap_or((0, 0));
                state
                    .space
                    .map_element(window.clone(), pos, should_focus);

                if let Some((w, h)) = p.size {
                    if let Some(toplevel) = window.toplevel() {
                        toplevel.with_pending_state(|s| {
                            s.size = Some(Size::from((w, h)));
                        });
                        toplevel.send_pending_configure();
                    }
                }
            }
            _ => {
                // Zoned or no policy: suggest full output size
                if let Some(mode) =
                    state.space.outputs().next().and_then(|o| o.current_mode())
                {
                    if let Some(toplevel) = window.toplevel() {
                        toplevel.with_pending_state(|s| {
                            s.size = Some(Size::from((mode.size.w, mode.size.h)));
                        });
                        toplevel.send_pending_configure();
                    }
                }
                state.space.map_element(window.clone(), (0, 0), true);
            }
        }

        if should_focus {
            if let Some(surface) = window.wl_surface() {
                let serial = SERIAL_COUNTER.next_serial();
                let keyboard = state.seat.get_keyboard().unwrap();
                keyboard.set_focus(state, Some(surface.into_owned()), serial);
            }
        }
    }
}

/// Graceful shutdown — clean up all resources.
pub fn shutdown(mut state: State, display: Display<State>, event_loop: EventLoop<'static, State>) {
    tracing::info!("sola compositor shutting down");
    state.devices.clear();
    drop(display);
    drop(event_loop);
}
