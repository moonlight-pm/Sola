/// Wayland shared memory (SHM) protocol handler.
///
/// `wl_shm` lets clients share pixel buffers with the compositor via shared
/// memory. This is the simplest buffer-passing mechanism in Wayland — the
/// client writes pixels into a shared memory region, and the compositor reads
/// them. It's CPU-based (no GPU involvement) and is used as a fallback when
/// hardware acceleration isn't available for a particular client.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/wayland/shm/trait.ShmHandler.html
use smithay::wayland::shm::{ShmHandler, ShmState};

use crate::state::State;

impl ShmHandler for State {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

smithay::delegate_shm!(State);
