//! Engine-agnostic types shared by every Sola browser engine, plus the
//! `Engine` trait the shared chrome is generic over.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

/// Shader keeps requesting redraws this long after the last paint so
/// animations do not kick `Msg::NewFrame` (and rebuild chrome) every frame.
pub const FRAME_PUMP_HANGOVER_MS: u64 = 50;

pub fn monotonic_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct TabId(pub u64);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HistoryEntry {
    pub index: i32,
    pub url: String,
    pub title: String,
}

/// Right-click target on the page (CEF context-menu params, chrome-owned UI).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PageContext {
    pub link_url: Option<String>,
    pub src_url: Option<String>,
    pub selection: Option<String>,
    pub editable: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// View-pixel hit from CEF (for Inspect element).
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TabInfo {
    pub id: TabId,
    pub url: String,
    pub title: String,
    /// True while this tab is loading (reload ↔ stop + omnibox progress).
    pub is_loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// CEF overall load progress in `0.0..=1.0`. Meaningful while `is_loading`.
    #[serde(default)]
    pub load_progress: f32,
    /// Session history for the **active** tab (empty on background tabs).
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    /// Index of the current history entry in [`Self::history`].
    #[serde(default)]
    pub history_index: i32,
}

impl TabInfo {
    pub fn chrome(id: TabId, url: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id,
            url: url.into(),
            title: title.into(),
            is_loading: false,
            can_go_back: false,
            can_go_forward: false,
            load_progress: 0.0,
            history: Vec::new(),
            history_index: 0,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum NavCmd {
    Back,
    Forward,
    Reload,
    Stop,
    LoadUrl(String),
    /// `window.history.go(delta)` on the active tab (`-1` = back one).
    GoHistory {
        delta: i32,
    },
}

/// Editing commands routed to the focused web content (or, in the chrome,
/// to the URL bar). Names map to WebKit editing-command strings via
/// [`crate::util::editing_command_name`].
///
/// Paste of system-clipboard text into page content uses
/// [`Cmd::PasteText`] instead — headless WPE has no Wayland clipboard, so
/// the chrome must read iced's clipboard and ship the string in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    Resize {
        width: u32,
        height: u32,
        scale: f64,
    },
    /// FrameDone: UI presented this buffer (Wayland frame cb / blit done).
    /// Must precede `Release` for correct WebKit pacing; `Release` will
    /// FrameDone first if this was skipped.
    FrameDone {
        token: E::Token,
    },
    /// Recycle a producer buffer (WPE dma-buf pool).
    Release {
        token: E::Token,
    },
    Input(E::Input),
    Focus(bool),
    /// Profile helper is / is not the painted identity. `false` hides every
    /// tab so a parked profile stops compositing (and stops sending frames).
    SetFront(bool),
    Nav(NavCmd),
    /// `title` seeds the tab strip before WebKit reports one (session restore).
    OpenTab {
        id: TabId,
        url: String,
        title: String,
    },
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
    /// Profile workspace switch without reloading parked pages.
    ///
    /// 1. Hide + park the current live tabs under `park_as_profile_id`
    ///    (same CEF browsers + request context stay in memory).
    /// 2. If a park exists for `resume_profile_id`, restore it.
    /// 3. Else create a request context at `cef_cache_path` and open
    ///    `create_tabs` (cold profile / after eviction).
    /// 4. Apply the shared eviction policy (idle / tab budget / park count).
    SwitchProfileWorkspace {
        park_as_profile_id: String,
        resume_profile_id: String,
        cef_cache_path: String,
        /// `None` → resume from park only (must exist). `Some` → create these
        /// tabs if not parked (or after forced create).
        create_tabs: Option<Vec<(TabId, String, String)>>,
        active: TabId,
    },
    /// Drop a parked profile workspace (eviction / profile deleted).
    DropParkedProfile {
        profile_id: String,
    },
    /// Cancel a CEF download on the helper that started it.
    CancelDownload {
        profile_id: String,
        id: u32,
    },
    /// Complete a CEF `OnShowPermissionPrompt` (notifications Allow / Block).
    NotifyPermission {
        prompt_id: u64,
        granted: bool,
    },
    /// Open Chromium DevTools for the active tab as a chrome tab.
    /// `panel` is `console` or `elements`. `inspect_*` selects the node
    /// under that view point (Inspect element).
    ShowDevTools {
        panel: String,
        inspect_x: Option<i32>,
        inspect_y: Option<i32>,
    },
    /// Helper IPC died — router respawns and restores tabs.
    HelperDied {
        profile_id: String,
    },
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
    /// Last composite for every tab we have painted this session.
    /// `present_tab` installs a same-size hit synchronously so tab /
    /// profile switch is instant; a miss blanks instead of keeping
    /// the previous tab on screen.
    pub parked_frames: Mutex<HashMap<u64, E::Frame>>,
    /// Drop the last sampled texture on the next shader prepare (dark
    /// fallback) until a frame for `paint_tab` arrives. Set on profile
    /// switch so the previous identity is not left on screen while the
    /// new helper opens / paints.
    pub blank_content: AtomicBool,
    /// Coalesce `Msg::NewFrame`: only one iced wakeup is in flight. Without
    /// this, 60+ NewFrame/s fill the queue ahead of keyboard events (typing
    /// lag, frozen caret, slow placeholder animation).
    pub redraw_queued: AtomicBool,
    /// Shader is already request_redraw-pumping; frame stream should not
    /// enqueue another `NewFrame` (that rebuilds chrome).
    pub pumping: AtomicBool,
    /// Monotonic ms of the last accepted paint (shader hangover / kick).
    pub last_frame_ms: AtomicU64,
    /// Composition caret (helper) / last pointer fallback (chrome).
    pub ime: ImeHandle,
}

impl<E: Engine> FrameSlot<E> {
    /// Front `id` in chrome. Same-size parked frame → pending this
    /// frame (instant). Otherwise blank until CEF paints.
    pub fn present_tab(&self, id: TabId) {
        self.paint_tab.store(id.0, Ordering::Relaxed);
        let want = *self.last_size.lock().unwrap();
        let hit = self
            .parked_frames
            .lock()
            .unwrap()
            .get(&id.0)
            .cloned()
            .filter(|f| crate::shader::size_matches(E::frame_size(f), want));
        let mut pending = self.pending.lock().unwrap();
        if let Some(frame) = hit {
            *pending = Some(PendingFrame { tab_id: id, frame });
            self.blank_content.store(false, Ordering::Relaxed);
        } else {
            *pending = None;
            self.blank_content.store(true, Ordering::Relaxed);
        }
    }

    pub fn forget_tab(&self, id: TabId) {
        self.parked_frames.lock().unwrap().remove(&id.0);
        let mut pending = self.pending.lock().unwrap();
        if pending.as_ref().is_some_and(|p| p.tab_id == id) {
            *pending = None;
        }
    }
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

/// Last IME / caret box in CEF view pixels. `w == 0` means "use last
/// pointer as a 1×16 fallback".
#[derive(Debug, Clone, Copy, Default)]
pub struct ImeCaret {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl ImeCaret {
    pub fn logical_rect(&self, bounds: iced::Rectangle, scale: f32) -> iced::Rectangle {
        let scale = scale.max(0.5);
        let (x, y, w, h) = if self.w > 0 && self.h > 0 {
            (self.x, self.y, self.w, self.h)
        } else {
            (self.x, self.y, 1, 16)
        };
        iced::Rectangle::new(
            iced::Point::new(bounds.x + x as f32 / scale, bounds.y + y as f32 / scale),
            iced::Size::new((w as f32 / scale).max(1.0), (h as f32 / scale).max(1.0)),
        )
    }
}

#[cfg(test)]
mod ime_caret_tests {
    use super::*;

    #[test]
    fn fallback_is_one_by_sixteen_at_point() {
        let c = ImeCaret {
            x: 40,
            y: 80,
            w: 0,
            h: 0,
        };
        let r = c.logical_rect(
            iced::Rectangle::new(iced::Point::new(10.0, 20.0), iced::Size::new(100.0, 100.0)),
            1.0,
        );
        assert_eq!(r.x, 50.0);
        assert_eq!(r.y, 100.0);
        assert_eq!(r.width, 1.0);
        assert_eq!(r.height, 16.0);
    }
}

pub type ImeHandle = Arc<Mutex<ImeCaret>>;

/// Helper download events waiting for chrome (`profile_id` + payload).
pub type DownloadsHandle = Arc<Mutex<Vec<(String, crate::cef::ipc::DownloadEvent)>>>;
/// Helper WebAuthn intercepts waiting for chrome.
pub type PasskeysHandle = Arc<Mutex<Vec<crate::cef::ipc::WebAuthnEvent>>>;
/// Helper → chrome page context-menu requests (right-click on content).
pub type PageMenusHandle = Arc<Mutex<Vec<PageContext>>>;
/// Helper → chrome: open these URLs as background tabs (⌘-click / popup).
/// Chrome mints ids so they do not collide with the session strip.
pub type BackgroundTabsHandle = Arc<Mutex<Vec<String>>>;
/// Helper → chrome: Web Notification show / permission.
pub type NotificationsHandle = Arc<Mutex<Vec<crate::notify::Ipc>>>;

/// A browser engine. Product path is [`crate::cef::CefEngine`].
pub trait Engine: Sized + Send + Sync + 'static {
    /// Engine-specific raw frame (CEF: CPU BGRA buffer).
    type Frame: Send + Clone + 'static;
    /// Pixel size of a parked frame (for same-size replay).
    fn frame_size(frame: &Self::Frame) -> (u32, u32);
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
    fn ime_handle(&self) -> ImeHandle;
    fn downloads_handle(&self) -> DownloadsHandle;
    fn passkeys_handle(&self) -> PasskeysHandle;
    fn page_menus_handle(&self) -> PageMenusHandle;
    fn background_tabs_handle(&self) -> BackgroundTabsHandle;
    fn notifications_handle(&self) -> NotificationsHandle;
    fn frames(&self) -> FrameReceiver<Self::Frame>;
    fn make_program(slot: Arc<FrameSlot<Self>>) -> Self::Program;
    /// Orderly engine teardown: send Quit, join the worker. Called from
    /// `App` drop so iced exit flushes the engine cleanly.
    fn shutdown(&mut self);
}
