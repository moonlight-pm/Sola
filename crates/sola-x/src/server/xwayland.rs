/// XWayland integration — spawns and manages XWayland.
///
/// Moved from sola-compositor. The key difference: instead of adding X11
/// windows to a compositor Space, we track them in WindowBridge for
/// forwarding to sola as proxy Wayland surfaces.
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Rectangle};
use smithay::wayland::xwayland_shell::{XWaylandShellHandler, XWaylandShellState};
use smithay::xwayland::xwm::{Reorder, ResizeEdge, X11Window, XwmHandler, XwmId};
use smithay::xwayland::X11Surface;

use crate::state::{State, X11WindowInfo};

/// Spawn XWayland and register its event source with the event loop.
pub fn setup(
    state: &mut State,
    event_loop: &smithay::reexports::calloop::EventLoop<'static, State>,
) -> Result<(), crate::error::Error> {
    use smithay::wayland::xwayland_shell::XWaylandShellState;
    use smithay::xwayland::XWayland;

    state.xwayland_shell_state = Some(XWaylandShellState::new::<State>(&state.display_handle));

    let (xwayland, xwayland_client) = XWayland::spawn(
        &state.display_handle,
        Some(0),
        std::iter::empty::<(String, String)>(),
        true,
        std::process::Stdio::null(),
        std::process::Stdio::null(),
        |_| {},
    )
    .map_err(|e| crate::error::Error::XWayland(e.to_string()))?;

    event_loop
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
        .map_err(|e| crate::error::Error::EventLoop(e.to_string()))?;

    Ok(())
}

impl XwmHandler for State {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut smithay::xwayland::X11Wm {
        self.xwm.as_mut().expect("xwm not initialized")
    }

    fn new_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn new_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn map_window_request(&mut self, _xwm: XwmId, window: X11Surface) {
        tracing::info!(
            title = %window.title(),
            class = %window.class(),
            "X11 window map request"
        );

        if let Err(err) = window.set_mapped(true) {
            tracing::error!(?err, "failed to set X11 window mapped");
            return;
        }

        self.xwayland_mapped.insert(window.window_id());

        if window.wl_surface().is_some() {
            track_x11_window(self, window);
        }
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn unmapped_window(&mut self, _xwm: XwmId, window: X11Surface) {
        tracing::info!(title = %window.title(), "X11 window unmapped");
        let id = window.window_id();
        self.xwayland_mapped.remove(&id);
        self.x11_windows.remove(&id);
        if let Some(client) = &mut self.client {
            client.destroy_proxy(id);
        }
    }

    fn destroyed_window(&mut self, _xwm: XwmId, window: X11Surface) {
        let id = window.window_id();
        self.xwayland_mapped.remove(&id);
        self.x11_windows.remove(&id);
        if let Some(client) = &mut self.client {
            client.destroy_proxy(id);
        }
    }

    fn configure_request(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _reorder: Option<Reorder>,
    ) {
        let geo = window.geometry();
        let new_geo = Rectangle::new(
            (x.unwrap_or(geo.loc.x), y.unwrap_or(geo.loc.y)).into(),
            (
                w.unwrap_or(geo.size.w as u32) as i32,
                h.unwrap_or(geo.size.h as u32) as i32,
            )
                .into(),
        );
        if let Err(err) = window.configure(Some(new_geo)) {
            tracing::error!(?err, "failed to configure X11 window");
        }
    }

    fn configure_notify(
        &mut self,
        _xwm: XwmId,
        _window: X11Surface,
        _geometry: Rectangle<i32, Logical>,
        _above: Option<X11Window>,
    ) {
    }

    fn resize_request(
        &mut self,
        _xwm: XwmId,
        _window: X11Surface,
        _button: u32,
        _resize_edge: ResizeEdge,
    ) {
    }

    fn move_request(&mut self, _xwm: XwmId, _window: X11Surface, _button: u32) {}
}

impl XWaylandShellHandler for State {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        self.xwayland_shell_state
            .as_mut()
            .expect("xwayland_shell_state not initialized")
    }

    fn surface_associated(&mut self, _xwm: XwmId, wl_surface: WlSurface, surface: X11Surface) {
        tracing::info!(
            title = %surface.title(),
            class = %surface.class(),
            "X11 surface associated"
        );

        // Record the surface→X11 mapping for buffer forwarding in commit().
        self.surface_to_x11.insert(wl_surface, surface.window_id());

        if self.xwayland_mapped.contains(&surface.window_id()) {
            track_x11_window(self, surface);
        }
    }
}

/// Create a proxy surface in sola-compositor for an X11 window.
/// Called when both mapping and surface association have occurred.
fn track_x11_window(state: &mut State, surface: X11Surface) {
    let id = surface.window_id();
    let title = surface.title();
    let class = surface.class();

    tracing::info!(id, title = %title, class = %class, "tracking X11 window");

    state.x11_windows.insert(id, X11WindowInfo {
        title: title.clone(),
        class: class.clone(),
    });

    if let Some(client) = &mut state.client {
        client.create_proxy(id, &title, &class);
    }
}

smithay::delegate_xwayland_shell!(State);
