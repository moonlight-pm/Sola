/// XWayland integration — runs X11 apps inside the Wayland compositor.
///
/// The lifecycle of an X11 window has two independent events:
/// 1. `map_window_request` — the X11 client wants the window visible
/// 2. `surface_associated` — XWayland pairs the X11 window with a wl_surface
///
/// These can happen in EITHER order. We only add the window to the Space
/// when BOTH have occurred — the window is mapped AND has a surface.
/// This prevents surfaceless windows from occupying space in the compositor.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/xwayland/index.html
use smithay::desktop::Window;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Rectangle, SERIAL_COUNTER};
use smithay::wayland::xwayland_shell::{XWaylandShellHandler, XWaylandShellState};
use smithay::xwayland::xwm::{Reorder, ResizeEdge, X11Window, XwmHandler, XwmId};
use smithay::xwayland::X11Surface;

use crate::error::CompositorError;
use crate::state::State;

/// Spawn XWayland and register its event source with the event loop.
///
/// XWayland connects as a Wayland client and provides an X11 display
/// for legacy apps (Steam, etc.). Pinned to `:0` for a stable `$DISPLAY`.
pub fn setup(state: &mut State, event_loop: &smithay::reexports::calloop::EventLoop<'static, State>) -> Result<(), CompositorError> {
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
    .map_err(|e| CompositorError::EventLoop(format!("XWayland spawn: {e}")))?;

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
        .map_err(|e| CompositorError::EventLoop(format!("XWayland source: {e}")))?;

    Ok(())
}

impl XwmHandler for State {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut smithay::xwayland::X11Wm {
        self.xwm.as_mut().expect("xwm not initialized")
    }

    fn new_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn new_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    /// X11 window wants to be visible. Allow it, and if the surface is
    /// already associated, add to Space now. Otherwise, `surface_associated`
    /// will add it later.
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

        // Track that this window wants to be mapped.
        self.xwayland_mapped.insert(window.window_id());

        // If surface is already associated, add to Space immediately.
        if window.wl_surface().is_some() {
            add_x11_to_space(self, window);
        }
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn unmapped_window(&mut self, _xwm: XwmId, window: X11Surface) {
        tracing::info!(title = %window.title(), "X11 window unmapped");
        self.xwayland_mapped.remove(&window.window_id());
        let id = window.window_id();
        let elem = self
            .space
            .elements()
            .find(|w| w.x11_surface().is_some_and(|s| s.window_id() == id))
            .cloned();
        if let Some(elem) = elem {
            self.space.unmap_elem(&elem);
        }
    }

    fn destroyed_window(&mut self, _xwm: XwmId, window: X11Surface) {
        self.xwayland_mapped.remove(&window.window_id());
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

    /// XWayland paired an X11 window with a Wayland surface.
    /// If the window already requested mapping, add to Space now.
    fn surface_associated(&mut self, _xwm: XwmId, _wl_surface: WlSurface, surface: X11Surface) {
        tracing::info!(
            title = %surface.title(),
            class = %surface.class(),
            "X11 surface associated"
        );

        if self.xwayland_mapped.contains(&surface.window_id()) {
            add_x11_to_space(self, surface);
        }
    }
}

/// Add an X11 window to the Space and give it keyboard focus.
/// Called when both mapping and surface association have occurred.
fn add_x11_to_space(state: &mut State, surface: X11Surface) {
    tracing::info!(
        title = %surface.title(),
        class = %surface.class(),
        "adding X11 window to space"
    );

    // Use the X11 window's requested geometry for positioning.
    let geo = surface.geometry();
    let wl_surface = surface.wl_surface();
    let window = Window::new_x11_window(surface);
    state.space.map_element(window, geo.loc, true);

    // Reset all DRM output buffers so the compositor has no cached frame
    // state. This forces a full re-render on the next frame, ensuring the
    // new window's content is picked up even if the damage tracker would
    // otherwise consider the frame unchanged.
    for device in state.devices.values() {
        for output in device.outputs.values() {
            output.reset_buffers();
        }
    }

    if let Some(surface) = wl_surface {
        let serial = SERIAL_COUNTER.next_serial();
        let keyboard = state.seat.get_keyboard().unwrap();
        keyboard.set_focus(state, Some(surface), serial);
    }
}

smithay::delegate_xwayland_shell!(State);
