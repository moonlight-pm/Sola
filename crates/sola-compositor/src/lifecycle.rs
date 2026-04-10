/// Main event loop and shutdown logic.
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use smithay::utils::SERIAL_COUNTER;

use crate::error::CompositorError;
use crate::output::render;
use crate::state::State;

/// Run the main event loop until `state.running` becomes false.
pub fn run_loop(
    state: &mut State,
    display: &mut Display<State>,
    event_loop: &mut EventLoop<'static, State>,
) -> Result<(), CompositorError> {
    tracing::info!("entering event loop");

    while state.running {
        process_bus(state);

        state.space.refresh();

        display
            .dispatch_clients(state)
            .map_err(|e| CompositorError::Display(e.to_string()))?;
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

/// Process any pending bus messages.
fn process_bus(state: &mut State) {
    use sola_bus::topics::Topic;

    // Collect messages first to release the borrow on state.bus.
    let Some(bus) = &state.bus else { return };
    let mut messages = Vec::new();
    while let Some(msg) = bus.try_recv() {
        messages.push(msg);
    }

    for msg in &messages {
        let Some(topic) = Topic::parse(msg) else {
            tracing::debug!(topic = %msg.topic, "unknown bus topic");
            continue;
        };

        match topic {
            Topic::GrabInput(target) => handle_grab_input(state, &target),
            Topic::ReleaseInput => handle_release_input(state),
            Topic::RaiseApp(app_id) => handle_raise_app(state, &app_id),
            _ => {
                tracing::debug!(topic = %msg.topic, "unhandled bus topic");
            }
        }
    }
}

/// Show the target app's surface above everything and give it exclusive input.
fn handle_grab_input(state: &mut State, target: &str) {
    let Some(window) = state.window_by_app_id(target) else {
        tracing::warn!(target, "GrabInput: no window found");
        return;
    };

    tracing::info!(target, "grabbing input");

    // Raise to top of z-order.
    state.space.raise_element(&window, true);

    // Give keyboard focus to the grabbed surface.
    let serial = SERIAL_COUNTER.next_serial();
    if let Some(toplevel) = window.toplevel() {
        let keyboard = state.seat.get_keyboard().unwrap();
        keyboard.set_focus(state, Some(toplevel.wl_surface().clone()), serial);
    }

    state.input_grab = Some(target.to_string());
}

/// Release the input grab and restore normal focus.
fn handle_release_input(state: &mut State) {
    let Some(target) = state.input_grab.take() else {
        return;
    };

    tracing::info!(target = %target, "releasing input");

    // TODO: hide the grabbed surface (skip in render pass)
    // TODO: restore focus to the previously focused window
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
    if let Some(window) = windows.last() {
        if let Some(toplevel) = window.toplevel() {
            let serial = SERIAL_COUNTER.next_serial();
            let keyboard = state.seat.get_keyboard().unwrap();
            keyboard.set_focus(state, Some(toplevel.wl_surface().clone()), serial);
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
