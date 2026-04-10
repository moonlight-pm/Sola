/// Shared memory buffer handler for XWayland.
use smithay::wayland::shm::ShmHandler;

use crate::state::State;

impl ShmHandler for State {
    fn shm_state(&self) -> &smithay::wayland::shm::ShmState {
        &self.shm_state
    }
}

smithay::delegate_shm!(State);
