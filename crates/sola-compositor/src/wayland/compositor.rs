/// Wayland compositor protocol handler.
///
/// The `wl_compositor` global is the core of the Wayland protocol. It lets
/// clients create surfaces — the fundamental building blocks that represent
/// rectangular regions of pixels on screen.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/wayland/compositor/trait.CompositorHandler.html
use smithay::reexports::wayland_server::Client;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::compositor::{CompositorClientState, CompositorHandler, CompositorState};

use crate::state::Sola;
use super::ClientState;

impl CompositorHandler for Sola {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    /// Return per-client compositor state.
    ///
    /// `ClientState` is stored in each client's user data when they connect
    /// — see `wayland/mod.rs`.
    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    /// Called when a client commits new state to a surface.
    fn commit(&mut self, _surface: &WlSurface) {}
}

smithay::delegate_compositor!(Sola);
