/// Wayland compositor protocol handler.
///
/// The `wl_compositor` global lets clients create surfaces — rectangular
/// regions of pixels that form the building blocks of all visible content.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/wayland/compositor/trait.CompositorHandler.html
use smithay::backend::renderer::utils::on_commit_buffer_handler;
use smithay::reexports::wayland_server::Client;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::compositor::{CompositorClientState, CompositorHandler, CompositorState};

use crate::state::Sola;
use super::ClientState;

impl CompositorHandler for Sola {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    /// Called when a client commits new state to a surface.
    ///
    /// `on_commit_buffer_handler` processes the buffer attachment (extracts
    /// dimensions, scale, damage) and stores it in the surface's data map
    /// for the renderer to pick up later.
    ///
    /// We also notify any `Window` wrapping this surface so it can update
    /// its bounding box.
    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        tracing::info!("surface commit");

        if let Some(window) = self.space.elements().find(|w| {
            w.toplevel()
                .is_some_and(|t| t.wl_surface() == surface)
        }) {
            window.on_commit();
        }
    }
}

smithay::delegate_compositor!(Sola);
