//! XWayland integration — spawns XWayland and handles X11 windows.
//!
//! X11 windows are wrapped as `Window::new_x11_window()` and enter
//! the compositor's standard `pending_surfaces` -> `Space` pipeline.

pub mod xwm;

use smithay::reexports::calloop::EventLoop;
use smithay::wayland::xwayland_shell::XWaylandShellState;
use smithay::xwayland::XWayland;

use crate::state::State;

/// Spawn XWayland and register its event source with the event loop.
pub fn setup(
    state: &mut State,
    event_loop: &EventLoop<'static, State>,
) {
    state.xwayland_shell_state = Some(XWaylandShellState::new::<State>(&state.display_handle));

    let (xwayland, xwayland_client) = match XWayland::spawn(
        &state.display_handle,
        None,
        std::iter::empty::<(String, String)>(),
        true,
        std::process::Stdio::null(),
        std::process::Stdio::null(),
        |_| {},
    ) {
        Ok(pair) => pair,
        Err(e) => {
            tracing::error!("failed to spawn XWayland: {e}");
            return;
        }
    };

    if let Err(e) = event_loop
        .handle()
        .insert_source(xwayland, move |event, _, state| match event {
            smithay::xwayland::XWaylandEvent::Ready {
                x11_socket,
                display_number,
            } => {
                tracing::info!(display_number, "XWayland ready");
                unsafe { std::env::set_var("DISPLAY", format!(":{display_number}")) };

                match smithay::xwayland::X11Wm::start_wm(
                    state.loop_handle.clone(),
                    x11_socket,
                    xwayland_client.clone(),
                ) {
                    Ok(wm) => {
                        state.xwm = Some(wm);
                        tracing::info!("X11 window manager started");
                    }
                    Err(err) => {
                        tracing::error!(?err, "failed to start X11 window manager");
                    }
                }
            }
            smithay::xwayland::XWaylandEvent::Error => {
                tracing::error!("XWayland failed to start");
            }
        })
    {
        tracing::error!("failed to register XWayland event source: {e}");
    }
}
