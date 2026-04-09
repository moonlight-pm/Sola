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

use crate::state::Sola;

impl XwmHandler for Sola {
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

impl XWaylandShellHandler for Sola {
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
fn add_x11_to_space(sola: &mut Sola, surface: X11Surface) {
    tracing::info!(
        title = %surface.title(),
        class = %surface.class(),
        "adding X11 window to space"
    );

    let wl_surface = surface.wl_surface();
    let window = Window::new_x11_window(surface);
    sola.space.map_element(window, (0, 0), true);

    // Reset all DRM output buffers so the compositor has no cached frame
    // state. This forces a full re-render on the next frame, ensuring the
    // new window's content is picked up even if the damage tracker would
    // otherwise consider the frame unchanged.
    for device in sola.devices.values() {
        for output in device.outputs.values() {
            output.reset_buffers();
        }
    }

    if let Some(surface) = wl_surface {
        let serial = SERIAL_COUNTER.next_serial();
        let keyboard = sola.seat.get_keyboard().unwrap();
        keyboard.set_focus(sola, Some(surface), serial);
    }
}

smithay::delegate_xwayland_shell!(Sola);
