//! Coalesced iced wakeup for chrome polls.
//!
//! Iced 0.14 presents the whole window after every `Message`. Helper threads
//! (CEF router, vault, handoff, passkey bridge) call [`wake`] when a queue
//! actually has work. The chrome `Tick` subscription is *not* a 250 ms idle
//! pump.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static QUEUED: AtomicBool = AtomicBool::new(false);
static TX: Mutex<Option<iced::futures::channel::mpsc::UnboundedSender<()>>> = Mutex::new(None);

/// Safe from any thread. No-ops until iced installs the stream.
pub fn wake() {
    if QUEUED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    let tx = TX.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(tx) = tx.as_ref() {
        if tx.unbounded_send(()).is_err() {
            QUEUED.store(false, Ordering::Relaxed);
        }
    } else {
        QUEUED.store(false, Ordering::Relaxed);
    }
}

pub fn take_queued() {
    QUEUED.store(false, Ordering::Relaxed);
}

pub fn install_tx(tx: iced::futures::channel::mpsc::UnboundedSender<()>) {
    let mut slot = TX.lock().unwrap_or_else(|p| p.into_inner());
    *slot = Some(tx);
}
