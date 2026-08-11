//! Browser chrome: message type, layout constants, and the generic `App<E>`.
//!
//! `Msg` and the consts were stubbed out in Task 1 and are kept here. Task 2
//! adds `App<E>`, its constructor, and all update/view/subscription methods.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use iced::widget::{
    Shader, Space, button, column, container, mouse_area, row, scrollable, stack, text,
};
use sola_kit::components::text_input::text_input;
use iced::{Alignment, Element, Event, Length, Padding, Subscription, Task, event, keyboard, mouse};
use sola_kit::components::{
    TabDescriptor, TabSize, horizontal_divider, toolbar_button, vertical_divider_with,
    vertical_tabs_sized,
};
use sola_kit::components::button as kit_button;
use sola_kit::components::card;
use sola_kit::components::icon::icon_handle;
use sola_kit::components::style::PAD_CONTROL_SM;
use sola_kit::components::toolbar as kit_toolbar;


use crate::engine::{Cmd, EditCmd, Engine, FrameSlot, NavCmd, TabId, TabInfo, TabsHandle};
use crate::session::{self, SessionTab};
#[cfg(feature = "bitwarden")]
use crate::vault::{
    MatchSummary, TwoFactorKind, VaultCmd, VaultEvent, VaultHandle, VaultStatus,
    fill_credentials_script,
};
#[cfg(feature = "bitwarden")]
use zeroize::Zeroize;

pub const DEFAULT_URL: &str = "https://www.wikipedia.org";
/// A fresh blank tab (⌘T). Loaded as an empty page; the chrome shows an empty,
/// focused URL bar rather than the literal "about:blank".
pub const BLANK_URL: &str = "about:blank";
pub const VIEW_W: u32 = 1280;
pub const VIEW_H: u32 = 800;
pub const CHROME_HEIGHT: f32 = 46.0;
/// Tab sidebar width (logical px) — the value the draggable divider
/// edits, clamped to `[MIN, MAX]`.
pub const SIDEBAR_W_DEFAULT: f32 = 200.0;
pub const SIDEBAR_W_MIN: f32 = 120.0;
pub const SIDEBAR_W_MAX: f32 = 420.0;

#[derive(Debug, Clone)]
pub enum Msg {
    NewFrame,
    NavBack,
    NavForward,
    /// Reload when idle; stop when the active tab is loading.
    NavReloadOrStop,
    /// Escape / explicit stop — always `NavCmd::Stop`.
    NavStop,
    UrlInput(String),
    UrlSubmit,
    CloseTab(TabId),
    ActivateTab(TabId),
    /// Timer tick — refresh `cached_tabs`/`cached_active` and
    /// sync `url_field` if the active tab's URL changed.
    Tick,
    /// A message delivered over the Sola bus (theme, open-url, menu
    /// action, close-app). Handled by `integration::handle_bus`.
    Bus(Arc<sola_bus::Message>),
    /// User pressed the mouse on the sidebar divider.
    DividerPress,
    /// Global cursor moved — only acted on while dragging the divider.
    CursorMoved(f32),
    /// Global left-button released — ends a divider drag.
    CursorReleased,
    /// Hovered tab row changed (index into `cached_tabs`), or `None`.
    TabHover(Option<usize>),
    /// A left button press landed inside the web view — the page took
    /// keyboard focus, so edit commands route to the engine (not the URL bar).
    WebViewFocused,
    /// A global left press — triggers a URL-bar focus query so we can
    /// select-all when the field has just gained focus (browser behavior).
    LeftPressed,
    /// Result of the focus query started by [`Msg::LeftPressed`]: whether the
    /// URL bar currently holds focus. Selects-all on the false→true edge.
    UrlBarFocusSync(bool),
    /// Result of the live focus query for an Edit action (⌘C/⌘X/⌘V/⌘A):
    /// route `cmd` to the URL bar when `url_bar_focused`, else the engine.
    EditRouted { cmd: EditCmd, url_bar_focused: bool },
    /// Result of an `iced::clipboard::read` kicked off by a URL-bar paste.
    UrlPasted(Option<String>),
    /// Result of an `iced::clipboard::read` for paste into page content.
    PagePasted(Option<String>),
    /// Clipboard paste targeted at the open vault form (⌘V via Edit menu).
    #[cfg(feature = "bitwarden")]
    VaultClipboardPaste(Option<String>),
    WindowReady(Option<iced::window::Id>),
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
    // —— Bitwarden vault (feature `bitwarden`) ——
    /// Toolbar lock: open login / status panel.
    VaultToggle,
    VaultEmail(String),
    VaultPassword(String),
    VaultOtp(String),
    VaultLogin,
    VaultVerifyOtp,
    VaultResendEmailCode,
    VaultPanelClose,
    /// Pick a vault match and fill the active page.
    #[cfg(feature = "bitwarden")]
    VaultFill(String),
    /// Re-query matches for the active tab URL.
    #[cfg(feature = "bitwarden")]
    VaultRefreshMatches,
    /// Tab / Shift+Tab while vault panel is open.
    VaultFocusNext,
    VaultFocusPrev,
    /// Worker → chrome (drained on Tick when bitwarden enabled).
    #[cfg(feature = "bitwarden")]
    VaultWorker(VaultEvent),
}

/// Which form the vault panel shows.
#[cfg(feature = "bitwarden")]
#[derive(Debug, Clone, Default)]
enum VaultPanelPhase {
    #[default]
    Credentials,
    /// Email new-device / 2FA or authenticator TOTP.
    TwoFactor {
        kind: TwoFactorKind,
        email_hint: Option<String>,
    },
}

/// Where shell Edit → Paste (⌘V) should land while the vault panel is open.
#[cfg(feature = "bitwarden")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum VaultPasteTarget {
    #[default]
    Email,
    Password,
    Otp,
}

/// Browser chrome application state, generic over the web engine.
///
/// `engine` owns the worker and keeps it alive for the lifetime of the
/// process — no process-wide statics needed.
pub struct App<E: Engine> {
    /// The running web engine. Kept alive here so neither a static nor an
    /// `Arc` is needed to keep the worker alive.
    pub engine: E,
    pub slot: Arc<FrameSlot<E>>,
    /// Command channel to the engine worker.
    pub cmd_tx: Sender<Cmd<E>>,
    /// Live tab snapshot, owned by the engine. We re-read on
    /// every Tick; `cached_tabs` is the value at last read.
    pub tabs_handle: TabsHandle,
    /// Active-tab id (worker is the sole writer after startup).
    /// Chrome keeps `cached_active` for optimistic paint.
    pub active_handle: Arc<AtomicU64>,
    /// Snapshot of tabs as of the last Tick — view() and
    /// subscription helpers read from here so they don't have to
    /// re-lock the engine's Mutex on every frame.
    pub cached_tabs: Vec<TabInfo>,
    pub cached_active: TabId,
    /// Editable contents of the URL bar.
    pub url_field: String,
    /// The URL we last copied from the engine into `url_field`,
    /// so we only overwrite on actual change.
    pub last_seen_url: String,
    /// Active iced theme, refreshed live from `Topic::Theme` so the
    /// chrome (tab strip, URL bar, buttons) tracks the system theme.
    pub theme: iced::Theme,
    /// Tab sidebar width; edited by the draggable divider.
    pub sidebar_w: f32,
    /// True while the divider is being dragged.
    pub dragging_divider: bool,
    /// Most-recent global cursor x, tracked continuously so the drag
    /// anchor is current at `DividerPress` time.
    pub last_cursor_x: Option<f32>,
    /// `(cursor_x_at_press, sidebar_w_at_press)` — anchor-relative drag
    /// (recompute from displacement, never accumulate deltas).
    pub drag_anchor: Option<(f32, f32)>,
    /// Index of the hovered tab row, if any — drives the float-in close
    /// button. Recomputed from `mouse_area` enter/exit each frame.
    pub hovered_tab: Option<usize>,
    /// Float tracker + iced window id for CSD while floating.
    pub float: sola_kit::FloatState,
    pub window_id: Option<iced::window::Id>,
    /// The app_id string passed to `run::<E>`, stored so `Msg::Bus` can
    /// forward it to `integration::handle_bus` without a static.
    pub app_id: &'static str,
    /// True when the chrome URL bar holds keyboard focus, so `Edit`
    /// commands target it instead of the web content. Set by ⌘L / ⌘T /
    /// typing in the bar; cleared when a press lands in the web view.
    /// Best-effort heuristic — see the design spec's documented edge case.
    pub url_bar_focused: bool,
    /// Last written session fingerprint — skip disk when unchanged.
    session_fp: String,
    /// Tabs to open after the iced Wayland window exists. Deferred so we can
    /// clear `WAYLAND_DISPLAY` first — otherwise WebKit's WebProcess inherits
    /// it and maps a real `org.webkit.*` toplevel next to our chrome.
    pending_session: Option<(Vec<SessionTab>, usize)>,
    /// Bitwarden vault worker + login panel state.
    #[cfg(feature = "bitwarden")]
    vault: VaultHandle,
    #[cfg(feature = "bitwarden")]
    vault_panel_open: bool,
    #[cfg(feature = "bitwarden")]
    vault_phase: VaultPanelPhase,
    #[cfg(feature = "bitwarden")]
    vault_email: String,
    #[cfg(feature = "bitwarden")]
    vault_password: String,
    /// Held across 2FA so we can resend / complete without retyping.
    #[cfg(feature = "bitwarden")]
    vault_pending_password: Option<String>,
    #[cfg(feature = "bitwarden")]
    vault_otp: String,
    /// Last vault field that received input — targets ⌘V paste from Edit menu.
    #[cfg(feature = "bitwarden")]
    vault_paste_target: VaultPasteTarget,
    #[cfg(feature = "bitwarden")]
    vault_error: Option<String>,
    #[cfg(feature = "bitwarden")]
    vault_busy: bool,
    #[cfg(feature = "bitwarden")]
    vault_status: VaultStatus,
    /// URI matches for the active tab (unlocked panel).
    #[cfg(feature = "bitwarden")]
    vault_matches: Vec<MatchSummary>,
    #[cfg(feature = "bitwarden")]
    vault_matches_loading: bool,
    /// Page URL last used for `vault_matches`.
    #[cfg(feature = "bitwarden")]
    vault_matches_url: String,
    #[cfg(feature = "bitwarden")]
    vault_icon_locked: iced::widget::svg::Handle,
    #[cfg(feature = "bitwarden")]
    vault_icon_unlocked: iced::widget::svg::Handle,
}

impl<E: Engine> App<E> {
    /// Construct the initial app state from an already-spawned engine.
    ///
    /// `run::<E>` calls this inside the iced application init closure so the
    /// engine is moved into `App` (rather than a static) and kept alive for
    /// the process lifetime.
    pub fn new(
        engine: E,
        slot: Arc<FrameSlot<E>>,
        cmd_tx: Sender<Cmd<E>>,
        tabs_handle: TabsHandle,
        active_handle: Arc<AtomicU64>,
        app_id: &'static str,
        tabs: Vec<SessionTab>,
        active_index: usize,
        sidebar_w: f32,
    ) -> Self {
        let mut app = Self {
            engine,
            slot,
            cmd_tx,
            tabs_handle,
            active_handle,
            cached_tabs: Vec::new(),
            cached_active: TabId(u64::MAX),
            url_field: String::new(),
            last_seen_url: String::new(),
            theme: sola_kit::theme::default_theme(),
            sidebar_w,
            dragging_divider: false,
            last_cursor_x: None,
            drag_anchor: None,
            hovered_tab: None,
            float: sola_kit::FloatState::new(app_id),
            window_id: None,
            app_id,
            url_bar_focused: false,
            session_fp: String::new(),
            pending_session: Some((tabs, active_index)),
            #[cfg(feature = "bitwarden")]
            vault: VaultHandle::spawn(),
            #[cfg(feature = "bitwarden")]
            vault_panel_open: false,
            #[cfg(feature = "bitwarden")]
            vault_phase: VaultPanelPhase::Credentials,
            #[cfg(feature = "bitwarden")]
            vault_email: String::new(),
            #[cfg(feature = "bitwarden")]
            vault_password: String::new(),
            #[cfg(feature = "bitwarden")]
            vault_pending_password: None,
            #[cfg(feature = "bitwarden")]
            vault_otp: String::new(),
            #[cfg(feature = "bitwarden")]
            vault_paste_target: VaultPasteTarget::Email,
            #[cfg(feature = "bitwarden")]
            vault_error: None,
            #[cfg(feature = "bitwarden")]
            vault_busy: false,
            #[cfg(feature = "bitwarden")]
            vault_status: VaultStatus::default(),
            #[cfg(feature = "bitwarden")]
            vault_matches: Vec::new(),
            #[cfg(feature = "bitwarden")]
            vault_matches_loading: false,
            #[cfg(feature = "bitwarden")]
            vault_matches_url: String::new(),
            // Distinct silhouettes: closed lock vs key (not keyhole open/closed).
            #[cfg(feature = "bitwarden")]
            vault_icon_locked: icon_handle("lucide/lock"),
            #[cfg(feature = "bitwarden")]
            vault_icon_unlocked: icon_handle("lucide/key-round"),
        };
        #[cfg(feature = "bitwarden")]
        {
            if let Some(email) = crate::vault::VaultPrefs::load_email() {
                app.vault_email = email;
                app.vault_paste_target = VaultPasteTarget::Password;
            }
        }
        app
    }

    #[cfg(feature = "bitwarden")]
    fn set_vault_panel_open(&mut self, open: bool) {
        self.vault_panel_open = open;
        // Subscription Tab handling reads this (fn-pointer listen_with).
        VAULT_PANEL_OPEN.store(open, Ordering::Relaxed);
        // Do **not** clear form / 2FA / pending password on dismiss — accidental
        // close must restore the same state when the lock icon is clicked again.
    }

    /// Ask the vault worker for logins matching the active tab URL.
    #[cfg(feature = "bitwarden")]
    fn request_vault_matches(&mut self) {
        if !self.vault_status.unlocked {
            self.vault_matches.clear();
            self.vault_matches_loading = false;
            return;
        }
        let url = self
            .active_tab_info()
            .map(|t| t.url.clone())
            .unwrap_or_default();
        self.vault_matches_url = url.clone();
        if url.is_empty() || url == BLANK_URL {
            self.vault_matches.clear();
            self.vault_matches_loading = false;
            return;
        }
        self.vault_matches_loading = true;
        self.vault.send(VaultCmd::Matches { url });
    }

    #[cfg(feature = "bitwarden")]
    fn clear_vault_secrets(&mut self) {
        self.vault_password.clear();
        if let Some(ref mut p) = self.vault_pending_password {
            p.zeroize();
        }
        self.vault_pending_password = None;
        self.vault_otp.clear();
    }

    /// Reset to a clean credentials form (after successful unlock, or explicit restart).
    #[cfg(feature = "bitwarden")]
    fn reset_vault_form_keep_email(&mut self) {
        self.clear_vault_secrets();
        self.vault_phase = VaultPanelPhase::Credentials;
        self.vault_error = None;
        self.vault_busy = false;
        self.vault_paste_target = if self.vault_email.trim().is_empty() {
            VaultPasteTarget::Email
        } else {
            VaultPasteTarget::Password
        };
    }

    #[cfg(feature = "bitwarden")]
    fn persist_vault_email(&self) {
        crate::vault::VaultPrefs::save_email(&self.vault_email);
    }

    /// Clear compositor env so WebKit child processes cannot open a Wayland
    /// window. Safe after iced has already connected (WindowReady).
    fn seal_wayland_from_webkit() {
        // Process-wide: WebProcess is forked from this process and inherits env.
        // Headless WPE does not need WAYLAND_DISPLAY; iced already holds its
        // wl_display connection from before this runs.
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            tracing::info!(
                "clearing WAYLAND_DISPLAY before creating WebViews (prevent phantom org.webkit toplevel)"
            );
            // SAFETY: single-threaded wrt other env writers at WindowReady.
            unsafe {
                std::env::remove_var("WAYLAND_DISPLAY");
            }
        }
    }

    /// Open restored tabs in order and focus `active_index`.
    fn bootstrap_tabs(&mut self, tabs: Vec<SessionTab>, active_index: usize) {
        debug_assert!(!tabs.is_empty(), "bootstrap always has ≥1 tab");
        let mut ids = Vec::with_capacity(tabs.len());
        for tab in tabs {
            let id = self.engine.alloc_tab_id();
            let url = crate::util::normalize_url(&tab.url);
            let url = if url.is_empty() {
                BLANK_URL.to_string()
            } else {
                url
            };
            // Optimistic chrome snapshot so the strip isn't empty for a tick.
            // Title from session is kept until WebKit reports a non-empty one.
            self.cached_tabs.push(TabInfo {
                id,
                url: url.clone(),
                title: tab.title.clone(),
                is_loading: !url.is_empty(),
                can_go_back: false,
                can_go_forward: false,
            });
            // One background frame may be imported to seed park cache.
            self.slot.need_park_prime.lock().unwrap().insert(id.0);
            let _ = self.cmd_tx.send(Cmd::OpenTab {
                id,
                url,
                title: tab.title,
            });
            ids.push(id);
        }
        let active = ids
            .get(active_index)
            .copied()
            .or_else(|| ids.first().copied())
            .unwrap_or(TabId(0));
        self.switch_active_tab(active);
        if let Some(info) = self.cached_tabs.iter().find(|t| t.id == active) {
            self.url_field = if info.url == BLANK_URL {
                String::new()
            } else {
                info.url.clone()
            };
            self.last_seen_url = info.url.clone();
        }
        self.persist_session();
        tracing::info!(
            tabs = self.cached_tabs.len(),
            active = active.0,
            "session restored"
        );
    }

    /// Write session to disk if the tab list / active / sidebar changed.
    pub fn persist_session(&mut self) {
        // Prefer engine snapshot when available (authoritative URLs/titles).
        let live = self.tabs_handle.lock().unwrap().clone();
        let tabs = if live.is_empty() {
            self.cached_tabs.clone()
        } else {
            live
        };
        if tabs.is_empty() {
            return;
        }
        let active = if tabs.iter().any(|t| t.id == self.cached_active) {
            self.cached_active
        } else {
            tabs[0].id
        };
        let session = session::session_from_tabs(&tabs, active, self.sidebar_w);
        let fp = session::fingerprint(&session);
        if fp == self.session_fp {
            return;
        }
        session.save();
        self.session_fp = fp;
    }

    pub fn active_tab_info(&self) -> Option<&TabInfo> {
        self.cached_tabs.iter().find(|t| t.id == self.cached_active)
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::WindowReady(id) => {
                self.window_id = id;
                // Iced is connected to the compositor. Strip WAYLAND_DISPLAY
                // before any WebKitWebView exists so WPEWebProcess cannot map
                // its own xdg_toplevel (app_id org.webkit.*).
                if let Some((tabs, active_index)) = self.pending_session.take() {
                    Self::seal_wayland_from_webkit();
                    self.bootstrap_tabs(tabs, active_index);
                }
                return Task::none();
            }
            Msg::TitleDrag => return sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => return sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => {
                sola_kit::close_app(self.app_id);
                return Task::none();
            }
            Msg::VaultToggle => {
                #[cfg(feature = "bitwarden")]
                {
                    let open = !self.vault_panel_open;
                    self.set_vault_panel_open(open);
                    if open {
                        self.vault_error = None;
                        if self.vault_status.unlocked {
                            self.request_vault_matches();
                            return Task::none();
                        }
                        // Prefill email when remembered; land caret on password.
                        if !self.vault_email.trim().is_empty() {
                            self.vault_paste_target = VaultPasteTarget::Password;
                            return iced::widget::operation::focus(vault_password_id());
                        }
                        return iced::widget::operation::focus(vault_email_id());
                    }
                }
            }
            Msg::VaultFill(id) => {
                #[cfg(feature = "bitwarden")]
                {
                    if self.vault_busy || !self.vault_status.unlocked {
                        return Task::none();
                    }
                    self.vault_busy = true;
                    self.vault_error = None;
                    self.vault.send(VaultCmd::Fill { id });
                }
            }
            Msg::VaultRefreshMatches => {
                #[cfg(feature = "bitwarden")]
                {
                    self.request_vault_matches();
                }
            }
            Msg::VaultEmail(s) => {
                #[cfg(feature = "bitwarden")]
                {
                    self.vault_email = s;
                    self.vault_paste_target = VaultPasteTarget::Email;
                }
            }
            Msg::VaultPassword(s) => {
                #[cfg(feature = "bitwarden")]
                {
                    self.vault_password = s;
                    self.vault_paste_target = VaultPasteTarget::Password;
                }
            }
            Msg::VaultOtp(s) => {
                #[cfg(feature = "bitwarden")]
                {
                    self.vault_otp = s;
                    self.vault_paste_target = VaultPasteTarget::Otp;
                }
            }
            Msg::VaultLogin => {
                #[cfg(feature = "bitwarden")]
                {
                    if self.vault_busy {
                        return Task::none();
                    }
                    let email = self.vault_email.trim().to_string();
                    let password = self.vault_password.clone();
                    if email.is_empty() || password.is_empty() {
                        self.vault_error =
                            Some("Email and master password are required.".into());
                        return Task::none();
                    }
                    self.vault_busy = true;
                    self.vault_error = None;
                    // Keep a copy for 2FA / resend until unlock completes.
                    self.vault_pending_password = Some(password.clone());
                    self.vault.send(VaultCmd::Login { email, password });
                    self.vault_password.clear();
                }
            }
            Msg::VaultVerifyOtp => {
                #[cfg(feature = "bitwarden")]
                {
                    if self.vault_busy {
                        return Task::none();
                    }
                    // Strip spaces / newlines that sneak in via paste.
                    let token: String = self
                        .vault_otp
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect();
                    if token.is_empty() {
                        self.vault_error = Some("Enter the verification code.".into());
                        return Task::none();
                    }
                    let Some(password) = self.vault_pending_password.clone() else {
                        self.vault_error =
                            Some("Session expired — enter your password again.".into());
                        self.vault_phase = VaultPanelPhase::Credentials;
                        return Task::none();
                    };
                    let kind = match &self.vault_phase {
                        VaultPanelPhase::TwoFactor { kind, .. } => *kind,
                        VaultPanelPhase::Credentials => {
                            self.vault_error = Some("Enter email and password first.".into());
                            return Task::none();
                        }
                    };
                    let email = self.vault_email.trim().to_string();
                    self.vault_busy = true;
                    self.vault_error = None;
                    self.vault.send(VaultCmd::LoginTwoFactor {
                        email,
                        password,
                        token,
                        kind,
                        remember: true,
                    });
                }
            }
            Msg::VaultResendEmailCode => {
                #[cfg(feature = "bitwarden")]
                {
                    if self.vault_busy {
                        return Task::none();
                    }
                    let Some(password) = self.vault_pending_password.clone() else {
                        self.vault_error =
                            Some("Session expired — enter your password again.".into());
                        self.vault_phase = VaultPanelPhase::Credentials;
                        return Task::none();
                    };
                    let kind = match &self.vault_phase {
                        VaultPanelPhase::TwoFactor { kind, .. } => *kind,
                        VaultPanelPhase::Credentials => {
                            self.vault_error = Some("Enter email and password first.".into());
                            return Task::none();
                        }
                    };
                    let email = self.vault_email.trim().to_string();
                    self.vault_busy = true;
                    self.vault_error = None;
                    self.vault.send(VaultCmd::ResendEmailCode {
                        email,
                        password,
                        kind,
                    });
                }
            }
            Msg::VaultPanelClose => {
                #[cfg(feature = "bitwarden")]
                {
                    self.set_vault_panel_open(false);
                }
            }
            Msg::VaultFocusNext => {
                return iced::widget::operation::focus_next();
            }
            Msg::VaultFocusPrev => {
                return iced::widget::operation::focus_previous();
            }
            #[cfg(feature = "bitwarden")]
            Msg::VaultWorker(ev) => {
                self.handle_vault_event(ev);
            }
            Msg::NewFrame => {
                // Allow the next frame stream wakeup (coalesced redraw).
                self.slot
                    .redraw_queued
                    .store(false, std::sync::atomic::Ordering::Release);
            }
            Msg::NavBack => {
                self.set_active_loading(true);
                let _ = self.cmd_tx.send(Cmd::Nav(NavCmd::Back));
            }
            Msg::NavForward => {
                self.set_active_loading(true);
                let _ = self.cmd_tx.send(Cmd::Nav(NavCmd::Forward));
            }
            Msg::NavReloadOrStop => {
                if self.active_is_loading() {
                    self.set_active_loading(false);
                    let _ = self.cmd_tx.send(Cmd::Nav(NavCmd::Stop));
                } else {
                    self.set_active_loading(true);
                    let _ = self.cmd_tx.send(Cmd::Nav(NavCmd::Reload));
                }
            }
            Msg::NavStop => {
                #[cfg(feature = "bitwarden")]
                if self.vault_panel_open {
                    // Dismiss panel only — keep login / 2FA state for re-open.
                    self.set_vault_panel_open(false);
                    return Task::none();
                }
                self.set_active_loading(false);
                let _ = self.cmd_tx.send(Cmd::Nav(NavCmd::Stop));
            }
            Msg::UrlInput(s) => {
                self.url_field = s;
                self.url_bar_focused = true;
            }
            Msg::UrlSubmit => {
                // A URL navigates directly; anything else is searched on Kagi.
                let url = crate::util::resolve_query(&self.url_field);
                if url.is_empty() {
                    return Task::none();
                }
                self.url_field = url.clone();
                self.last_seen_url = url.clone();
                // Optimistic: update cached tab url so session persists immediately.
                if let Some(t) = self
                    .cached_tabs
                    .iter_mut()
                    .find(|t| t.id == self.cached_active)
                {
                    t.url = url.clone();
                    t.is_loading = true;
                }
                let _ = self.cmd_tx.send(Cmd::Nav(NavCmd::LoadUrl(url)));
                self.persist_session();
            }
            Msg::CloseTab(id) => {
                // Never drop below one tab: open a blank replacement first.
                // (Bus-integration last-tab contract: never drop below one tab.)
                if self.cached_tabs.len() <= 1 {
                    self.open_tab(BLANK_URL.to_string(), true);
                }
                // If closing the active tab, pick a new active tab
                // first so the engine never sees `active` pointing
                // at a closed tab.
                let was_active = self.cached_active == id;
                if was_active {
                    if let Some(new_active) = self.pick_new_active_after_close(id) {
                        self.switch_active_tab(new_active);
                    }
                }
                // Release any parked GPU frame for this tab on next prepare.
                self.slot.drop_paint_tabs.lock().unwrap().push(id.0);
                // Drop a queued frame for the closed tab so prepare cannot
                // re-park a dead surface after the drop list is drained.
                {
                    let mut pending = self.slot.pending.lock().unwrap();
                    if pending.as_ref().is_some_and(|p| p.tab_id == id) {
                        *pending = None;
                    }
                }
                self.slot.need_park_prime.lock().unwrap().remove(&id.0);
                let _ = self.cmd_tx.send(Cmd::CloseTab(id));
                // Drop from optimistic cache immediately so persist sees it.
                self.cached_tabs.retain(|t| t.id != id);
                self.persist_session();
            }
            Msg::ActivateTab(id) => {
                self.switch_active_tab(id);
                self.persist_session();
            }
            Msg::Tick => {
                #[cfg(feature = "bitwarden")]
                let mut focus_otp = false;
                #[cfg(feature = "bitwarden")]
                {
                    while let Some(ev) = self.vault.try_recv() {
                        if matches!(ev, VaultEvent::LoginNeedsTwoFactor { .. }) {
                            focus_otp = true;
                        }
                        self.handle_vault_event(ev);
                    }
                }
                // Merge engine snapshot with prior cache: WebKit often reports
                // empty title until the page finishes loading (esp. inactive
                // restored tabs). Keep the last known title so the strip does
                // not blank out after session restore.
                let live = self.tabs_handle.lock().unwrap().clone();
                if !live.is_empty() {
                    self.cached_tabs = merge_tab_snapshot(&self.cached_tabs, &live);
                }
                // Chrome `paint_tab` is the strip/omnibox authority. The
                // worker `active_handle` can lag a pump tick behind and was
                // clobbering optimistic activate (new-tab had no highlight).
                let paint = self.slot.paint_tab.load(Ordering::Relaxed);
                if paint != u64::MAX {
                    self.cached_active = TabId(paint);
                } else {
                    let engine_active = TabId(self.active_handle.load(Ordering::Relaxed));
                    if engine_active.0 != u64::MAX {
                        self.cached_active = engine_active;
                    }
                }
                let active_url = self.active_tab_info().map(|t| t.url.clone());
                if let Some(url) = active_url {
                    if url != self.last_seen_url {
                        self.last_seen_url = url.clone();
                        // A blank tab shows an empty URL bar, not "about:blank".
                        self.url_field = if url == BLANK_URL { String::new() } else { url };
                    }
                }
                self.persist_session();
                // Drain any page-selection text the engine extracted for a copy
                // and put it on the system clipboard via iced. The engine's own
                // clipboard can't reach Wayland (headless display); iced's can.
                if let Some(text) = self.engine.clipboard_handle().lock().unwrap().take() {
                    tracing::debug!(len = text.len(), "draining page selection → system clipboard");
                    #[cfg(feature = "bitwarden")]
                    if focus_otp {
                        return Task::batch([
                            iced::clipboard::write(text),
                            iced::widget::operation::focus(vault_otp_id()),
                        ]);
                    }
                    return iced::clipboard::write(text);
                }
                #[cfg(feature = "bitwarden")]
                if focus_otp {
                    return iced::widget::operation::focus(vault_otp_id());
                }
            }
            Msg::Bus(message) => {
                return crate::integration::handle_bus(self, message, self.app_id);
            }
            Msg::DividerPress => {
                self.dragging_divider = true;
                if let Some(x) = self.last_cursor_x {
                    self.drag_anchor = Some((x, self.sidebar_w));
                }
            }
            Msg::CursorMoved(x) => {
                self.last_cursor_x = Some(x);
                if self.dragging_divider {
                    if let Some((anchor_x, anchor_w)) = self.drag_anchor {
                        // Sidebar is on the LEFT: it grows as the cursor
                        // moves right of the anchor, shrinks moving left.
                        let desired = anchor_w + (x - anchor_x);
                        self.sidebar_w = desired.clamp(SIDEBAR_W_MIN, SIDEBAR_W_MAX);
                    }
                }
            }
            // sidebar width persisted on Tick / drop
            Msg::CursorReleased => {
                if self.dragging_divider {
                    self.dragging_divider = false;
                    self.drag_anchor = None;
                }
            }
            Msg::TabHover(i) => self.hovered_tab = i,
            Msg::WebViewFocused => self.url_bar_focused = false,
            Msg::LeftPressed => {
                // A press landed somewhere. Resolve, against the real widget
                // tree, whether it focused the URL bar — `text_input` captures
                // the click so no wrapper can tell us directly.
                return crate::integration::url_bar_is_focused(Msg::UrlBarFocusSync);
            }
            Msg::UrlBarFocusSync(now) => {
                // Select-all only on the false→true edge, so a second click in
                // an already-focused field can place the caret normally.
                let gained = now && !self.url_bar_focused;
                self.url_bar_focused = now;
                if gained {
                    return crate::integration::select_url_bar();
                }
            }
            Msg::EditRouted { cmd, url_bar_focused } => {
                // Vault panel owns Edit shortcuts while open — shell grabs ⌘V
                // globally and would otherwise paste into the page.
                #[cfg(feature = "bitwarden")]
                if self.vault_panel_open {
                    tracing::debug!(?cmd, "edit → vault panel");
                    return match cmd {
                        EditCmd::Paste => {
                            iced::clipboard::read().map(Msg::VaultClipboardPaste)
                        }
                        EditCmd::SelectAll => match self.vault_paste_target {
                            VaultPasteTarget::Email => {
                                iced::widget::operation::focus(vault_email_id())
                            }
                            VaultPasteTarget::Password => {
                                iced::widget::operation::focus(vault_password_id())
                            }
                            VaultPasteTarget::Otp => {
                                iced::widget::operation::focus(vault_otp_id())
                            }
                        },
                        EditCmd::Copy => match self.vault_paste_target {
                            VaultPasteTarget::Email => {
                                iced::clipboard::write(self.vault_email.clone())
                            }
                            VaultPasteTarget::Password => Task::none(),
                            VaultPasteTarget::Otp => {
                                iced::clipboard::write(self.vault_otp.clone())
                            }
                        },
                        EditCmd::Cut | EditCmd::Undo | EditCmd::Redo => Task::none(),
                    };
                }
                if url_bar_focused {
                    tracing::debug!(?cmd, "edit → URL bar (iced clipboard)");
                    return match cmd {
                        EditCmd::Copy => iced::clipboard::write(self.url_field.clone()),
                        EditCmd::Cut => {
                            let task = iced::clipboard::write(self.url_field.clone());
                            self.url_field.clear();
                            task
                        }
                        EditCmd::Paste => iced::clipboard::read().map(Msg::UrlPasted),
                        EditCmd::SelectAll => crate::integration::select_url_bar(),
                        // The URL bar has no app-level undo/redo stack.
                        EditCmd::Undo | EditCmd::Redo => Task::none(),
                    };
                }
                tracing::debug!(?cmd, "edit → engine (web content)");
                // Paste-into-page: read iced's Wayland clipboard and ship the
                // text (WPE headless has no clipboard backend).
                if cmd == EditCmd::Paste {
                    return iced::clipboard::read().map(Msg::PagePasted);
                }
                let _ = self.cmd_tx.send(Cmd::Edit(cmd));
            }
            Msg::UrlPasted(text) => {
                if let Some(s) = text {
                    // Best-effort: iced exposes no caret/selection, so append
                    // at the end (cursor-at-end assumption).
                    self.url_field.push_str(&s);
                }
            }
            Msg::PagePasted(text) => {
                if let Some(s) = text {
                    let _ = self.cmd_tx.send(Cmd::PasteText(s));
                }
            }
            #[cfg(feature = "bitwarden")]
            Msg::VaultClipboardPaste(text) => {
                let Some(raw) = text else {
                    return Task::none();
                };
                let cleaned: String = raw.chars().filter(|c| !c.is_control()).collect();
                match self.vault_phase {
                    VaultPanelPhase::TwoFactor { .. } => {
                        self.vault_otp =
                            cleaned.chars().filter(|c| !c.is_whitespace()).collect();
                        self.vault_paste_target = VaultPasteTarget::Otp;
                    }
                    VaultPanelPhase::Credentials => match self.vault_paste_target {
                        VaultPasteTarget::Email => {
                            self.vault_email = cleaned;
                            self.vault_paste_target = VaultPasteTarget::Email;
                        }
                        VaultPasteTarget::Password | VaultPasteTarget::Otp => {
                            self.vault_password = cleaned;
                            self.vault_paste_target = VaultPasteTarget::Password;
                        }
                    },
                }
            }
        }
        Task::none()
    }

    /// Open a new tab loading `url`, focusing it when `activate`. Called from
    /// app-menu intents (e.g., ⌘T for new tab) and bus-driven OpenUrl via
    /// `integration::run_intent`.
    pub fn open_tab(&mut self, url: String, activate: bool) {
        let url = crate::util::normalize_url(&url);
        let id = self.engine.alloc_tab_id();
        let title = if url == BLANK_URL {
            "New Tab".to_string()
        } else {
            String::new()
        };
        self.cached_tabs.push(TabInfo {
            id,
            url: url.clone(),
            title: title.clone(),
            is_loading: !url.is_empty(),
            can_go_back: false,
            can_go_forward: false,
        });
        if !activate {
            // Background open (e.g. cmd-click): allow one park prime frame.
            self.slot.need_park_prime.lock().unwrap().insert(id.0);
        }
        let _ = self.cmd_tx.send(Cmd::OpenTab { id, url, title });
        if activate {
            self.switch_active_tab(id);
        }
        self.persist_session();
    }

    pub fn active_is_loading(&self) -> bool {
        self.active_tab_info()
            .map(|t| t.is_loading)
            .unwrap_or(false)
    }

    fn set_active_loading(&mut self, loading: bool) {
        if let Some(t) = self
            .cached_tabs
            .iter_mut()
            .find(|t| t.id == self.cached_active)
        {
            t.is_loading = loading;
        }
    }

    /// Switch which tab paints: update chrome state, drop any queued
    /// frame for the previous tab, and ask the worker to activate.
    /// Without clearing `pending` / `paint_tab`, the shader keeps
    /// sampling the previous tab's texture until a new frame arrives
    /// (and static pages may never produce one).
    pub fn switch_active_tab(&mut self, id: TabId) {
        self.cached_active = id;
        self.slot
            .paint_tab
            .store(id.0, std::sync::atomic::Ordering::Relaxed);
        // Do not clear pending: background frames still update park cache.
        let _ = self.cmd_tx.send(Cmd::SetActiveTab(id));
        // Omnibox follows chrome optimistically — don't wait for Tick
        // (250ms) or the worker URI notify, which feels laggy on tab click.
        if let Some(info) = self.cached_tabs.iter().find(|t| t.id == id) {
            self.url_field = if info.url == BLANK_URL {
                String::new()
            } else {
                info.url.clone()
            };
            self.last_seen_url = info.url.clone();
        } else {
            // Unknown id (shouldn't happen): force Tick to refresh.
            self.last_seen_url.clear();
        }
    }

    /// Current iced theme (chrome styling), refreshed from `Topic::Theme`.
    pub fn theme(&self) -> iced::Theme {
        sola_kit::theme_for(self.float.is_floating_any(), &self.theme)
    }

    pub fn pick_new_active_after_close(&self, closing: TabId) -> Option<TabId> {
        let idx = self.cached_tabs.iter().position(|t| t.id == closing)?;
        // Prefer the right neighbour (like every desktop browser);
        // fall back to the left if closing was last.
        self.cached_tabs
            .get(idx + 1)
            .or_else(|| {
                if idx == 0 {
                    None
                } else {
                    self.cached_tabs.get(idx - 1)
                }
            })
            .map(|t| t.id)
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let webview = Shader::new(E::make_program(self.slot.clone()))
            .width(Length::Fill)
            .height(Length::Fill);

        // Right side: nav bar on top of the web content.
        let content = column![self.view_nav_bar(), horizontal_divider(), webview];

        // Left tab column (resizable) | divider | content.
        let main = row![
            container(self.view_tab_sidebar())
                .width(Length::Fixed(self.sidebar_w))
                .height(Length::Fill),
            vertical_divider_with(
                Msg::DividerPress,
                sola_kit::components::DividerColors::raised_to_canvas(&self.theme),
            ),
            container(content).width(Length::Fill).height(Length::Fill),
        ]
        .height(Length::Fill);

        let body: Element<'_, Msg> = main.into();

        // While dragging, a transparent top layer holds the resize
        // cursor steady even when the pointer races ahead of the divider.
        let content: Element<'_, Msg> = if self.dragging_divider {
            stack![
                body,
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .interaction(mouse::Interaction::ResizingColumn),
            ]
            .into()
        } else {
            body
        };

        #[cfg(feature = "bitwarden")]
        let content: Element<'_, Msg> = if self.vault_panel_open {
            stack![content, self.view_vault_panel()].into()
        } else {
            content
        };

        sola_kit::wrap_if_floating(
            self.float.is_floating_any(),
            "Browser",
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            content,
        )
    }

    /// Left vertical tab column, built from the kit `vertical_tabs`
    /// component so it tracks the shared theme. Single-line labels (no
    /// wrap), active-row highlight, and a close `×` that floats in on
    /// hover. New tabs come from `⌘T` (the app-menu shortcut), so there's
    /// no in-column "+" button.
    pub fn view_tab_sidebar(&self) -> Element<'_, Msg> {
        let tabs: Vec<TabDescriptor<Msg>> = self
            .cached_tabs
            .iter()
            .map(|t| {
                let label = if !t.title.is_empty() {
                    crate::util::truncate(&t.title, 20)
                } else if !t.url.is_empty() {
                    crate::util::truncate(&t.url, 20)
                } else {
                    String::from("Loading…")
                };
                let active_id = {
                    let paint = self.slot.paint_tab.load(Ordering::Relaxed);
                    if paint != u64::MAX {
                        TabId(paint)
                    } else {
                        self.cached_active
                    }
                };
                TabDescriptor::new(
                    label,
                    t.id == active_id,
                    Msg::ActivateTab(t.id),
                    Msg::CloseTab(t.id),
                )
            })
            .collect();

        vertical_tabs_sized(tabs, self.hovered_tab, Msg::TabHover, TabSize::Large).into()
    }

    /// Top navigation bar: back / forward / reload·stop + the URL field. All
    /// widgets are kit-styled, so they track the bus theme.
    ///
    /// The URL field isn't wrapped in a `mouse_area`: `text_input` captures
    /// the click to place its caret, and `mouse_area` skips `on_press` for
    /// captured events. Click-into-focus + select-all is handled instead via
    /// the global press subscription (`Msg::LeftPressed`) plus a live focus
    /// query, which sees the press regardless of widget capture.
    pub fn view_nav_bar(&self) -> Element<'_, Msg> {
        use sola_kit::components::style::{SPACE_MD, SPACE_SM};
        // Fixed slot so ↻ ↔ × does not reflow the omnibox.
        const NAV_BTN_W: f32 = 36.0;
        let info = self.active_tab_info();
        let can_back = info.map(|t| t.can_go_back).unwrap_or(false);
        let can_fwd = info.map(|t| t.can_go_forward).unwrap_or(false);
        // No `on_press` → iced marks Disabled (muted by toolbar style).
        let back = {
            let b = toolbar_button("←").width(Length::Fixed(NAV_BTN_W));
            if can_back {
                b.on_press(Msg::NavBack)
            } else {
                b
            }
        };
        let forward = {
            let b = toolbar_button("→").width(Length::Fixed(NAV_BTN_W));
            if can_fwd {
                b.on_press(Msg::NavForward)
            } else {
                b
            }
        };
        // Reload when idle; × stops an in-flight load (Escape also stops).
        let reload_icon = if self.active_is_loading() { "×" } else { "↻" };
        let reload_or_stop = toolbar_button(reload_icon)
            .width(Length::Fixed(NAV_BTN_W))
            .on_press(Msg::NavReloadOrStop);
        // Bitwarden: locked = closed lock (muted); unlocked = key (accent).
        // Different shapes + color so state is obvious at a glance.
        #[cfg(feature = "bitwarden")]
        let vault_btn = {
            let unlocked = self.vault_status.unlocked;
            let handle = if unlocked {
                self.vault_icon_unlocked.clone()
            } else {
                self.vault_icon_locked.clone()
            };
            let icon = if unlocked {
                sola_kit::components::icon::icon_svg_colored(
                    handle,
                    18,
                    // Accent reads as “vault ready” against chrome.
                    self.theme.extended_palette().primary.base.color,
                )
            } else {
                sola_kit::components::icon::icon_svg_colored(
                    handle,
                    18,
                    {
                        let t = self.theme.extended_palette().background.base.text;
                        iced::Color {
                            a: 0.55,
                            ..t
                        }
                    },
                )
            };
            button(icon)
                .padding(PAD_CONTROL_SM)
                .width(Length::Fixed(NAV_BTN_W))
                .style(if unlocked {
                    vault_toolbar_btn_unlocked
                } else {
                    kit_toolbar::style
                })
                .on_press(Msg::VaultToggle)
        };
        #[cfg(not(feature = "bitwarden"))]
        let vault_btn = Space::new().width(Length::Fixed(0.0));

        row![
            back,
            forward,
            reload_or_stop,
            // Kit body density (13) + DEFAULT_PADDING — chrome inherits tokens.
            text_input("Search or enter URL", &self.url_field)
                .id(crate::integration::url_input_id())
                .on_input(Msg::UrlInput)
                .on_submit(Msg::UrlSubmit)
                .size(13)
                .style(sola_kit::components::text_input::style)
                .width(Length::Fill),
            vault_btn,
        ]
        .spacing(SPACE_MD)
        .padding([SPACE_SM, SPACE_MD + SPACE_SM])
        .align_y(iced::Alignment::Center)
        .height(Length::Fixed(CHROME_HEIGHT))
        .into()
    }

    #[cfg(feature = "bitwarden")]
    fn handle_vault_event(&mut self, ev: VaultEvent) {
        match ev {
            VaultEvent::Status(s) => {
                self.vault_status = s;
            }
            VaultEvent::LoginOk { email } => {
                self.vault_email = email;
                self.persist_vault_email();
                self.reset_vault_form_keep_email();
                // Unlock is done — dismiss the panel. User opens it again
                // (key icon) to pick a login for the current page.
                self.set_vault_panel_open(false);
            }
            VaultEvent::LoginNeedsTwoFactor {
                email,
                preferred,
                email_hint,
                email_sent,
                ..
            } => {
                tracing::info!(
                    ?preferred,
                    email_sent,
                    "vault ui: switching to verification code entry"
                );
                self.vault_busy = false;
                self.vault_email = email;
                self.vault_otp.clear();
                self.vault_phase = VaultPanelPhase::TwoFactor {
                    kind: preferred,
                    email_hint,
                };
                self.vault_paste_target = VaultPasteTarget::Otp;
                // Subtitle covers email/TOTP instructions; keep error clear.
                let _ = email_sent;
                self.vault_error = None;
            }
            VaultEvent::LoginFailed { message } => {
                self.vault_busy = false;
                self.vault_error = Some(message);
            }
            VaultEvent::EmailCodeSent => {
                self.vault_busy = false;
                self.vault_error = None;
                self.vault_otp.clear();
            }
            VaultEvent::EmailCodeFailed { message } => {
                self.vault_busy = false;
                self.vault_error = Some(format!("Could not send code: {message}"));
            }
            VaultEvent::SyncOk { full } => {
                tracing::info!(full, "vault: sync ok");
                // Only refresh matches if the user has the fill panel open.
                if self.vault_panel_open && self.vault_status.unlocked {
                    self.request_vault_matches();
                }
            }
            VaultEvent::SyncFailed { message } => {
                tracing::warn!(%message, "vault: sync failed");
                if self.vault_panel_open && self.vault_status.unlocked {
                    self.vault_error = Some(format!("Signed in, but sync failed: {message}"));
                }
            }
            VaultEvent::Matches(list) => {
                self.vault_matches_loading = false;
                tracing::info!(n = list.len(), url = %self.vault_matches_url, "vault: matches");
                self.vault_matches = list;
            }
            VaultEvent::FillReady {
                mut username,
                mut password,
            } => {
                self.vault_busy = false;
                let script =
                    fill_credentials_script(username.as_deref(), password.as_deref());
                if let Some(ref mut p) = password {
                    p.zeroize();
                }
                if let Some(ref mut u) = username {
                    u.zeroize();
                }
                let _ = self.cmd_tx.send(Cmd::EvaluateJs(script));
                tracing::info!("vault: fill injected into active page");
                // Close panel so the user can submit the form immediately.
                self.set_vault_panel_open(false);
            }
            VaultEvent::Error { message } => {
                tracing::warn!(%message, "vault: error");
                self.vault_busy = false;
                self.vault_matches_loading = false;
                if self.vault_panel_open {
                    self.vault_error = Some(message);
                }
            }
        }
    }

    /// Bitwarden panel anchored top-right under the toolbar vault icon.
    #[cfg(feature = "bitwarden")]
    fn view_vault_panel(&self) -> Element<'_, Msg> {
        use sola_kit::components::style::{SPACE_MD, SPACE_SM};

        // Secondary.base on the modal face is near-invisible. Soften primary
        // text instead so body copy stays readable (≥~4.5:1 intent).
        let soft = |s: String| {
            text(s).size(12).style(|theme: &iced::Theme| {
                let t = theme.extended_palette().background.base.text;
                iced::widget::text::Style {
                    color: Some(iced::Color { a: 0.72, ..t }),
                }
            })
        };
        let soft_sm = |s: String| {
            text(s).size(11).style(|theme: &iced::Theme| {
                let t = theme.extended_palette().background.base.text;
                iced::widget::text::Style {
                    color: Some(iced::Color { a: 0.62, ..t }),
                }
            })
        };

        let err_line = self.vault_error.as_ref().map(|err| {
            text(err.clone())
                .size(12)
                .style(|theme: &iced::Theme| iced::widget::text::Style {
                    color: Some(theme.extended_palette().danger.base.color),
                })
        });

        let body: Element<'_, Msg> = if self.vault_status.unlocked {
            // Fill picker only — not a status card. Unlock already closed the panel.
            let title = text("Fill login")
                .size(15)
                .font(sola_kit::fonts::ui_medium());

            let page_url = if self.vault_matches_url.is_empty() {
                self.active_tab_info()
                    .map(|t| t.url.as_str())
                    .unwrap_or("")
            } else {
                self.vault_matches_url.as_str()
            };
            let host_hint = page_host_hint(page_url);

            let mut col = column![title]
                .spacing(SPACE_SM)
                .width(Length::Fixed(300.0));

            if !host_hint.is_empty() {
                col = col.push(soft(host_hint));
            }

            if let Some(err) = err_line {
                col = col.push(err);
            }

            if self.vault_matches_loading {
                col = col.push(text("Looking up logins…").size(13));
            } else if page_url.is_empty() || page_url == BLANK_URL {
                col = col.push(text("Open a website to fill a login.").size(13));
            } else if self.vault_matches.is_empty() {
                col = col.push(text("No saved logins for this page.").size(13));
                col = col.push(soft_sm(
                    "Add one in Bitwarden, then Refresh.".into(),
                ));
            } else {
                let mut list = column![].spacing(4.0);
                for m in &self.vault_matches {
                    let title_line = if m.name.is_empty() {
                        "Login".to_string()
                    } else {
                        m.name.clone()
                    };
                    let sub = m
                        .username
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("—");
                    let row_body = column![
                        text(title_line).size(13).font(sola_kit::fonts::ui_medium()),
                        soft_sm(sub.to_string()),
                    ]
                    .spacing(2);
                    let id = m.id.clone();
                    let mut btn = button(row_body)
                        .padding(Padding::from([8, 10]))
                        .width(Length::Fill)
                        .style(|theme: &iced::Theme, status| {
                            let p = theme.extended_palette();
                            let bg = match status {
                                iced::widget::button::Status::Hovered
                                | iced::widget::button::Status::Pressed => p.background.strong.color,
                                _ => p.background.weak.color,
                            };
                            iced::widget::button::Style {
                                background: Some(iced::Background::Color(bg)),
                                text_color: p.background.base.text,
                                border: iced::Border {
                                    color: p.background.strong.color,
                                    width: 1.0,
                                    radius: 8.0.into(),
                                },
                                ..Default::default()
                            }
                        });
                    if !self.vault_busy {
                        btn = btn.on_press(Msg::VaultFill(id));
                    }
                    list = list.push(btn);
                }
                col = col.push(
                    scrollable(list)
                        .height(Length::Fixed(200.0))
                        .width(Length::Fill),
                );
            }

            let mut refresh = kit_button::labeled("Refresh", kit_button::ghost);
            if !self.vault_busy && !self.vault_matches_loading {
                refresh = refresh.on_press(Msg::VaultRefreshMatches);
            }
            let close = kit_button::labeled("Close", kit_button::secondary)
                .on_press(Msg::VaultPanelClose);
            col = col.push(
                row![refresh, close]
                    .spacing(SPACE_SM)
                    .align_y(Alignment::Center),
            );
            col.into()
        } else {
            match &self.vault_phase {
                VaultPanelPhase::Credentials => {
                    // While a request is in flight, freeze the form (no on_input
                    // ⇒ disabled). Cancel stays active so the panel can dismiss
                    // without losing state.
                    //
                    // Remembered email → prefill the same Email field and focus
                    // password on open (no alternate layout / switch-account link).
                    let busy = self.vault_busy;
                    let title = text("Unlock vault")
                        .size(15)
                        .font(sola_kit::fonts::ui_medium());

                    let mut email = text_input("Email", &self.vault_email)
                        .id(vault_email_id())
                        .size(13)
                        .style(sola_kit::components::text_input::style)
                        .width(Length::Fill);
                    let mut password = text_input("Master password", &self.vault_password)
                        .id(vault_password_id())
                        .secure(true)
                        .size(13)
                        .style(sola_kit::components::text_input::style)
                        .width(Length::Fill);
                    if !busy {
                        email = email.on_input(Msg::VaultEmail).on_submit(Msg::VaultLogin);
                        password = password
                            .on_input(Msg::VaultPassword)
                            .on_submit(Msg::VaultLogin);
                    }

                    let mut login_btn = kit_button::labeled(
                        if busy { "Unlocking…" } else { "Unlock" },
                        kit_button::primary,
                    );
                    if !busy {
                        login_btn = login_btn.on_press(Msg::VaultLogin);
                    }
                    // Cancel always enabled — closes panel, keeps form state.
                    let cancel = kit_button::labeled("Cancel", kit_button::ghost)
                        .on_press(Msg::VaultPanelClose);

                    let mut col = column![
                        title,
                        soft("Bitwarden".into()),
                        Space::new().height(SPACE_SM),
                        email,
                        password,
                    ]
                    .spacing(SPACE_SM)
                    .width(Length::Fixed(300.0));

                    if let Some(err) = err_line {
                        col = col.push(err);
                    }
                    col = col.push(
                        row![login_btn, cancel]
                            .spacing(SPACE_SM)
                            .align_y(Alignment::Center),
                    );
                    col.into()
                }
                VaultPanelPhase::TwoFactor { kind, email_hint } => {
                    let busy = self.vault_busy;
                    let title = text("Verify")
                        .size(15)
                        .font(sola_kit::fonts::ui_medium());
                    let hint = email_hint
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("your email");
                    let (subtitle, placeholder, show_resend) = match kind {
                        // New-device protection emails a code automatically on the
                        // password grant; complete with form field `newDeviceOtp`.
                        TwoFactorKind::NewDevice => (
                            format!(
                                "Enter the code Bitwarden emailed to {hint}."
                            ),
                            "Verification code",
                            true,
                        ),
                        TwoFactorKind::Email => (
                            format!("Enter the code sent to {hint}."),
                            "Email verification code",
                            true,
                        ),
                        TwoFactorKind::Authenticator => (
                            "Enter the code from your authenticator app.".into(),
                            "Authenticator code",
                            false,
                        ),
                    };

                    let mut otp = text_input(placeholder, &self.vault_otp)
                        .id(vault_otp_id())
                        .size(13)
                        .style(sola_kit::components::text_input::style)
                        .width(Length::Fill);
                    if !busy {
                        otp = otp.on_input(Msg::VaultOtp).on_submit(Msg::VaultVerifyOtp);
                    }

                    let mut verify_btn = kit_button::labeled(
                        if busy { "Verifying…" } else { "Verify" },
                        kit_button::primary,
                    );
                    if !busy {
                        verify_btn = verify_btn.on_press(Msg::VaultVerifyOtp);
                    }
                    let cancel = kit_button::labeled("Cancel", kit_button::ghost)
                        .on_press(Msg::VaultPanelClose);

                    let mut col = column![
                        title,
                        soft(subtitle),
                        Space::new().height(SPACE_SM),
                        otp,
                    ]
                    .spacing(SPACE_SM)
                    .width(Length::Fixed(300.0));

                    if let Some(err) = err_line {
                        col = col.push(err);
                    }

                    let mut actions = row![verify_btn].spacing(SPACE_SM);
                    if show_resend {
                        let mut resend =
                            kit_button::labeled("Resend", kit_button::ghost);
                        if !busy {
                            resend = resend.on_press(Msg::VaultResendEmailCode);
                        }
                        actions = actions.push(resend);
                    }
                    actions = actions.push(cancel);
                    col = col.push(actions.align_y(Alignment::Center));
                    col.into()
                }
            }
        };

        // Fixed-width card — do not let modal face stretch to the window.
        let panel = card::modal(container(body).padding(SPACE_MD + SPACE_SM))
            .width(Length::Fixed(320.0));

        // Light click-away (no full dim wash — popover by the icon).
        let backdrop = mouse_area(
            container(Space::new().width(Length::Fill).height(Length::Fill)).style(|_t| {
                container::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgba(
                        0.0, 0.0, 0.0, 0.12,
                    ))),
                    ..container::Style::default()
                }
            }),
        )
        .on_press(Msg::VaultPanelClose);

        // Top-right under the nav bar / lock button.
        let anchored = container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::End)
            .align_y(Alignment::Start)
            .padding(Padding {
                top: CHROME_HEIGHT + 4.0,
                right: 10.0,
                bottom: 0.0,
                left: 0.0,
            });

        stack![backdrop, anchored].into()
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        let frames = self.engine.frames();
        let slot = self.slot.clone();
        let active = self.active_handle.clone();
        Subscription::batch(vec![
            crate::run::frame_subscription::<E>(frames, slot, active),
            iced::time::every(Duration::from_millis(250)).map(|_| Msg::Tick),
            sola_kit::app::bus_subscription().map(Msg::Bus),
            event::listen_with(|event, status, _| match event {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Msg::CursorMoved(position.x))
                }
                // A left press anywhere: resolve whether it focused the URL bar
                // (for click-to-select-all). Received regardless of which widget
                // captures it, unlike a wrapping `mouse_area`.
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    Some(Msg::LeftPressed)
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Msg::CursorReleased)
                }
                // Escape stops loading (browser-standard). Only when iced did
                // not already capture the key (e.g. a focused text field that
                // wants Escape for its own cancel).
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                }) if status == event::Status::Ignored => Some(Msg::NavStop),
                // Tab between vault form fields while the panel is open.
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Tab),
                    modifiers,
                    ..
                }) if vault_panel_is_open() => Some(if modifiers.shift() {
                    Msg::VaultFocusPrev
                } else {
                    Msg::VaultFocusNext
                }),
                _ => None,
            }),
        ])
    }
}

/// Process-wide flag so `event::listen_with` (fn pointer) can see panel state.
#[cfg(feature = "bitwarden")]
static VAULT_PANEL_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(feature = "bitwarden")]
fn vault_panel_is_open() -> bool {
    VAULT_PANEL_OPEN.load(Ordering::Relaxed)
}

#[cfg(not(feature = "bitwarden"))]
fn vault_panel_is_open() -> bool {
    false
}

/// Short host label for the fill panel (no full URL wall of gray text).
#[cfg(feature = "bitwarden")]
fn page_host_hint(page_url: &str) -> String {
    if page_url.is_empty() || page_url == BLANK_URL {
        return String::new();
    }
    url::Url::parse(page_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .map(|h| format!("For {h}"))
        .unwrap_or_else(|| {
            let short = if page_url.len() > 36 {
                format!("{}…", &page_url[..35])
            } else {
                page_url.to_string()
            };
            format!("For {short}")
        })
}

/// Unlocked vault toolbar control — subtle accent wash so “ready” ≠ locked.
#[cfg(feature = "bitwarden")]
fn vault_toolbar_btn_unlocked(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    use iced::{Background, Border, Color};
    use sola_kit::components::style::RADIUS_SM;

    let p = theme.extended_palette();
    let accent = p.primary.base.color;
    let bg = match status {
        iced::widget::button::Status::Hovered => Color {
            a: 0.22,
            ..accent
        },
        iced::widget::button::Status::Pressed => Color {
            a: 0.30,
            ..accent
        },
        _ => Color {
            a: 0.14,
            ..accent
        },
    };
    iced::widget::button::Style {
        background: Some(Background::Color(bg)),
        text_color: accent,
        border: Border {
            color: Color {
                a: 0.35,
                ..accent
            },
            width: 1.0,
            radius: RADIUS_SM.into(),
        },
        shadow: Default::default(),
        snap: false,
    }
}

#[cfg(feature = "bitwarden")]
fn vault_email_id() -> iced::widget::Id {
    iced::widget::Id::new("sola-browser-vault-email")
}

#[cfg(feature = "bitwarden")]
fn vault_password_id() -> iced::widget::Id {
    iced::widget::Id::new("sola-browser-vault-password")
}

#[cfg(feature = "bitwarden")]
fn vault_otp_id() -> iced::widget::Id {
    iced::widget::Id::new("sola-browser-vault-otp")
}

impl<E: Engine> Drop for App<E> {
    fn drop(&mut self) {
        // Flush tab session before killing the worker.
        self.persist_session();
        // Orderly engine teardown on iced exit (Cmd::Quit + join worker).
        self.engine.shutdown();
    }
}

/// Prefer live engine url; keep previous title when engine still has "".
fn merge_tab_snapshot(prev: &[TabInfo], live: &[TabInfo]) -> Vec<TabInfo> {
    live.iter()
        .map(|t| {
            let kept_title = prev
                .iter()
                .find(|p| p.id == t.id)
                .map(|p| p.title.as_str())
                .unwrap_or("");
            let title = if t.title.is_empty() && !kept_title.is_empty() {
                kept_title.to_string()
            } else {
                t.title.clone()
            };
            TabInfo {
                id: t.id,
                url: t.url.clone(),
                title,
                is_loading: t.is_loading,
                can_go_back: t.can_go_back,
                can_go_forward: t.can_go_forward,
            }
        })
        .collect()
}
