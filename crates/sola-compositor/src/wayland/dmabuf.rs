/// Linux DMA-BUF protocol handler.
///
/// `zwp_linux_dmabuf` lets clients share GPU buffers directly with the
/// compositor via DMA-BUF file descriptors — the zero-copy path for
/// GPU-accelerated clients.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/wayland/dmabuf/index.html
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::renderer::ImportDma;
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
        // Use the render node (not primary node) — matches what was
        // registered with GpuManager::add_node().
        let render_node = self.primary_render_node;

        match self.gpu_manager.single_renderer(&render_node) {
            Ok(mut renderer) => match renderer.import_dmabuf(&dmabuf, None) {
                Ok(_texture) => {
                    // Tell GpuManager which GPU owns this buffer.
                    dmabuf.set_node(render_node);
                    let _ = notifier.successful::<State>();
                }
                Err(err) => {
                    tracing::debug!(?err, "dmabuf import failed");
                    // Use failed(), NOT invalid_format() — invalid_format
                    // posts a protocol error that kills the client.
                    notifier.failed();
                }
            },
            Err(err) => {
                tracing::error!(?err, "failed to get renderer for dmabuf import");
                notifier.failed();
            }
        }
    }
}

smithay::delegate_dmabuf!(State);
