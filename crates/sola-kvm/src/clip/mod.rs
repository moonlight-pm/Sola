//! Clipboard sync over TCP (CLIP1) — separate from KVM1 UDP input.
//!
//! See `docs/specs/2026-07-30-sola-kvm-clipboard-design.md`.

mod platform;
mod proto;
mod worker;

pub use proto::{MAGIC, Message, Role, VERSION, hash_text};
pub use worker::{ClipConfig, ClipHandle, ClipJob, disabled_handle, spawn, spawn_listen};
