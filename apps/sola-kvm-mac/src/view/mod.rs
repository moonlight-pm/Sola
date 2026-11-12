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
mod display;
#[cfg(target_os = "macos")]
mod encode;
#[cfg(target_os = "macos")]
mod lid;
mod flow;
#[cfg(target_os = "macos")]
mod pattern;
mod rate;
mod server;
mod shape;

pub use server::{Source, spawn};

/// `sola-kvm-mac make-main <display>`: see [`display::make_main`].
#[cfg(target_os = "macos")]
pub use display::make_main;

#[cfg(not(target_os = "macos"))]
pub fn make_main(_display: u32) -> Result<(), String> {
    Err("arranging displays needs macOS".into())
}

/// Stub off macOS: no lid.
#[cfg(not(target_os = "macos"))]
mod lid {
    pub fn watch(_changed: impl Fn() + Send + Sync + 'static) {}

    pub fn closed() -> bool {
        false
    }
}

/// Stub for tests off macOS: there is no screen to send.
#[cfg(not(target_os = "macos"))]
mod capture {
    use super::server::{Feed, Source};
    use crate::view_wire::{Down, Size};

    pub struct Running;

    #[derive(Default)]
    pub struct Screen;

    impl Screen {
        pub fn release(&mut self) {}
    }

    pub fn can_mirror(_screen: &Screen) -> bool {
        true
    }

    pub fn allowed() -> bool {
        true
    }

    pub fn start(
        _source: Source,
        _size: Option<Size>,
        _screen: &mut Screen,
        _feed: Feed,
    ) -> Result<(Down, Running, bool), String> {
        Err("remote view needs macOS".into())
    }
}
