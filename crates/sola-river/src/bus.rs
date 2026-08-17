//! Thin wrapper around `sola_bus::BusClient` that:
//!   - Sets our app_id.
//!   - Exposes `ensure_connected` and `subscribe` for connection setup.
//!   - Re-subscribes automatically after a bus restart (sticky state on the
//!     bus is wiped when sola-bus restarts; we must re-attach our interest).
//!   - Exposes `try_recv` and `emit*` for the translator.
use std::os::unix::io::RawFd;

use sola_bus::Message;
use sola_bus::topics::{Topic, TopicKind};

/// Topics sola-river always wants. Stored so a reconnect can re-subscribe
/// without main having to re-issue the list.
pub const SUBSCRIPTIONS: &[TopicKind] = &[
    TopicKind::Composition,
    TopicKind::Frame,
    TopicKind::Focus,
    // Floating state gates CSD move/resize. Sticky, so subscribing
    // replays the current float bit for every window (also on a
    // sola-river restart). Without this, `floating` stays empty and
    // titlebar drag bails at the "not floating" guard.
    TopicKind::WindowFloating,
    TopicKind::RegisteredChords,
    TopicKind::CloseApp,
    TopicKind::Shutdown,
];

pub struct BusClient {
    inner: sola_bus::BusClient,
    /// Last-known subscription set. Re-sent after every successful (re)connect
    /// so a bus restart does not leave us connected but deaf.
    subscriptions: Vec<TopicKind>,
    /// True once we have successfully connected at least once. Distinguishes
    /// boot connect from mid-session reconnect in logs and republish policy.
    ever_connected: bool,
}

impl BusClient {
    pub fn new() -> Self {
        let mut inner = sola_bus::BusClient::new();
        inner.set_app_id("sola-river");
        Self {
            inner,
            subscriptions: SUBSCRIPTIONS.to_vec(),
            ever_connected: false,
        }
    }

    /// Ensure we have a live bus connection. Returns `true` when this call
    /// performed a **reconnect** after a previous live session (i.e. the bus
    /// process was replaced). Initial boot connect returns `false` so callers
    /// do not re-publish empty state before Wayland has reported dimensions.
    pub fn ensure_connected(&mut self) -> bool {
        if self.inner.is_connected() {
            return false;
        }
        let was_live = self.ever_connected;
        match self.inner.connect() {
            Ok(()) => {
                self.ever_connected = true;
                self.resubscribe();
                if was_live {
                    tracing::info!("bus reconnected");
                    true
                } else {
                    tracing::info!("bus connected");
                    false
                }
            }
            Err(e) => {
                tracing::warn!(%e, "bus connect failed");
                false
            }
        }
    }

    /// Remember and apply a subscription set. Prefer [`SUBSCRIPTIONS`] at
    /// boot; this exists so tests (or a future dynamic set) can override.
    pub fn subscribe(&mut self, kinds: &[TopicKind]) {
        self.subscriptions = kinds.to_vec();
        self.resubscribe();
    }

    fn resubscribe(&mut self) {
        if self.subscriptions.is_empty() {
            return;
        }
        if let Err(e) = self.inner.subscribe(&self.subscriptions) {
            tracing::warn!(%e, "bus subscribe failed");
        }
    }

    pub fn try_recv(&mut self) -> Option<Message> {
        self.inner.try_recv()
    }

    pub fn emit(&mut self, topic: Topic) {
        if let Err(e) = self.inner.emit(topic) {
            tracing::warn!(%e, "bus emit failed");
        }
    }

    /// Retract a sticky topic (keyed by the payload's key fields). Used to drop
    /// a closed window's `WindowGeometry` so late subscribers can't resurrect it.
    pub fn retract(&mut self, topic: Topic) {
        if let Err(e) = self.inner.retract(topic) {
            tracing::warn!(%e, "bus retract failed");
        }
    }

    /// File descriptor that becomes readable when messages arrive.
    /// Used to register the bus with calloop.
    pub fn notify_fd(&self) -> Option<RawFd> {
        self.inner.notify_fd()
    }

    /// Drain pending notification bytes after `notify_fd` signals readable.
    pub fn drain_notify(&self) {
        self.inner.drain_notify();
    }
}
