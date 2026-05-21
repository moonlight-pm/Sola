//! CEF engine wrapper used by the main browser binary.
//!
//! Mirrors the public surface of `sola-browser-wpe::wpe::WpeEngine`
//! (spawn / cmd_sender / frames / shutdown) but drives a CEF
//! off-screen browser instead of a WPE Platform API display.
//!
//! Phase A (this file) is a scaffold — the type shape exists so
//! `shader.rs` and `main.rs` compile against the same `Cmd` /
//! `CefFrame` / `ResourceToken` types as the WPE crate. Engine
//! lifecycle, browser creation, and `on_accelerated_paint` plumbing
//! land in Phase B; see the implementation plan and
//! `crates/sola-kit/src/cef/browser.rs` for the reference wiring.

use std::ffi::c_void;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

/// One frame as it crosses thread boundaries. Shape is intentionally
/// identical to `sola_browser_wpe::wpe::WpeFrame` so `shader.rs` and
/// `wgpu_import.rs` are pure copies.
pub struct CefFrame {
    pub fd: OwnedFd,
    pub width: u32,
    pub height: u32,
    /// DRM fourcc (e.g. `0x34325241` = ARGB8888).
    pub format: u32,
    pub modifier: u64,
    pub stride: u32,
    pub offset: u32,
    pub token: ResourceToken,
}

/// `Send + Sync`-safe wrapper around opaque CEF resource handles
/// the producer hands us alongside a frame. We give it back on
/// `Cmd::Release` so CEF can recycle the underlying texture.
#[derive(Clone, Copy, Debug)]
pub struct ResourceToken {
    pub browser: *mut c_void,
    pub buffer: *mut c_void,
}

unsafe impl Send for ResourceToken {}
unsafe impl Sync for ResourceToken {}

pub enum Cmd {
    /// Request a new viewport size — calls `browser.host().was_resized()`
    /// after updating the cached size that `RenderHandler::view_rect`
    /// returns.
    Resize { width: u32, height: u32 },
    Release { token: ResourceToken },
    Quit,
}

pub struct CefEngine {
    _worker: Option<JoinHandle<()>>,
    cmd_tx: Sender<Cmd>,
    frames: Arc<Mutex<Receiver<CefFrame>>>,
}

impl CefEngine {
    /// Called very early in `main`. If this process was re-exec'd as
    /// a CEF subprocess (renderer / GPU / network helper) the
    /// underlying `cef::execute_process` returns the subprocess exit
    /// code, which we propagate. Returns `None` for the browser
    /// process so `main` continues.
    pub fn dispatch_subprocess() -> Option<i32> {
        // Phase B will wire `cef::args::Args::new()` + `cef::execute_process`.
        // For scaffold, always run as the browser (no subprocess fan-out).
        None
    }

    pub fn spawn(_url: &str, _width: u32, _height: u32) -> Self {
        let (cmd_tx, _cmd_rx) = channel::<Cmd>();
        let (_frame_tx, frame_rx) = channel::<CefFrame>();
        // Phase B: spawn worker thread that drives CEF's message
        // loop and pumps frames into `frame_tx`. For now leave
        // the worker absent so the binary at least starts and
        // shows an empty iced window.
        tracing::warn!(
            "CefEngine::spawn is a scaffold — no CEF browser created, no frames will arrive"
        );
        Self {
            _worker: None,
            cmd_tx,
            frames: Arc::new(Mutex::new(frame_rx)),
        }
    }

    pub fn cmd_sender(&self) -> Sender<Cmd> {
        self.cmd_tx.clone()
    }

    pub fn frames(&self) -> Arc<Mutex<Receiver<CefFrame>>> {
        self.frames.clone()
    }

    pub fn shutdown(self) {
        // Phase B: signal Quit, CEF shutdown, join worker.
        let _ = self.cmd_tx.send(Cmd::Quit);
    }
}
