//! The lock on the bus: `Topic::ScreenLock` (sticky) once the compositor
//! confirms the lock, and `locked: false` once this process unlocks it.
//! Never on a failure or an exit while locked: the compositor keeps the
//! screen locked until another `sola-lock` takes over, so `locked`
//! stands. `sola-vaultd` locks the vault on it.
//!
//! The bus may be away or restart while the screen is locked: the client
//! queues what it could not send, and [`Announce::keep_up`] connects again
//! (sending the queue, and the sticky lock after a restart).

use sola_bus::BusClient;
use sola_bus::topics::{ScreenLock, Topic};
use tracing::{info, warn};

const APP_ID: &str = "sola-lock";

pub struct Announce {
    bus: BusClient,
}

impl Announce {
    pub fn new() -> Self {
        let mut bus = BusClient::new();
        bus.set_app_id(APP_ID);
        if let Err(e) = bus.connect() {
            info!("bus not up ({e}); the lock is told once it is");
        }
        Self { bus }
    }

    /// The screen is locked, or this process unlocked it.
    pub fn locked(&mut self, locked: bool) {
        self.keep_up();
        match self.bus.emit(Topic::ScreenLock(ScreenLock { locked })) {
            Ok(()) => info!(locked, "bus: ScreenLock"),
            Err(e) => warn!(locked, "bus: ScreenLock not sent: {e}"),
        }
    }

    /// Connect again when the bus was away or restarted.
    pub fn keep_up(&mut self) {
        if !self.bus.is_connected() {
            let _ = self.bus.connect();
        }
    }
}
