//! First-party Bitwarden vault (D7).
//!
//! Official Bitwarden cloud + iced chrome login (incl. email new-device
//! verification / authenticator TOTP), URI match picker, page fill inject.
//!
//! Spec: `docs/specs/2026-08-10-sola-browser-bitwarden-design.md`.

mod client;
mod fill_js;
mod identity;
mod match_uri;
mod memory_repo;
mod prefs;
mod sync_cipher;
mod worker;

pub use client::{
    FillMaterial, LoginOutcome, MatchSummary, TwoFactorKind, VaultError, VaultService, VaultStatus,
};
pub use fill_js::fill_credentials_script;
pub use match_uri::uri_matches;
pub use prefs::VaultPrefs;
pub use worker::{VaultCmd, VaultEvent, VaultHandle};
