/// Main event loop and shutdown logic.
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;

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

/// Graceful shutdown — clean up all resources.
pub fn shutdown(mut state: State, display: Display<State>, event_loop: EventLoop<'static, State>) {
    tracing::info!("sola compositor shutting down");
    state.xwm = None;
    state.devices.clear();
    drop(display);
    drop(event_loop);
}
