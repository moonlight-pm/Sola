//! Engine-agnostic types shared by every Sola browser engine, plus the
//! `Engine` trait the shared chrome is generic over.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(pub u64);

#[derive(Debug, Clone)]
pub struct TabInfo {
    pub id: TabId,
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub enum NavCmd {
    Back,
    Forward,
    Reload,
    Stop,
    LoadUrl(String),
}

/// Editing commands routed to the focused web content (or, in the chrome,
/// to the URL bar). Names map to WebKit editing-command strings via
/// [`crate::util::editing_command_name`]; CEF maps them to `cef::Frame`
/// methods directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditCmd {
    Copy,
    Cut,
    Paste,
    SelectAll,
    Undo,
    Redo,
}

/// Commands the chrome sends to the engine worker. Generic over the
/// engine `E`: `Release` carries `E::Token` (the buffer-recycle token)
/// and `Input` carries `E::Input` (the engine's native input shape —
/// WPE uses GDK keyvals + f64 coords, CEF uses Windows VK codes +
/// integer pixels), so input rides the normal command channel with no
/// process-wide side-channel.
pub enum Cmd<E: Engine> {
    Resize { width: u32, height: u32 },
    Release { token: E::Token },
    Input(E::Input),
    Focus(bool),
    Nav(NavCmd),
    OpenTab { id: TabId, url: String },
    CloseTab(TabId),
    SetActiveTab(TabId),
    /// Run an editing command against the active tab's web content.
    Edit(EditCmd),
    Quit,
}

/// One frame as it crosses the worker→chrome boundary.
pub struct TaggedFrame<F> {
    pub tab_id: TabId,
    pub frame: F,
}

/// Shared between `App` (fills `pending`) and the engine's shader Program
/// (drains it on next prepare). `releaser` goes back to the engine worker.
pub struct FrameSlot<E: Engine> {
    pub pending: Mutex<Option<E::Frame>>,
    pub releaser: Sender<Cmd<E>>,
    pub last_size: Mutex<(u32, u32)>,
    pub cursor: Arc<AtomicU32>,
}

pub type TabsHandle = Arc<Mutex<Vec<TabInfo>>>;
pub type ActiveHandle = Arc<AtomicU64>;
pub type CursorHandle = Arc<AtomicU32>;
pub type FrameReceiver<F> = Arc<Mutex<Receiver<TaggedFrame<F>>>>;
/// Engine→chrome handoff for text the engine extracted for a copy (e.g. the
/// page's selection). The engine worker sets it; the chrome drains it on the
/// next `Tick` and writes it to the system clipboard via iced. `None` when
/// there's nothing pending.
pub type ClipboardHandle = Arc<Mutex<Option<String>>>;

/// A browser engine. Both `WpeEngine` and `CefEngine` already expose this
/// exact surface (7 methods + the CEF subprocess gate); the trait names it.
pub trait Engine: Sized + Send + Sync + 'static {
    /// Engine-specific raw frame (WPE: dma-buf fd; CEF: dma-buf or CPU buffer).
    type Frame: Send + 'static;
    /// Opaque buffer-recycle token returned via `Cmd::Release`.
    type Token: Send + 'static;
    /// Engine-specific native input event carried by `Cmd::Input` (WPE:
    /// GDK keyvals + f64 coords; CEF: Windows VK codes + integer pixels).
    type Input: Send + 'static;
    /// The iced shader Program that imports `Self::Frame` and samples it.
    type Program: iced::widget::shader::Program<crate::app::Msg> + 'static;

    /// CEF subprocess gate; runs first in `run()`, before logging/Wayland
    /// init. WPE returns `None`; CEF dispatches `--type=` workers and
    /// returns `Some(exit_code)`.
    fn dispatch_subprocess(_app_id: &'static str) -> Option<std::process::ExitCode> {
        None
    }

    /// Bring the engine up. Encapsulates ALL engine-specific startup
    /// quirks (e.g. WPE's WEBKIT_EXEC_PATH + WAYLAND_DISPLAY dance).
    fn spawn(app_id: &'static str, url: &str, w: u32, h: u32) -> Self;

    fn alloc_tab_id(&self) -> TabId;
    fn cmd_sender(&self) -> Sender<Cmd<Self>>;
    fn tabs_handle(&self) -> TabsHandle;
    fn active_tab_handle(&self) -> ActiveHandle;
    fn cursor_handle(&self) -> CursorHandle;
    /// Shared slot the engine fills with copy text (page selection) for the
    /// chrome to drain onto the system clipboard. See [`ClipboardHandle`].
    fn clipboard_handle(&self) -> ClipboardHandle;
    fn frames(&self) -> FrameReceiver<Self::Frame>;
    fn make_program(slot: Arc<FrameSlot<Self>>) -> Self::Program;
    fn shutdown(self);
}
