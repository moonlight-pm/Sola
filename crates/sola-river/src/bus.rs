//! Thin wrapper around `sola_bus::BusClient` that:
//!   - Sets our app_id.
//!   - Exposes `ensure_connected` and `subscribe` for connection setup.
//!   - Exposes `try_recv` and `emit*` for the translator.
use std::os::unix::io::RawFd;

use sola_bus::Message;
use sola_bus::topics::{Topic, TopicKind};

pub struct BusClient {
    inner: sola_bus::BusClient,
}

impl BusClient {
    pub fn new() -> Self {
        let mut inner = sola_bus::BusClient::new();
        inner.set_app_id("sola-river");
        Self { inner }
    }

    pub fn ensure_connected(&mut self) {
        if !self.inner.is_connected() {
            if let Err(e) = self.inner.connect() {
                tracing::warn!(%e, "bus connect failed");
            }
        }
    }

    pub fn subscribe(&mut self, kinds: &[TopicKind]) {
        if let Err(e) = self.inner.subscribe(kinds) {
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

    pub fn emit_sticky(&mut self, topic: Topic) {
        if let Err(e) = self.inner.emit_sticky(topic) {
            tracing::warn!(%e, "bus emit_sticky failed");
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
