//! X11 window manager handler — maps X11 windows into the compositor's Space.
use smithay::desktop::Window;
use smithay::utils::{Logical, Rectangle};
use smithay::wayland::xwayland_shell::{XWaylandShellHandler, XWaylandShellState};
use smithay::xwayland::xwm::{Reorder, ResizeEdge, X11Window, XwmHandler, XwmId};
use smithay::xwayland::X11Surface;

use crate::lifecycle::emit_apps_list;
use crate::state::State;

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

        // X11 window enters the standard pending_surfaces flow.
        // It will be picked up by apply_pending_surfaces once the
        // wl_surface is associated (surface_associated callback).
        let win = Window::new_x11_window(window);
        self.pending_surfaces.push(win);
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, window: X11Surface) {
        tracing::info!(
            title = %window.title(),
            class = %window.class(),
            "X11 override-redirect window mapped"
        );

        // OR windows (menus, popups, tooltips) are mapped directly
        // into the space at their X11-requested position, above
        // everything else.
        let geo = window.geometry();
        let win = Window::new_x11_window(window);
        self.space.map_element(win, (geo.loc.x, geo.loc.y), true);
    }

    fn unmapped_window(&mut self, _xwm: XwmId, window: X11Surface) {
        tracing::info!(title = %window.title(), "X11 window unmapped");
        remove_x11_window(self, &window);
    }

    fn destroyed_window(&mut self, _xwm: XwmId, window: X11Surface) {
        remove_x11_window(self, &window);
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
                w.map(|v| v as i32).unwrap_or(geo.size.w),
                h.map(|v| v as i32).unwrap_or(geo.size.h),
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

    fn surface_associated(
        &mut self,
        _xwm: XwmId,
        _wl_surface: smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
        surface: X11Surface,
    ) {
        tracing::info!(
            title = %surface.title(),
            class = %surface.class(),
            "X11 surface associated"
        );
    }
}

/// Remove an X11 window from all tracked collections.
fn remove_x11_window(state: &mut State, surface: &X11Surface) {
    // Remove from space (mapped windows).
    let found = state.space.elements().find(|w| {
        w.x11_surface().is_some_and(|s| s.window_id() == surface.window_id())
    }).cloned();

    if let Some(window) = found {
        state.space.unmap_elem(&window);
    }

    // Remove from pending/unmapped.
    state.pending_surfaces.retain(|w| {
        !w.x11_surface().is_some_and(|s| s.window_id() == surface.window_id())
    });
    state.unmapped_surfaces.retain(|w| {
        !w.x11_surface().is_some_and(|s| s.window_id() == surface.window_id())
    });

    emit_apps_list(state);
}

smithay::delegate_xwayland_shell!(State);
