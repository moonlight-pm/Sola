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

        apply_pending_surfaces(state);

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

/// Drain and dispatch all pending bus messages.
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
            Topic::Composition(entries) => handle_composition(state, entries),
            Topic::Frame(update) => handle_frame(state, update),
            Topic::Focus(target) => handle_focus(state, target),
            _ => {
                tracing::debug!(topic = %msg.topic, "unhandled bus topic");
            }
        }
    }
}

/// Apply a Composition list: unmap surfaces not in the list, map/reorder
/// surfaces in the list (bottom to top).
fn handle_composition(state: &mut State, entries: Vec<sola_bus::topics::CompositionEntry>) {
    // First pass: find windows for each entry.
    let to_map: Vec<(smithay::desktop::Window, String, Option<String>)> = entries
        .iter()
        .filter_map(|entry| {
            state.find_surface(&entry.app_id, entry.title.as_deref())
                .map(|w| (w, entry.app_id.clone(), entry.title.clone()))
        })
        .collect();

    // Unmap all currently-mapped surfaces NOT in the composition list.
    let current: Vec<smithay::desktop::Window> = state.space.elements().cloned().collect();
    for window in &current {
        let dominated = to_map.iter().any(|(w, _, _)| w == window);
        if !dominated {
            state.space.unmap_elem(window);
            state.unmapped_surfaces.push(window.clone());
        }
    }

    // Map surfaces in list order (bottom to top).
    for (window, app_id, title) in &to_map {
        state.unmapped_surfaces.retain(|w| w != window);

        let key = (app_id.clone(), title.clone());
        let geo = state.frame_geometries.get(&key);

        let pos = geo.map(|g| (g.x, g.y)).unwrap_or((0, 0));
        state.space.map_element(window.clone(), pos, false);

        if let Some(geo) = geo {
            if let Some(toplevel) = window.toplevel() {
                toplevel.with_pending_state(|s| {
                    s.size = Some(Size::from((geo.width, geo.height)));
                });
                toplevel.send_pending_configure();
            }
        }
    }

    // Reorder: raise elements in list order so the last entry is on top.
    for (window, _, _) in &to_map {
        state.space.raise_element(window, false);
    }
}

/// Apply a Frame update: configure the surface with the given size and position.
fn handle_frame(state: &mut State, update: sola_bus::topics::FrameUpdate) {
    let key = (update.app_id.clone(), update.title.clone());

    // Store the geometry for future Composition mapping.
    state.frame_geometries.insert(key, update.clone());

    // If the surface exists (mapped or unmapped), configure it now.
    if let Some(window) = state.find_surface(&update.app_id, update.title.as_deref()) {
        // If already in Space, reposition.
        if state.window_by_app_id_title(&update.app_id, update.title.as_deref()).is_some() {
            state.space.map_element(window.clone(), (update.x, update.y), false);
        }

        if let Some(toplevel) = window.toplevel() {
            toplevel.with_pending_state(|s| {
                s.size = Some(Size::from((update.width, update.height)));
            });
            toplevel.send_pending_configure();
        }
    }
}

/// Apply a Focus target: set keyboard focus to the matching surface.
fn handle_focus(state: &mut State, target: sola_bus::topics::FocusTarget) {
    use smithay::wayland::seat::WaylandFocus;

    let window = state.window_by_app_id_title(&target.app_id, target.title.as_deref());
    let Some(window) = window else { return };
    let Some(surface) = window.wl_surface() else { return };

    state.applying_shell_focus = true;
    let serial = SERIAL_COUNTER.next_serial();
    let keyboard = state.seat.get_keyboard().unwrap();
    keyboard.set_focus(state, Some(surface.into_owned()), serial);
    state.applying_shell_focus = false;
}

/// Emit the current app list as a sticky bus message.
///
/// Called whenever surfaces appear or disappear. The shell uses this to
/// know which surfaces exist and compute composition.
pub(crate) fn emit_apps_list(state: &mut State) {
    use sola_bus::topics::{App, Topic};
    use std::collections::HashMap;

    let mut counts: HashMap<String, u32> = HashMap::new();

    // Count mapped surfaces.
    for window in state.space.elements() {
        if let Some(app_id) = State::app_id(window) {
            if app_id == "sola-shell" { continue; }
            *counts.entry(app_id).or_default() += 1;
        }
    }

    // Count unmapped surfaces (known to exist but not yet composed).
    for window in &state.unmapped_surfaces {
        if let Some(app_id) = State::app_id(window) {
            if app_id == "sola-shell" { continue; }
            *counts.entry(app_id).or_default() += 1;
        }
    }

    let mut app_ids: Vec<String> = counts.keys().cloned().collect();
    app_ids.sort();

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

/// Move pending surfaces whose app_id is now known to unmapped_surfaces.
/// Emit updated Apps list when new surfaces are detected.
///
/// Surfaces stay unmapped until the shell includes them in a Composition.
fn apply_pending_surfaces(state: &mut State) {
    use smithay::wayland::seat::WaylandFocus;

    let surfaces: Vec<_> = state.pending_surfaces.drain(..).collect();
    let mut still_pending = Vec::new();
    let mut new_surfaces = false;

    for window in surfaces {
        let Some(app_id) = State::app_id(&window) else {
            still_pending.push(window);
            continue;
        };

        let title = state::window_title(&window);
        tracing::info!(app_id = %app_id, title = ?title, "surface ready, waiting for composition");

        // The sola-shell menubar is the Super+key routing target.
        if app_id == "sola-shell" && title.as_deref() == Some("menubar") {
            if let Some(surface) = window.wl_surface() {
                let owned = surface.into_owned();
                setup_shell_keyboard_target(state, &owned);
                state.shell_keyboard_target = Some(owned);
            }
        }

        // If the app has no WindowPolicy, emit a default one.
        if !state.window_policies.contains_key(&app_id) {
            let default_policy = sola_bus::topics::WindowPolicyPayload {
                app_id: app_id.clone(),
                windows: vec![sola_bus::topics::WindowPolicy {
                    title: title.unwrap_or_default(),
                    zoned: true,
                    keyboard_target: false,
                    size: None,
                    position: None,
                }],
            };
            let _ = state.bus.emit_sticky(sola_bus::topics::Topic::SetWindowPolicy(default_policy.clone()));
            state.window_policies.insert(app_id, default_policy.windows);
        }

        state.unmapped_surfaces.push(window);
        new_surfaces = true;
    }

    state.pending_surfaces = still_pending;

    if new_surfaces {
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
