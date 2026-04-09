/// XWayland integration — runs X11 apps inside the Wayland compositor.
///
/// XWayland is an X11 server that runs as a Wayland client. X11 apps
/// (like Steam) connect to it, and it translates their X11 windows into
/// Wayland surfaces that our compositor can manage.
///
/// The lifecycle of an X11 window:
/// 1. `new_window` — X11 window created (no surface yet)
/// 2. `map_window_request` — window wants to be visible, we call `set_mapped(true)`
/// 3. `surface_associated` — XWayland pairs the X11 window with a wl_surface
///    (THIS is when we add it to the Space, because only now does it have
///    renderable content)
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

    /// An X11 window wants to be shown. We allow it but don't add to Space
    /// yet — the wl_surface may not exist. We add it in `surface_associated`.
    fn map_window_request(&mut self, _xwm: XwmId, window: X11Surface) {
        tracing::info!(
            title = %window.title(),
            class = %window.class(),
            "X11 window map request"
        );

        if let Err(err) = window.set_mapped(true) {
            tracing::error!(?err, "failed to set X11 window mapped");
        }
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn unmapped_window(&mut self, _xwm: XwmId, window: X11Surface) {
        tracing::info!(title = %window.title(), "X11 window unmapped");
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

    fn destroyed_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

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

    /// Called when XWayland pairs an X11 window with a Wayland surface.
    /// NOW the window has renderable content, so we add it to the Space.
    fn surface_associated(&mut self, _xwm: XwmId, wl_surface: WlSurface, surface: X11Surface) {
        tracing::info!(
            title = %surface.title(),
            class = %surface.class(),
            "X11 surface associated with wl_surface"
        );

        let window = Window::new_x11_window(surface);
        self.space.map_element(window, (0, 0), true);

        // Give keyboard focus to the new window.
        let serial = SERIAL_COUNTER.next_serial();
        let keyboard = self.seat.get_keyboard().unwrap();
        keyboard.set_focus(self, Some(wl_surface), serial);
    }
}

smithay::delegate_xwayland_shell!(Sola);
