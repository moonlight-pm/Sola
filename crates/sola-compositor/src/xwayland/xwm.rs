//! X11 window manager handler — maps X11 windows into the compositor's Space.
use smithay::desktop::Window;
use smithay::utils::{Logical, Rectangle};
use smithay::wayland::xwayland_shell::{XWaylandShellHandler, XWaylandShellState};
use smithay::xwayland::xwm::{Reorder, ResizeEdge, X11Window, XwmHandler, XwmId};
use smithay::xwayland::X11Surface;

use crate::lifecycle::emit_windows_list;
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

        // OR windows (menus, popups, tooltips) bypass the shell's
        // composition system. They position and size themselves.
        // Defer mapping until the wl_surface has content — mapping
        // a bufferless surface hangs NVIDIA's EGL.
        let win = Window::new_x11_window(window);
        self.pending_or_windows.push(win);
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
        window: X11Surface,
        geometry: Rectangle<i32, Logical>,
        _above: Option<X11Window>,
    ) {
        if window.is_override_redirect() {
            let found = self.space.elements().find(|w| {
                w.x11_surface()
                    .is_some_and(|s| s.window_id() == window.window_id())
            }).cloned();

            if let Some(win) = found {
                self.space
                    .map_element(win, (geometry.loc.x, geometry.loc.y), false);
            }
        }
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
        // Remove from window_ids map.
        if let Some(wid) = crate::state::window_id(&window) {
            state.window_ids.remove(&wid);
        }
        state.space.unmap_elem(&window);
    }

    // Remove from pending OR windows.
    state.pending_or_windows.retain(|w| {
        !w.x11_surface().is_some_and(|s| s.window_id() == surface.window_id())
    });

    // Remove from pending/unmapped, also cleaning window_ids.
    state.pending_surfaces.retain(|w| {
        let remove = w.x11_surface().is_some_and(|s| s.window_id() == surface.window_id());
        if remove {
            if let Some(wid) = crate::state::window_id(w) {
                state.window_ids.remove(&wid);
            }
        }
        !remove
    });
    state.unmapped_surfaces.retain(|w| {
        let remove = w.x11_surface().is_some_and(|s| s.window_id() == surface.window_id());
        if remove {
            if let Some(wid) = crate::state::window_id(w) {
                state.window_ids.remove(&wid);
            }
        }
        !remove
    });

    emit_windows_list(state);
}

smithay::delegate_xwayland_shell!(State);
