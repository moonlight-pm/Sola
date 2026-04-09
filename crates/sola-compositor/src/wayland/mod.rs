/// Wayland protocol handlers.
///
/// Each submodule implements a specific Wayland protocol handler trait for `Sola`.
/// Smithay uses a "delegate" pattern: each protocol has a handler trait you
/// implement, plus a `delegate_*!` macro that wires up the Wayland message
/// dispatch to your handler. This is how Smithay avoids a monolithic event
/// callback — each protocol is handled independently.
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
///
/// These are small enough to live in this module directly rather than getting
/// their own files.
mod client {
    use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
    use smithay::reexports::wayland_server::protocol::wl_buffer;
    use smithay::wayland::buffer::BufferHandler;
    use smithay::wayland::compositor::CompositorClientState;

    use crate::state::Sola;

    /// Per-client data stored by the Wayland server.
    ///
    /// Every connected client gets an instance of this. Smithay requires it to
    /// implement `ClientData` (a wayland-server trait) so the server can
    /// manage client lifecycle. It also holds the `CompositorClientState` that
    /// Smithay needs for per-client surface bookkeeping.
    #[derive(Default)]
    pub struct ClientState {
        pub compositor_state: CompositorClientState,
    }

    /// `ClientData` is wayland-server's trait for per-client lifecycle hooks.
    /// It's called when a client connects or disconnects.
    impl ClientData for ClientState {
        fn initialized(&self, _client_id: ClientId) {}
        fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
    }

    /// Buffer lifecycle handler.
    ///
    /// `wl_buffer` objects represent pixel data that clients share with the
    /// compositor (via SHM, DMA-BUF, etc.). Smithay calls `buffer_destroyed`
    /// when a client destroys a buffer so we can clean up any references.
    impl BufferHandler for Sola {
        fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
    }
}

/// Helper to create a new `Arc<ClientState>` for incoming client connections.
///
/// Called from `lib.rs` when registering new clients on the Wayland display.
pub fn new_client_state() -> Arc<ClientState> {
    Arc::new(ClientState::default())
}
