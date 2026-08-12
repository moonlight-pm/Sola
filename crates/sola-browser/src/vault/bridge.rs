//! Page ↔ vault bridge for WebAuthn passkeys (console → vault → EvaluateJs).

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Mutex, OnceLock};

/// Request captured from the page WebAuthn intercept.
#[derive(Debug, Clone)]
pub struct PasskeyPageRequest {
    pub id: u64,
    pub origin: String,
    pub rp_id: String,
    /// Serialized PublicKeyCredentialRequestOptions.publicKey (challenge etc. base64url).
    pub public_key_json: String,
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
        }
    } else {
        tracing::debug!("passkey: bridge not installed yet");
    }
}

/// Drain pending page requests (UI Tick).
pub fn try_recv() -> Option<PasskeyPageRequest> {
    let lock = FROM_UI.get()?;
    let rx = lock.lock().ok()?;
    match rx.try_recv() {
        Ok(r) => Some(r),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => None,
    }
}
