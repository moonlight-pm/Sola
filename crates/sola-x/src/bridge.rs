/// Buffer bridge — forwards buffers from XWayland to sola-compositor.
///
/// When XWayland commits a surface, we extract the buffer (dmabuf or shm)
/// and re-attach the same underlying data to the proxy surface in
/// sola-compositor. For dmabuf this is zero-copy (same GPU memory via FD
/// passing). For shm, the fd is shared directly.
use std::os::fd::AsFd;

use std::os::unix::io::{AsRawFd, FromRawFd};

use smithay::backend::allocator::Buffer;
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface as ServerWlSurface;
use smithay::wayland::compositor::{self, SurfaceAttributes, BufferAssignment};
use smithay::wayland::dmabuf::get_dmabuf;
use smithay::wayland::shm::with_buffer_contents;

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

                // SHM fallback — copy pixel data to a client-side SHM buffer.
                forward_shm(wl_buffer, x11_window_id, client);
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

    let modifier_raw = u64::from(format.modifier);

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
            (modifier_raw >> 32) as u32,
            modifier_raw as u32,
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

/// Forward an SHM buffer by copying pixel data to a client-side SHM pool.
fn forward_shm(
    wl_buffer: &WlBuffer,
    x11_window_id: u32,
    client: &mut ClientConnection,
) {
    let proxy = match client.app.proxies.get(&x11_window_id) {
        Some(p) => p,
        None => return,
    };

    let Some(ref shm) = client.app.shm else {
        return;
    };

    // Read the buffer data from the server side.
    let result = with_buffer_contents(wl_buffer, |ptr, _pool_len, data| {
        let buf_size = (data.stride * data.height) as usize;

        // Create an anonymous shared memory file for the client-side pool.
        let file = create_shm_file(buf_size);
        let pool = shm.create_pool(file.as_fd(), buf_size as i32, &client.qh, ());

        // Convert server-side wl_shm::Format (wayland-server) to client-side
        // (wayland-client). Same wire values, different Rust types.
        use wayland_client::protocol::wl_shm as client_shm;
        let format_raw: u32 = data.format.into();
        // SAFETY: wl_shm::Format is #[repr(u32)] in both crates with identical values.
        let format: client_shm::Format = unsafe { std::mem::transmute(format_raw) };

        let buffer = pool.create_buffer(
            0,
            data.width,
            data.height,
            data.stride,
            format,
            &client.qh,
            (),
        );

        // Copy the pixel data from the server-side pool to our file.
        unsafe {
            let dst = libc::mmap(
                std::ptr::null_mut(),
                buf_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_fd().as_raw_fd(),
                0,
            );
            if dst != libc::MAP_FAILED {
                let src = ptr.add(data.offset as usize);
                std::ptr::copy_nonoverlapping(src, dst as *mut u8, buf_size);
                libc::munmap(dst, buf_size);
            }
        }

        (buffer, data.width, data.height)
    });

    match result {
        Ok((buffer, w, h)) => {
            proxy.surface.attach(Some(&buffer), 0, 0);
            proxy.surface.damage_buffer(0, 0, w, h);
            proxy.surface.commit();
        }
        Err(e) => {
            tracing::warn!(?e, x11_window_id, "failed to read SHM buffer");
        }
    }
}

/// Create an anonymous shared memory file.
fn create_shm_file(size: usize) -> std::fs::File {
    use std::ffi::CString;
    let name = CString::new("/sola-x-shm").unwrap();
    unsafe {
        libc::shm_unlink(name.as_ptr());
        let fd = libc::shm_open(
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            0o600,
        );
        assert!(fd >= 0, "shm_open failed");
        libc::shm_unlink(name.as_ptr());
        libc::ftruncate(fd, size as libc::off_t);
        std::fs::File::from_raw_fd(fd)
    }
}
