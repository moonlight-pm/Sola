/// Main event loop and shutdown logic.
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use smithay::reexports::wayland_server::Resource;
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
        let pre_refresh_count = state.space.elements().count();
        state.space.refresh();
        let post_refresh_count = state.space.elements().count();

        if post_refresh_count < pre_refresh_count {
            emit_apps_list(state);
        }

        display
            .dispatch_clients(state)
            .map_err(|e| CompositorError::Display(e.to_string()))?;

        // After dispatch: app_id and title are now set on pending surfaces.
        apply_pending_surfaces(state);
        apply_pending_geometries(state);
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

    let keyboard = state.seat.get_keyboard().unwrap();
    let Some(focused) = keyboard.current_focus() else { return };

    let app_id = state.space.elements().find_map(|window| {
        window.wl_surface()
            .filter(|s| s.as_ref() == &focused)
            .and_then(|_| State::app_id(window))
    });

    let Some(app_id) = app_id else { return };

    if state.mru_apps.first().is_some_and(|f| f == &app_id) { return; }

    state.mru_apps.retain(|id| id != &app_id);
    state.mru_apps.insert(0, app_id.clone());

    use sola_bus::topics::Topic;
    let _ = state.bus.emit_sticky(Topic::FocusChanged(app_id));

    emit_apps_list(state);
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
            Topic::RaiseApp(app_id) => handle_raise_app(state, &app_id),
            Topic::SetWindowGeometry(geo) => handle_set_window_geometry(state, &geo),
            _ => {
                tracing::debug!(topic = %msg.topic, "unhandled bus topic");
            }
        }
    }
}

/// Raise all windows belonging to the given app_id.
fn handle_raise_app(state: &mut State, app_id: &str) {
    let windows = state.windows_by_app_id(app_id);
    if windows.is_empty() {
        tracing::warn!(app_id, "RaiseApp: no windows found");
        return;
    }

    tracing::info!(app_id, count = windows.len(), "raising app");

    for window in &windows {
        state.space.raise_element(window, true);
    }

    // Only focus if the app has at least one auto_focus window.
    let should_focus = state.window_policies.get(app_id)
        .map_or(true, |ps| ps.iter().any(|p| p.auto_focus));

    if should_focus {
        use smithay::wayland::seat::WaylandFocus;
        if let Some(window) = windows.last() {
            if let Some(surface) = window.wl_surface() {
                let serial = SERIAL_COUNTER.next_serial();
                let keyboard = state.seat.get_keyboard().unwrap();
                keyboard.set_focus(state, Some(surface.into_owned()), serial);
            }
        }
    }
}

/// Apply any pending geometries whose windows now exist in the Space.
fn apply_pending_geometries(state: &mut State) {
    use sola_bus::topics::WindowGeometry;

    let matches: Vec<((String, Option<String>), WindowGeometry)> = state
        .pending_geometries
        .iter()
        .filter_map(|(key, geo)| {
            let window = state.window_by_app_id_title(&geo.app_id, geo.title.as_deref());
            if window.is_some() {
                Some((key.clone(), geo.clone()))
            } else {
                None
            }
        })
        .collect();

    for (key, geo) in matches {
        if let Some(window) = state.window_by_app_id_title(&geo.app_id, geo.title.as_deref()) {
            state.space.map_element(window.clone(), (geo.x, geo.y), false);

            if let Some(toplevel) = window.toplevel() {
                toplevel.with_pending_state(|s| {
                    s.size = Some((geo.width, geo.height).into());
                });
                toplevel.send_pending_configure();
            }
        }
        state.pending_geometries.remove(&key);
    }
}

/// Reposition and resize a window based on geometry from the bus.
/// If the window doesn't exist yet, store the geometry for later.
fn handle_set_window_geometry(state: &mut State, geo: &sola_bus::topics::WindowGeometry) {
    let window = state.window_by_app_id_title(&geo.app_id, geo.title.as_deref());
    if let Some(window) = window {
        state.space.map_element(window.clone(), (geo.x, geo.y), false);

        if let Some(toplevel) = window.toplevel() {
            toplevel.with_pending_state(|s| {
                s.size = Some((geo.width, geo.height).into());
            });
            toplevel.send_pending_configure();
        }
    }
    let key = (geo.app_id.clone(), geo.title.clone());
    state.pending_geometries.insert(key, geo.clone());
}

/// Emit the current app list as a sticky bus message.
///
/// Called whenever windows are mapped, unmapped, or MRU order changes.
/// The shell caches this list and uses it immediately when the switcher activates.
fn emit_apps_list(state: &mut State) {
    use sola_bus::topics::{App, Topic};
    use std::collections::HashMap;

    let mut counts: HashMap<String, u32> = HashMap::new();
    for window in state.space.elements() {
        if let Some(app_id) = State::app_id(window) {
            if app_id == "sola-shell" { continue; }
            *counts.entry(app_id).or_default() += 1;
        }
    }

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

    let _ = state.bus.emit_sticky(Topic::Apps(apps));
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

    let mut mapped_any = false;

    for (window, ref _app_id, policy) in to_map {
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

                // Cache keyboard_target surface for direct Super+key routing.
                if p.keyboard_target {
                    if let Some(surface) = window.wl_surface() {
                        let owned = surface.into_owned();
                        setup_shell_keyboard_target(state, &owned);
                        state.shell_keyboard_target = Some(owned);
                    }
                }
            }
            _ => {
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

        mapped_any = true;
    }

    if mapped_any {
        emit_apps_list(state);
    }
}

/// Send wl_keyboard.enter to the shell's keyboard_target surface so GTK
/// dispatches key events to it.
fn setup_shell_keyboard_target(
    state: &State,
    surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
) {
    let Some(client) = surface.client() else {
        return;
    };
    let keyboard = state.seat.get_keyboard().unwrap();
    let serial = SERIAL_COUNTER.next_serial();
    for kbd in keyboard.client_keyboards(&client) {
        kbd.enter(serial.into(), surface, vec![]);
        kbd.modifiers(serial.into(), 0, 0, 0, 0);
    }
    tracing::info!("shell keyboard_target surface registered");
}

/// Graceful shutdown — clean up all resources.
pub fn shutdown(mut state: State, display: Display<State>, event_loop: EventLoop<'static, State>) {
    tracing::info!("sola compositor shutting down");
    state.devices.clear();
    drop(display);
    drop(event_loop);
}
