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

/// Graceful shutdown. If a restart was requested, preserves the Wayland socket
/// FD across execv so clients can reconnect. Otherwise cleans up everything.
pub fn shutdown(
    mut sola: Sola,
    display: Display<Sola>,
    event_loop: EventLoop<'static, Sola>,
) {
    let should_restart = sola
        .restart_requested
        .load(std::sync::atomic::Ordering::Relaxed);

    tracing::info!("sola compositor shutting down");

    if should_restart {
        // Preserve the Wayland socket FD for the new process.
        // Clear FD_CLOEXEC so it survives execv.
        let socket_fd = sola.wayland_socket_fd;
        if let Some(fd) = socket_fd {
            clear_cloexec(fd);
            tracing::info!(fd, "preserved Wayland socket FD for restart");
        }

        // Drop XWayland and DRM devices (they'll be re-initialized).
        sola.xwm = None;
        sola.devices.clear();

        // Intentionally forget Display and EventLoop instead of dropping
        // them. Dropping Display closes client connections; dropping the
        // event loop drops the socket source (which would close the FD
        // we just preserved). The memory is replaced by execv anyway.
        std::mem::forget(display);
        std::mem::forget(event_loop);

        let _ = std::fs::remove_file("/tmp/.X0-lock");
        let _ = std::fs::remove_file("/tmp/.X11-unix/X0");
        crate::backend::watcher::exec_new_binary(socket_fd);
    } else {
        sola.xwm = None;
        sola.devices.clear();
        drop(display);
        drop(event_loop);
    }
}

/// Clear FD_CLOEXEC on a file descriptor so it survives execv.
fn clear_cloexec(fd: std::os::unix::io::RawFd) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC);
        }
    }
}
