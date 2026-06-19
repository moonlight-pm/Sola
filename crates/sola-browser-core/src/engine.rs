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

#[derive(Debug, Clone)]
pub enum InputEvent {
    PointerMove { x: f64, y: f64, delta_x: f64, delta_y: f64, modifiers: u32, time_ms: u32 },
    PointerButton { down: bool, x: f64, y: f64, button: u32, modifiers: u32, time_ms: u32 },
    Scroll { x: f64, y: f64, delta_x: f64, delta_y: f64, precise: bool, modifiers: u32, time_ms: u32 },
    Key { down: bool, keyval: u32, keycode: u32, modifiers: u32, time_ms: u32 },
}

/// Commands the chrome sends to the engine worker. `Release` carries an
/// engine-specific token, so it is generic over the engine's token type.
pub enum Cmd<Tok> {
    Resize { width: u32, height: u32 },
    Release { token: Tok },
    Input(InputEvent),
    Focus(bool),
    Nav(NavCmd),
    OpenTab { id: TabId, url: String },
    CloseTab(TabId),
    SetActiveTab(TabId),
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
    pub releaser: Sender<Cmd<E::Token>>,
    pub last_size: Mutex<(u32, u32)>,
    pub cursor: Arc<AtomicU32>,
}

pub type TabsHandle = Arc<Mutex<Vec<TabInfo>>>;
pub type ActiveHandle = Arc<AtomicU64>;
pub type CursorHandle = Arc<AtomicU32>;
pub type FrameReceiver<F> = Arc<Mutex<Receiver<TaggedFrame<F>>>>;

/// A browser engine. Both `WpeEngine` and `CefEngine` already expose this
/// exact surface (7 methods + the CEF subprocess gate); the trait names it.
pub trait Engine: Sized + Send + Sync + 'static {
    /// Engine-specific raw frame (WPE: dma-buf fd; CEF: dma-buf or CPU buffer).
    type Frame: Send + 'static;
    /// Opaque buffer-recycle token returned via `Cmd::Release`.
    type Token: Send + 'static;
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
    fn cmd_sender(&self) -> Sender<Cmd<Self::Token>>;
    fn tabs_handle(&self) -> TabsHandle;
    fn active_tab_handle(&self) -> ActiveHandle;
    fn cursor_handle(&self) -> CursorHandle;
    fn frames(&self) -> FrameReceiver<Self::Frame>;
    fn make_program(slot: Arc<FrameSlot<Self>>) -> Self::Program;
    fn shutdown(self);
}
