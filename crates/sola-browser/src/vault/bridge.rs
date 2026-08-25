//! Page ↔ vault bridge for WebAuthn passkeys (console → vault → EvaluateJs).

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Mutex, OnceLock};

/// Request captured from the page WebAuthn intercept.
#[derive(Debug, Clone)]
pub struct PasskeyPageRequest {
    pub id: u64,
    /// `get` or `create`.
    pub action: String,
    pub origin: String,
    pub rp_id: String,
    /// Serialized publicKey options (challenge / user.id etc. base64url).
    pub public_key_json: String,
}

impl PasskeyPageRequest {
    pub fn is_create(&self) -> bool {
        self.action == "create"
    }
}

static TO_UI: OnceLock<Sender<PasskeyPageRequest>> = OnceLock::new();
static FROM_UI: OnceLock<Mutex<Receiver<PasskeyPageRequest>>> = OnceLock::new();

/// Install the page→UI channel once (call from `run` before engine start).
pub fn install() {
    let (tx, rx) = mpsc::channel();
    let _ = TO_UI.set(tx);
    let _ = FROM_UI.set(Mutex::new(rx));
}

/// CEF console path → UI (non-blocking).
pub fn push_from_page(req: PasskeyPageRequest) {
    if let Some(tx) = TO_UI.get() {
        if let Err(e) = tx.send(req) {
            tracing::warn!(error = %e, "passkey: page→ui channel closed");
        } else {
            crate::chrome_wake::wake();
        }
    } else {
        tracing::debug!("passkey: bridge not installed yet");
    }
}

/// Drain pending page requests.
pub fn try_recv() -> Option<PasskeyPageRequest> {
    let lock = FROM_UI.get()?;
    let rx = lock.lock().ok()?;
    match rx.try_recv() {
        Ok(r) => Some(r),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => None,
    }
}

/// Page fill script reported whether it found fields (`__sola_vault_fill__:0|1`).
static FILL_TO_UI: OnceLock<Sender<bool>> = OnceLock::new();
static FILL_FROM_UI: OnceLock<Mutex<Receiver<bool>>> = OnceLock::new();

fn fill_channels() -> (&'static Sender<bool>, &'static Mutex<Receiver<bool>>) {
    let tx = FILL_TO_UI.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let _ = FILL_FROM_UI.set(Mutex::new(rx));
        tx
    });
    let rx = FILL_FROM_UI.get().expect("fill rx");
    (tx, rx)
}

pub fn push_fill_result(found: bool) {
    let (tx, _) = fill_channels();
    let _ = tx.send(found);
    crate::chrome_wake::wake();
}

pub fn try_recv_fill() -> Option<bool> {
    let (_, rx) = fill_channels();
    let lock = rx.lock().ok()?;
    match lock.try_recv() {
        Ok(v) => Some(v),
        Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
    }
}

pub fn drain_fill_results() {
    while try_recv_fill().is_some() {}
}
