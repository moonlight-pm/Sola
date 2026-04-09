/// Wayland compositor protocol handler.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/wayland/compositor/trait.CompositorHandler.html
use smithay::backend::renderer::utils::on_commit_buffer_handler;
use smithay::reexports::wayland_server::Client;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::compositor::{CompositorClientState, CompositorHandler, CompositorState};

use crate::state::Sola;
use super::ClientState;

// Fallback CompositorClientState for clients not created through our
// socket listener (e.g., XWayland's internal client). XWayland connects
// with its own ClientData type, so `get_data::<ClientState>()` returns
// None. This thread-local provides a stable reference for those clients.
thread_local! {
    static XWAYLAND_CLIENT_STATE: CompositorClientState = CompositorClientState::default();
}

impl CompositorHandler for Sola {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        if let Some(state) = client.get_data::<ClientState>() {
            &state.compositor_state
        } else {
            // XWayland or other internally-created clients.
            // Safety: thread_local with 'static lifetime, returned reference
            // is valid for 'a since the thread_local outlives the client.
            XWAYLAND_CLIENT_STATE.with(|s| unsafe {
                &*(s as *const CompositorClientState)
            })
        }
    }

    fn commit(&mut self, surface: &WlSurface) {
        use smithay::wayland::seat::WaylandFocus;

        on_commit_buffer_handler::<Self>(surface);

        // Update geometry for the window that owns this surface.
        // Uses WaylandFocus::wl_surface() which works for both Wayland
        // toplevels and X11 windows.
        if let Some(window) = self.space.elements().find(|w| {
            w.wl_surface()
                .is_some_and(|s| s.as_ref() == surface)
        }) {
            window.on_commit();
        }
    }
}

smithay::delegate_compositor!(Sola);
