//! The vault's socket protocol (`docs/specs/2026-10-31-sola-vault-design.md`).
//!
//! `sola-vaultd` alone holds the unlocked vault. Vault (the app), the
//! browser and the shell talk to it here, on its own socket — never the
//! bus or the call plane, which agents reach. This crate is what those
//! clients share: the data types, the requests and replies, and a blocking
//! client. It has no Bitwarden SDK.
//!
//! Wire: one JSON object per line. A client sends [`Envelope`]`<Request>`
//! and reads `Envelope<Response>` with the same id; after
//! [`Request::Subscribe`] the connection carries [`Event`] lines only.
//! Additive only: a new method, field (`#[serde(default)]`) or variant,
//! never a changed one — the browser, the shell and Vault can be from
//! different builds than the daemon.

mod client;
pub mod draft;
pub mod totp;
mod types;
mod wire;

pub use client::{Client, Events, call, subscribe};
pub use draft::{CustomDraft, DraftRow, ItemDraft, UriDraft};
pub use totp::{TotpSpec, remaining_secs};
pub use types::*;
pub use wire::{Envelope, ErrorCode, Event, Reply, Request, Response, VaultError};

use std::path::PathBuf;

/// The protocol's version, sent in [`Request::Hello`].
pub const VERSION: u32 = 1;

/// The daemon's socket: `$XDG_RUNTIME_DIR/sola-vault.sock` (mode `0600`).
pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", uid())));
    dir.join("sola-vault.sock")
}

/// Our uid, read from `/proc` (no libc).
fn uid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("Uid:"))
                .and_then(|v| v.split_whitespace().next())
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(1000)
}
