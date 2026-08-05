//! Clipboard sync over TCP (CLIP1) — separate from KVM1 UDP input.
//!
//! See `docs/specs/2026-07-30-sola-kvm-clipboard-design.md`.

mod platform;
mod proto;
mod worker;

pub use proto::{hash_text, Message, Role, MAGIC, VERSION};
pub use worker::{disabled_handle, spawn, ClipConfig, ClipHandle, ClipJob};
