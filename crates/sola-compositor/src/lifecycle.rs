/// Main event loop and shutdown/restart logic.
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;

use crate::error::CompositorError;
use crate::output::render;
use crate::state::Sola;

/// Run the main event loop until `sola.running` becomes false.
pub fn run_loop(
    sola: &mut Sola,
    display: &mut Display<Sola>,
    event_loop: &mut EventLoop<'static, Sola>,
) -> Result<(), CompositorError> {
    tracing::info!("entering event loop");

    while sola.running {
        if sola
            .restart_requested
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            tracing::info!("restart requested by binary watcher");
            break;
        }

        sola.space.refresh();

        display
            .dispatch_clients(sola)
            .map_err(|e| CompositorError::Display(e.to_string()))?;
        display
            .flush_clients()
            .map_err(|e| CompositorError::Display(e.to_string()))?;

        render::render_all(sola);

        event_loop
            .dispatch(Some(std::time::Duration::from_millis(16)), sola)
            .map_err(|e| CompositorError::EventLoop(e.to_string()))?;
    }

    Ok(())
}

/// Graceful shutdown. Drops XWayland, DRM devices, display, event loop.
/// If a restart was requested, cleans up X11 lock files and exec's the
/// new binary (does not return).
pub fn shutdown(
    mut sola: Sola,
    display: Display<Sola>,
    event_loop: EventLoop<'static, Sola>,
) {
    let should_restart = sola
        .restart_requested
        .load(std::sync::atomic::Ordering::Relaxed);

    tracing::info!("sola compositor shutting down");

    sola.xwm = None;
    sola.devices.clear();
    drop(display);
    drop(event_loop);

    if should_restart {
        let _ = std::fs::remove_file("/tmp/.X0-lock");
        let _ = std::fs::remove_file("/tmp/.X11-unix/X0");
        crate::backend::watcher::exec_new_binary();
    }
}
