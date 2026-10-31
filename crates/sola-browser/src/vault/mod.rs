//! The browser's side of the vault (D7, `docs/specs/2026-10-31-sola-vault-design.md`).
//!
//! `sola-vaultd` holds the unlocked vault; the browser is its client
//! ([`VaultHandle`], on the daemon's socket) and keeps the page: the fill
//! and WebAuthn scripts, which frames a fill may reach, and the panel
//! (`app/vault`). No Bitwarden SDK and no keys here.

pub mod bridge;
mod fill_js;
mod match_uri;
mod passkey;
mod scope;
mod webauthn_js;
mod worker;

pub use bridge::PasskeyPageRequest;
pub use fill_js::{
    fill_card_script, fill_credentials_script, fill_credentials_script_ex, fill_identity_script,
    fill_totp_script,
};
pub use match_uri::{apex_domain, log_host, registrable_domain, uri_matches};
pub use passkey::create_account_hint;
pub use scope::{frame_in_site, origin_of, rp_id_allowed, site_uri};
pub use sola_vault_proto as proto;
pub use webauthn_js::{inject_webauthn_intercept_script, resolve_webauthn_script};
pub use worker::{VaultEvent, VaultHandle};
