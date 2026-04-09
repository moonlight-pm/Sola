/// Wayland compositor protocol handler.
///
/// The `wl_compositor` global is the core of the Wayland protocol. It lets
/// clients create surfaces — the fundamental building blocks that represent
/// rectangular regions of pixels on screen. Every visible window, popup, or
/// overlay starts as a `wl_surface`.
///
/// Smithay requires us to implement `CompositorHandler` so it knows how to
/// route surface lifecycle events to our compositor.
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
    /// Smithay stores bookkeeping per client (e.g., which surfaces belong to
    /// them). We stash a `ClientState` containing `CompositorClientState` in
    /// the client's user data when they connect — see `ClientState` in
    /// `wayland/mod.rs`.
    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    /// Called when a client commits new state to a surface.
    ///
    /// A "commit" in Wayland means the client has finished preparing a frame
    /// and wants the compositor to display it. In Phase 1 we have no clients,
    /// so this is a no-op.
    fn commit(&mut self, _surface: &WlSurface) {}
}

smithay::delegate_compositor!(Sola);
