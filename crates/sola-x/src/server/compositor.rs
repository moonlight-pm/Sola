/// Wayland compositor protocol handler for XWayland's surfaces.
use smithay::reexports::wayland_server::Client;
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    self, CompositorClientState, CompositorHandler, CompositorState, SurfaceAttributes,
};

use crate::state::State;

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

impl CompositorHandler for State {
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

    fn commit(&mut self, surface: &WlSurface) {
        // Look up which X11 window this surface belongs to.
        let Some(&x11_id) = self.surface_to_x11.get(surface) else {
            return;
        };

        // Check whether this commit uses the async dmabuf path.
        let is_dmabuf = compositor::with_states(surface, |data| {
            let mut guard = data.cached_state.get::<SurfaceAttributes>();
            let attrs = guard.current();
            matches!(
                attrs.buffer,
                Some(ref a) if matches!(a, compositor::BufferAssignment::NewBuffer(buf) if smithay::wayland::dmabuf::get_dmabuf(buf).is_ok())
            )
        });

        // Forward the buffer to the proxy surface in sola-compositor.
        if let Some(client) = &mut self.client {
            crate::bridge::forward_buffer(surface, x11_id, client);
        }

        if is_dmabuf {
            // Async dmabuf: stash frame callbacks until the compositor
            // confirms import via `Created`. This prevents XWayland from
            // submitting frames faster than the compositor can import them.
            compositor::with_states(surface, |data| {
                let mut guard = data.cached_state.get::<SurfaceAttributes>();
                let attrs = guard.current();
                let callbacks: Vec<_> = attrs.frame_callbacks.drain(..).collect();
                if !callbacks.is_empty() {
                    self.pending_frame_callbacks
                        .entry(x11_id)
                        .or_default()
                        .extend(callbacks);
                }
            });
        } else {
            // SHM or buffer-removed: fire frame callbacks immediately
            // since the buffer is already forwarded synchronously.
            compositor::with_states(surface, |data| {
                let mut guard = data.cached_state.get::<SurfaceAttributes>();
                let attrs = guard.current();
                let time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u32;
                for callback in attrs.frame_callbacks.drain(..) {
                    callback.done(time);
                }
            });
        }
    }
}

impl BufferHandler for State {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

smithay::delegate_compositor!(State);
