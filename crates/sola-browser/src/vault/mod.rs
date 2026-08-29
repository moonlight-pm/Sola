//! First-party Bitwarden vault (D7).
//!
//! Official Bitwarden cloud + iced chrome login (incl. email new-device
//! verification / authenticator TOTP), unified vault panel (search + full
//! item record), URI fill, card/identity fill, authenticator TOTP, and
//! WebAuthn passkey get/create (FIDO2).
//!
//! Spec: `docs/specs/2026-08-28-sola-browser-vault-panel-design.md`.

pub mod bridge;
mod client;
mod fill_js;
mod generate;
mod identity;
mod item;
mod match_uri;
mod memory_repo;
mod org_crypto;
mod passkey;
mod prefs;
mod sync_cipher;
mod totp;
mod webauthn_js;
mod worker;

pub use bridge as passkey_bridge;
pub use bridge::PasskeyPageRequest;
pub use client::{
    CardFillMaterial, CardSummary, FillMaterial, LoginOutcome, MatchSummary, TotpSummary,
    TwoFactorKind, VaultError, VaultService, VaultStatus,
};
pub use fill_js::{
    fill_card_script, fill_credentials_script, fill_credentials_script_ex, fill_identity_script,
    fill_totp_script,
};
pub use generate::password as generate_password;
pub use item::{
    IdentityFillMaterial, ItemFilter, ItemKind, ItemRecord, ItemSummary, RecordField, filter_items,
};
pub use match_uri::{apex_domain, uri_matches};
pub use passkey::{PasskeyCandidate, create_account_hint};
pub use prefs::VaultPrefs;
pub use totp::remaining_secs as totp_remaining_secs;
pub use webauthn_js::{
    inject_webauthn_intercept_script, resolve_webauthn_script, resolve_webauthn_scripts,
};
pub use worker::{VaultCmd, VaultEvent, VaultHandle};
