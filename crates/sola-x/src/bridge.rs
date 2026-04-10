/// Buffer bridge — forwards buffers from XWayland to sola-compositor.
///
/// When XWayland commits a surface, we extract the buffer (dmabuf or shm)
/// and re-attach the same underlying data to the proxy surface in
/// sola-compositor. For dmabuf this is zero-copy (same GPU memory via FD
/// passing). For shm, the fd is shared directly.
use std::os::fd::AsFd;

use smithay::backend::allocator::Buffer;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface as ServerWlSurface;
use smithay::wayland::compositor::{self, SurfaceAttributes, BufferAssignment};
use smithay::wayland::dmabuf::get_dmabuf;

use crate::client::ClientConnection;

/// Check the committed surface for a new buffer and forward it to
/// the corresponding proxy surface in sola-compositor.
pub fn forward_buffer(
    server_surface: &ServerWlSurface,
    x11_window_id: u32,
    client: &mut ClientConnection,
) {
    compositor::with_states(server_surface, |data| {
        let mut guard = data.cached_state.get::<SurfaceAttributes>();
        let attrs = guard.current();
        let Some(ref assignment) = attrs.buffer else {
            return;
        };

        match assignment {
            BufferAssignment::Removed => {
                // Buffer detached — detach from proxy too.
                if let Some(proxy) = client.app.proxies.get(&x11_window_id) {
                    proxy.surface.attach(None, 0, 0);
                    proxy.surface.commit();
                }
            }
            BufferAssignment::NewBuffer(wl_buffer) => {
                // Try dmabuf first (GPU path, zero-copy).
                if let Ok(dmabuf) = get_dmabuf(wl_buffer) {
                    forward_dmabuf(dmabuf, x11_window_id, client);
                    return;
                }

                // TODO: shm fallback path.
                tracing::debug!("non-dmabuf buffer on X11 window {x11_window_id}, skipping");
            }
        }
    });
}

/// Forward a dmabuf buffer to the proxy surface via linux-dmabuf protocol.
fn forward_dmabuf(
    dmabuf: &smithay::backend::allocator::dmabuf::Dmabuf,
    x11_window_id: u32,
    client: &mut ClientConnection,
) {
    let proxy = match client.app.proxies.get(&x11_window_id) {
        Some(p) => p,
        None => return,
    };

    let Some(ref dmabuf_manager) = client.app.dmabuf else {
        tracing::warn!("compositor doesn't support linux-dmabuf, can't forward buffer");
        return;
    };

    let format = dmabuf.format();
    let size = dmabuf.size();

    // Create buffer params and add each plane.
    let params = dmabuf_manager.create_params(&client.qh, ());

    for (i, ((fd, offset), stride)) in dmabuf
        .handles()
        .zip(dmabuf.offsets())
        .zip(dmabuf.strides())
        .enumerate()
    {
        // dup the fd — the client-side protocol takes ownership.
        let dup_fd = fd.try_clone_to_owned().unwrap();
        params.add(
            dup_fd.as_fd(),
            i as u32,
            offset,
            stride,
            (u64::from(format.modifier) >> 32) as u32,
            u64::from(format.modifier) as u32,
        );
    }

    // Create the buffer immediately (no roundtrip needed).
    use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1;
    let buffer = params.create_immed(
        size.w,
        size.h,
        format.code as u32,
        zwp_linux_buffer_params_v1::Flags::empty(),
        &client.qh,
        (),
    );

    proxy.surface.attach(Some(&buffer), 0, 0);
    proxy.surface.damage_buffer(0, 0, size.w, size.h);
    proxy.surface.commit();
}
