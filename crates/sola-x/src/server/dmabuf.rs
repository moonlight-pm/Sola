/// Dmabuf handler for sola-x's server side.
///
/// Accepts all dmabuf imports without GPU validation — we're just passing
/// the FDs through to sola-compositor which does the real import.
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::wayland::dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier};

use crate::state::State;

impl DmabufHandler for State {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        self.dmabuf_state.as_mut().expect("dmabuf not initialized")
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        _dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        // Always accept — sola-compositor validates when we forward.
        if let Err(err) = notifier.successful::<State>() {
            tracing::warn!(?err, "dmabuf import notification failed");
        }
    }
}

smithay::delegate_dmabuf!(State);
