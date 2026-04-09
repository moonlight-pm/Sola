/// Wayland protocol handlers.
///
/// Each submodule implements a Wayland protocol handler trait for `Sola`.
/// Smithay uses a "delegate" pattern: each protocol has a handler trait
/// plus a `delegate_*!` macro that wires up message dispatch.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/index.html#protocols
mod compositor;
mod data;
mod output;
mod seat;
mod shell;
mod shm;

use std::sync::Arc;

pub use client::ClientState;

/// Per-client state and buffer handling.
mod client {
    use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
    use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
    use smithay::wayland::buffer::BufferHandler;
    use smithay::wayland::compositor::CompositorClientState;

    use crate::state::Sola;

    /// Per-client data stored by the Wayland server.
    ///
    /// Every connected client gets an instance. Holds `CompositorClientState`
    /// for per-client surface bookkeeping.
    #[derive(Default)]
    pub struct ClientState {
        pub compositor_state: CompositorClientState,
    }

    /// Lifecycle hooks for client connect/disconnect.
    impl ClientData for ClientState {
        fn initialized(&self, _client_id: ClientId) {}
        fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
    }

    /// Buffer lifecycle — called when a client destroys a pixel buffer.
    impl BufferHandler for Sola {
        fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
    }
}

/// Create a new `Arc<ClientState>` for incoming client connections.
pub fn new_client_state() -> Arc<ClientState> {
    Arc::new(ClientState::default())
}
