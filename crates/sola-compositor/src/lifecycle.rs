/// Main event loop and shutdown logic.
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use smithay::reexports::wayland_server::Resource;
use smithay::utils::{SERIAL_COUNTER, Size};

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
            // Clean up window_ids for surfaces that space.refresh() removed.
            state.window_ids.retain(|_, w| {
                state.space.elements().any(|e| e == w)
                    || state.unmapped_surfaces.contains(w)
                    || state.pending_surfaces.contains(w)
            });
            emit_windows_list(state);
        }

        display
            .dispatch_clients(state)
            .map_err(|e| CompositorError::Display(e.to_string()))?;

        apply_pending_surfaces(state);
        apply_pending_or_windows(state);

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
            Topic::ShellKeyBindings(payload) => handle_set_key_bindings(state, payload),
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
    let to_map: Vec<(smithay::desktop::Window, u32)> = entries
        .iter()
        .filter_map(|entry| {
            state
                .find_window_by_id(entry.window_id)
                .map(|w| (w, entry.window_id))
        })
        .collect();

    // Unmap all currently-mapped surfaces NOT in the composition list.
    // Skip X11 override-redirect windows (popups, menus, tooltips) — they
    // live outside the shell's composition and are mapped/unmapped directly
    // by the XwmHandler.
    let current: Vec<smithay::desktop::Window> = state.space.elements().cloned().collect();
    for window in &current {
        let is_or = window
            .x11_surface()
            .is_some_and(|s| s.is_override_redirect());
        if is_or {
            continue;
        }
        let dominated = to_map.iter().any(|(w, _)| w == window);
        if !dominated {
            state.space.unmap_elem(window);
            state.unmapped_surfaces.push(window.clone());
        }
    }

    // Map surfaces in list order (bottom to top).
    for (window, wid) in &to_map {
        state.unmapped_surfaces.retain(|w| w != window);

        let geo = state.frame_geometries.get(wid);

        if let Some(geo) = geo {
            // Shell has a frame for this window — use it.
            state
                .space
                .map_element(window.clone(), (geo.x, geo.y), false);
            configure_window(window, geo.x, geo.y, geo.width, geo.height);
        } else if let Some(x11) = window.x11_surface() {
            // Unframed X11 window — respect its self-requested geometry.
            let x11_geo = x11.geometry();
            state
                .space
                .map_element(window.clone(), (x11_geo.loc.x, x11_geo.loc.y), false);
        } else {
            // Unframed Wayland window — map at origin.
            state.space.map_element(window.clone(), (0, 0), false);
        }
    }

    // Reorder: raise elements in list order so the last entry is on top.
    for (window, _) in &to_map {
        state.space.raise_element(window, false);
    }
}

/// Apply a Frame update: configure the surface with the given size and position.
fn handle_frame(state: &mut State, update: sola_bus::topics::FrameUpdate) {
    // Store the geometry for future Composition mapping.
    state.frame_geometries.insert(update.window_id, update.clone());

    // If the surface exists, configure it now.
    if let Some(window) = state.find_window_by_id(update.window_id) {
        // If already in Space, reposition.
        let in_space = state
            .space
            .elements()
            .any(|w| w == &window);
        if in_space {
            state
                .space
                .map_element(window.clone(), (update.x, update.y), false);
        }

        configure_window(
            &window,
            update.x,
            update.y,
            update.width,
            update.height,
        );
    }
}

/// Configure a window's geometry — works for both Wayland toplevels
/// and X11 surfaces.
///
/// For Wayland toplevels, only size is sent (position is managed by
/// map_element in the Space). For X11 windows, both position and size
/// are sent so XWayland knows where the window is — this is critical
/// for correct override-redirect popup positioning.
fn configure_window(
    window: &smithay::desktop::Window,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) {
    if let Some(toplevel) = window.toplevel() {
        toplevel.with_pending_state(|s| {
            s.size = Some(Size::from((width, height)));
        });
        toplevel.send_pending_configure();
    } else if let Some(x11) = window.x11_surface() {
        let new_geo = smithay::utils::Rectangle::new(
            (x, y).into(),
            (width, height).into(),
        );
        if let Err(err) = x11.configure(Some(new_geo)) {
            tracing::warn!(?err, "failed to configure X11 window");
        }
    }
}

/// Apply a Focus target: set keyboard focus to the matching surface.
///
/// After focusing a non-shell surface, re-enters the shell keyboard
/// target. This ensures GTK dispatches shell key-binding events even
/// after keyboard focus has moved to another client.
fn handle_focus(state: &mut State, target: sola_bus::topics::FocusTarget) {
    let Some(window) = state.find_window_by_id(target.window_id) else {
        return;
    };

    let is_shell = State::app_id(&window).is_some_and(|id| id == "sola-shell");

    let serial = SERIAL_COUNTER.next_serial();
    let keyboard = state.seat.get_keyboard().unwrap();
    keyboard.set_focus(
        state,
        Some(crate::focus::FocusTarget::Window(window)),
        serial,
    );

    if !is_shell {
        if let Some(ref shell_surface) = state.shell_keyboard_target.clone() {
            setup_shell_keyboard_target(state, shell_surface);
        }
    }
}

/// Emit the current window list as a sticky bus message.
///
/// Called whenever surfaces appear or disappear. The shell uses this to
/// know which windows exist and compute composition.
pub(crate) fn emit_windows_list(state: &mut State) {
    use crate::state::window_id;
    use sola_bus::topics::{Topic, WindowInfo};

    let mut windows: Vec<WindowInfo> = Vec::new();

    let all_windows: Vec<_> = state
        .space
        .elements()
        .cloned()
        .chain(state.unmapped_surfaces.iter().cloned())
        .collect();

    for window in &all_windows {
        // Skip OR windows — they're unmanaged and not visible to the shell.
        if window
            .x11_surface()
            .is_some_and(|s| s.is_override_redirect())
        {
            continue;
        }

        let Some(wid) = window_id(window) else {
            continue;
        };
        let Some(app_id) = State::app_id(window) else {
            continue;
        };

        let title = state::window_title(window).unwrap_or_default();

        // Resolve X11 transient_for to a compositor window_id.
        let parent_window_id = window.x11_surface().and_then(|x11| {
            let parent_x11_id = x11.is_transient_for()?;
            // Find the compositor window whose X11 window ID matches.
            all_windows.iter().find_map(|w| {
                let x = w.x11_surface()?;
                if x.window_id() == parent_x11_id {
                    window_id(w)
                } else {
                    None
                }
            })
        });

        windows.push(WindowInfo {
            window_id: wid,
            app_id,
            title,
            parent_window_id,
        });
    }

    let _ = state.bus.emit_sticky(Topic::Windows(windows));
}

fn handle_set_window_policy(state: &mut State, payload: sola_bus::topics::WindowPolicyPayload) {
    tracing::info!(
        app_id = %payload.app_id,
        windows = payload.windows.len(),
        "registered window policy"
    );
    state
        .window_policies
        .insert(payload.app_id.clone(), payload.windows);
}

fn handle_set_key_bindings(state: &mut State, payload: sola_bus::topics::ShellKeyBindingsPayload) {
    tracing::info!(
        app_id = %payload.app_id,
        count = payload.bindings.len(),
        "registered shell key bindings"
    );

    state.shell_key_bindings = payload.bindings;
}

/// Move pending surfaces whose app_id is now known to unmapped_surfaces.
/// Emit updated Windows list when new surfaces are detected.
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

        // X11 windows need their wl_surface associated before they're ready.
        if window.x11_surface().is_some() && window.wl_surface().is_none() {
            still_pending.push(window);
            continue;
        }

        let title = state::window_title(&window);

        // Assign a stable window ID.
        let wid = state.assign_window_id(&window);
        tracing::info!(window_id = wid, app_id = %app_id, title = ?title, "surface ready, waiting for composition");

        // The sola-shell menubar is the Meta+key routing target.
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
            let _ = state
                .bus
                .emit_sticky(sola_bus::topics::Topic::SetWindowPolicy(
                    default_policy.clone(),
                ));
            state.window_policies.insert(app_id, default_policy.windows);
        }

        state.unmapped_surfaces.push(window);
        new_surfaces = true;
    }

    state.pending_surfaces = still_pending;

    if new_surfaces {
        emit_windows_list(state);
    }
}

/// Map pending override-redirect windows once their wl_surface has content.
///
/// OR windows bypass the shell's composition system entirely — they
/// position themselves via X11 geometry and are rendered above managed
/// windows. We defer mapping until the wl_surface is associated to
/// avoid rendering a bufferless surface (hangs NVIDIA EGL).
fn apply_pending_or_windows(state: &mut State) {
    use smithay::wayland::seat::WaylandFocus;

    let pending: Vec<_> = state.pending_or_windows.drain(..).collect();
    let mut still_pending = Vec::new();

    for window in pending {
        if window.wl_surface().is_none() {
            still_pending.push(window);
            continue;
        }

        let (x, y) = window
            .x11_surface()
            .map(|s| {
                let geo = s.geometry();
                (geo.loc.x, geo.loc.y)
            })
            .unwrap_or((0, 0));

        state.space.map_element(window, (x, y), false);
    }

    state.pending_or_windows = still_pending;
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
