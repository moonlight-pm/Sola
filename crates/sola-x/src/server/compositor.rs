/// Wayland compositor protocol handler for XWayland's surfaces.
use smithay::reexports::wayland_server::Client;
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{CompositorClientState, CompositorHandler, CompositorState};

use crate::state::SolaX;

// Thread-local fallback for XWayland's internal client which doesn't
// go through our socket listener (so it has no ClientState).
thread_local! {
    static FALLBACK_STATE: CompositorClientState = CompositorClientState::default();
}

/// Per-client data for the XWayland Wayland connection.
#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {
        tracing::warn!("XWayland Wayland client disconnected");
    }
}

impl CompositorHandler for SolaX {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        if let Some(state) = client.get_data::<ClientState>() {
            &state.compositor_state
        } else {
            FALLBACK_STATE.with(|s| unsafe { &*(s as *const CompositorClientState) })
        }
    }

    fn commit(&mut self, _surface: &WlSurface) {
        // TODO: Phase 3 — forward buffer to sola via the bridge.
    }
}

impl BufferHandler for SolaX {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

smithay::delegate_compositor!(SolaX);
