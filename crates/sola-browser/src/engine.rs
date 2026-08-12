//! Engine-agnostic types shared by every Sola browser engine, plus the
//! `Engine` trait the shared chrome is generic over.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::mpsc::{Receiver, Sender, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(pub u64);

#[derive(Debug, Clone)]
pub struct TabInfo {
    pub id: TabId,
    pub url: String,
    pub title: String,
    /// True while WebKit is loading this tab (reload ↔ stop chrome).
    pub is_loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
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
/// [`crate::util::editing_command_name`].
///
/// Paste of system-clipboard text into page content uses
/// [`Cmd::PasteText`] instead — headless WPE has no Wayland clipboard, so
/// the chrome must read iced's clipboard and ship the string in.
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
/// and `Input` carries `E::Input` (WPE: GDK keyvals + f64 coords), so
/// input rides the normal command channel with no
/// process-wide side-channel.
pub enum Cmd<E: Engine> {
    /// Physical pixel size of the content scissor + compositor scale factor
    /// (for `wpe_toplevel_scale_changed` / HiDPI text).
    Resize { width: u32, height: u32, scale: f64 },
    /// FrameDone: UI presented this buffer (Wayland frame cb / blit done).
    /// Must precede `Release` for correct WebKit pacing; `Release` will
    /// FrameDone first if this was skipped.
    FrameDone { token: E::Token },
    /// Recycle a producer buffer (WPE dma-buf pool).
    Release { token: E::Token },
    Input(E::Input),
    Focus(bool),
    Nav(NavCmd),
    /// `title` seeds the tab strip before WebKit reports one (session restore).
    OpenTab { id: TabId, url: String, title: String },
    CloseTab(TabId),
    SetActiveTab(TabId),
    /// Run an editing command against the active tab's web content.
    Edit(EditCmd),
    /// Insert clipboard text into the page (chrome already read iced's
    /// Wayland clipboard). Preferred path for paste-into-page on WPE.
    PasteText(String),
    /// Run JavaScript in the active tab (password fill inject, etc.).
    /// Script is sourced from chrome; do not put untrusted page content here
    /// without escaping — vault fill embeds secrets via JSON string literals.
    EvaluateJs(String),
    Quit,
}

/// One frame as it crosses the worker→chrome boundary.
pub struct TaggedFrame<F> {
    pub tab_id: TabId,
    pub frame: F,
}

/// Latest-wins handoff from the engine worker to iced.
///
/// A capacity-1 queue that *dropped the newer frame* under load (blackouts
/// while scrolling). This mailbox always keeps the **newest** frame and
/// drops the older (older `Drop` → WPE release), then pings the consumer.
pub struct FrameMailbox<F: Send> {
    latest: Mutex<Option<TaggedFrame<F>>>,
    /// Capacity 1: `Full` means a wakeup is already queued.
    notify_tx: SyncSender<()>,
    notify_rx: Mutex<Receiver<()>>,
}

impl<F: Send> FrameMailbox<F> {
    pub fn new() -> Arc<Self> {
        let (notify_tx, notify_rx) = sync_channel(1);
        Arc::new(Self {
            latest: Mutex::new(None),
            notify_tx,
            notify_rx: Mutex::new(notify_rx),
        })
    }

    /// Producer path (WPE worker). Never blocks; prefer newest frame.
    pub fn push(&self, frame: TaggedFrame<F>) -> bool {
        let old = {
            let mut g = self.latest.lock().unwrap();
            g.replace(frame)
        };
        let replaced = old.is_some();
        // Release previous buffer outside the lock.
        drop(old);
        match self.notify_tx.try_send(()) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => {}
        }
        replaced
    }

    /// Consumer path (browser-frames thread). Blocks until a frame arrives.
    pub fn recv(&self) -> Result<TaggedFrame<F>, ()> {
        loop {
            if let Some(f) = self.latest.lock().unwrap().take() {
                // Drain coalesced pings; a concurrent push leaves its frame
                // in `latest` for the next `recv`.
                if let Ok(rx) = self.notify_rx.lock() {
                    while rx.try_recv().is_ok() {}
                }
                return Ok(f);
            }
            let ok = {
                let rx = self.notify_rx.lock().map_err(|_| ())?;
                rx.recv().is_ok()
            };
            if !ok {
                return Err(());
            }
        }
    }
}

/// Worker → chrome frame path (latest-wins mailbox).
pub type FrameReceiver<F> = Arc<FrameMailbox<F>>;

/// One decoded frame waiting for the shader, tagged with its tab so a
/// late background-tab frame cannot paint after the user switched away.
pub struct PendingFrame<E: Engine> {
    pub tab_id: TabId,
    pub frame: E::Frame,
}

/// Shared between `App` (fills `pending`) and the engine's shader Program
/// (drains it on next prepare). `cmd_tx` goes back to the engine worker.
pub struct FrameSlot<E: Engine> {
    /// Latest frame for the painted tab (and optional one-shot park primes).
    pub pending: Mutex<Option<PendingFrame<E>>>,
    /// Command channel to the engine worker (input, resize, nav, release, …).
    pub cmd_tx: Sender<Cmd<E>>,
    pub last_size: Mutex<(u32, u32)>,
    pub cursor: Arc<AtomicU32>,
    /// Tab the chrome wants painted (`TabId.0`). Written by chrome on tab switch.
    pub paint_tab: AtomicU64,
    /// Tab ids that still need a background snapshot for park-on-first-switch.
    /// Cleared when a frame for that tab is accepted into pending.
    pub need_park_prime: Mutex<std::collections::HashSet<u64>>,
    /// Tab ids whose GPU caches should be dropped (closed tabs).
    pub drop_paint_tabs: Mutex<Vec<u64>>,
    /// Coalesce `Msg::NewFrame`: only one iced wakeup is in flight. Without
    /// this, 60+ NewFrame/s fill the queue ahead of keyboard events (typing
    /// lag, frozen caret, slow placeholder animation).
    pub redraw_queued: AtomicBool,
}

pub type TabsHandle = Arc<Mutex<Vec<TabInfo>>>;
/// Active / paint-tab id (`TabId.0`). Chrome writes optimistically on
/// `switch_active_tab` so the worker can filter frames without waiting for
/// the cmd pump. Worker also writes on `Cmd::SetActiveTab` (focus/resize).
pub type ActiveHandle = Arc<AtomicU64>;
pub type CursorHandle = Arc<AtomicU32>;
/// Engine→chrome handoff for text the engine extracted for a copy (e.g. the
/// page's selection). The engine worker sets it; the chrome drains it on the
/// next `Tick` and writes it to the system clipboard via iced. `None` when
/// there's nothing pending.
pub type ClipboardHandle = Arc<Mutex<Option<String>>>;

/// A browser engine. Product path is [`crate::cef::CefEngine`].
pub trait Engine: Sized + Send + Sync + 'static {
    /// Engine-specific raw frame (CEF: CPU BGRA buffer).
    type Frame: Send + 'static;
    /// Opaque buffer-recycle token returned via `Cmd::Release`.
    type Token: Send + 'static;
    /// Engine-specific native input event carried by `Cmd::Input`.
    type Input: Send + 'static;
    /// The iced shader Program that imports `Self::Frame` and samples it.
    type Program: iced::widget::shader::Program<crate::app::Msg> + 'static;

    /// Optional early-exit for engines that re-exec helper processes.
    /// CEF returns `Some(exit_code)` for `--type=` subprocess workers.
    fn dispatch_subprocess(_app_id: &'static str) -> Option<std::process::ExitCode> {
        None
    }

    /// Bring the engine up. Encapsulates ALL engine-specific startup
    /// quirks (e.g. CEF `initialize` + message-loop thread).
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
    /// Orderly engine teardown: send Quit, join the worker. Called from
    /// `App` drop so iced exit flushes the engine cleanly.
    fn shutdown(&mut self);
}
