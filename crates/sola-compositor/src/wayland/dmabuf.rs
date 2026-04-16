/// Linux DMA-BUF protocol handler.
///
/// `zwp_linux_dmabuf` lets clients share GPU buffers directly with the
/// compositor via DMA-BUF file descriptors — the zero-copy path for
/// GPU-accelerated clients.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/wayland/dmabuf/index.html
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::wayland::dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier};

use crate::state::State;

impl DmabufHandler for State {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        self.dmabuf_state
            .as_mut()
            .expect("dmabuf_state not initialized")
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        // Accept unconditionally. The actual EGL import happens at render
        // time where Smithay handles failures by skipping the surface.
        //
        // We deliberately avoid calling renderer.import_dmabuf() here
        // because a failed eglCreateImageKHR on this NVIDIA GPU corrupts
        // the EGL context's fence state, making ALL subsequent renders
        // fail with eglDupNativeFenceFDANDROID errors — freezing the
        // entire desktop. Deferring to render time avoids this because
        // Smithay's render path isolates import failures per-surface.
        dmabuf.set_node(self.primary_render_node);
        let _ = notifier.successful::<State>();
    }
}

smithay::delegate_dmabuf!(State);
