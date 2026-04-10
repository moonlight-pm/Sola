/// Wayland compositor protocol handler.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/wayland/compositor/trait.CompositorHandler.html
use smithay::backend::renderer::utils::on_commit_buffer_handler;
use smithay::reexports::wayland_server::Client;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::compositor::{CompositorClientState, CompositorHandler, CompositorState};

use crate::state::State;
use super::ClientState;

// Fallback CompositorClientState for clients not created through our
// socket listener (e.g., XWayland's internal client).
thread_local! {
    static XWAYLAND_CLIENT_STATE: CompositorClientState = CompositorClientState::default();
}

impl CompositorHandler for State {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        if let Some(state) = client.get_data::<ClientState>() {
            &state.compositor_state
        } else {
            XWAYLAND_CLIENT_STATE.with(|s| unsafe {
                &*(s as *const CompositorClientState)
            })
        }
    }

    /// Called when a client commits new state to a surface.
    fn commit(&mut self, surface: &WlSurface) {
        use smithay::wayland::seat::WaylandFocus;

        // Process buffer state (dimensions, damage, etc.).
        on_commit_buffer_handler::<Self>(surface);

        // Import the buffer into the primary GPU's renderer early.
        // See: https://docs.rs/smithay/latest/smithay/backend/renderer/multigpu/struct.GpuManager.html#method.early_import
        if let Err(err) = self.gpu_manager.early_import(self.primary_gpu, surface) {
            tracing::warn!(?err, "early_import failed");
        }

        // Update geometry for the window that owns this surface.
        if let Some(window) = self.space.elements().find(|w| {
            w.wl_surface()
                .is_some_and(|s| s.as_ref() == surface)
        }) {
            window.on_commit();
        }
    }
}

smithay::delegate_compositor!(State);
