//! Remote view, the host's half: this Mac's screen to the paired viewer
//! (`sola-kvm view`), and the viewer's input back. The wire is
//! `crates/sola-kvm/src/view_wire.rs`.
//!
//! Nothing runs until a viewer connects: capture starts with its video
//! stream and stops with it.

mod annexb;
#[cfg(target_os = "macos")]
mod capture;
#[cfg(target_os = "macos")]
mod cursor;
#[cfg(target_os = "macos")]
mod encode;
mod flow;
#[cfg(target_os = "macos")]
mod pattern;
mod server;

pub use server::{Source, spawn};

/// Stub for tests off macOS: there is no screen to send.
#[cfg(not(target_os = "macos"))]
mod capture {
    use super::server::{Feed, Source};
    use crate::view_wire::Down;

    pub struct Running;

    pub fn start(_source: Source, _feed: Feed) -> Result<(Down, Running), String> {
        Err("remote view needs macOS".into())
    }
}
