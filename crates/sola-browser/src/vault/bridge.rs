//! A page's WebAuthn request as chrome holds it while the user decides.

use std::time::Instant;

use crate::cef::ipc::WebAuthnEvent;
use crate::engine::TabId;

/// Request captured from the page WebAuthn intercept. `origin`, `tab` and
/// `frame` are the helper's (from the frame that sent it).
#[derive(Debug, Clone)]
pub struct PasskeyPageRequest {
    /// Chrome's id for the request (worker round trips echo it).
    pub id: u64,
    pub tab: TabId,
    /// CEF frame identifier: the answer goes to this frame only.
    pub frame: String,
    /// The page's key for its pending promise.
    pub nonce: String,
    /// `get` or `create`.
    pub action: String,
    pub origin: String,
    pub rp_id: String,
    /// Serialized publicKey options (challenge / user.id etc. base64url).
    pub public_key_json: String,
    pub received: Instant,
}

impl PasskeyPageRequest {
    pub fn new(id: u64, ev: WebAuthnEvent) -> Self {
        Self {
            id,
            tab: TabId(ev.tab),
            frame: ev.frame,
            nonce: ev.nonce,
            action: ev.action,
            origin: ev.origin,
            rp_id: ev.rp_id,
            public_key_json: ev.public_key_json,
            received: Instant::now(),
        }
    }

    pub fn is_create(&self) -> bool {
        self.action == "create"
    }

    /// The script that settles this request in its page.
    pub fn resolve_script(&self, ok: bool, payload: &str) -> String {
        super::resolve_webauthn_script(&self.nonce, ok, payload)
    }
}
