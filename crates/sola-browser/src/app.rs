//! Browser chrome: message type, layout constants, and the generic `App<E>`.
//!
//! `Msg` and the consts were stubbed out in Task 1 and are kept here. Task 2
//! adds `App<E>`, its constructor, and all update/view/subscription methods.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use iced::widget::{
    Shader, Space, button, column, container, mouse_area, row, scrollable, stack, text,
};
use sola_kit::components::text_input::text_input;
use iced::{Alignment, Element, Event, Length, Padding, Subscription, Task, event, keyboard, mouse};
use sola_kit::components::{
    TabDescriptor, TabSize, field, horizontal_divider, vertical_divider_with, vertical_tabs_sized,
};
use sola_kit::components::select::{SelectOption, select_sized};
use sola_kit::components::button as kit_button;
use sola_kit::components::card;
use sola_kit::components::icon::{icon_handle, icon_svg, icon_svg_colored};
use sola_kit::components::style::{CHROME_SURFACE, PAD_CONTROL_SM, RADIUS_MD};
use sola_kit::components::toolbar as kit_toolbar;
use sola_kit::components::divider::DIVIDER_HIT_PX;


use crate::engine::{Cmd, EditCmd, Engine, FrameSlot, NavCmd, TabId, TabInfo, TabsHandle};
use crate::session::{self, SessionTab};
#[cfg(feature = "bitwarden")]
use crate::vault::{
    MatchSummary, PasskeyCandidate, PasskeyPageRequest, TwoFactorKind, VaultCmd, VaultEvent,
    VaultHandle, VaultStatus, apex_domain, fill_credentials_script, fill_credentials_script_ex,
    generate_password,
};
#[cfg(feature = "bitwarden")]
use zeroize::Zeroize;

/// Cold-start default when the profile session is empty (and for new profiles).
pub const DEFAULT_URL: &str = "about:blank";
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
    /// User picked a passkey for a pending WebAuthn get().
    #[cfg(feature = "bitwarden")]
    VaultPasskeyPick(String),
    /// Cancel pending WebAuthn (reject the page promise).
    #[cfg(feature = "bitwarden")]
    VaultPasskeyCancel,
    /// Re-query matches for the active tab URL.
    #[cfg(feature = "bitwarden")]
    VaultRefreshMatches,
    /// Open the create-login form on the unlocked card.
    #[cfg(feature = "bitwarden")]
    VaultCreateOpen,
    #[cfg(feature = "bitwarden")]
    VaultCreateCancel,
    #[cfg(feature = "bitwarden")]
    VaultCreateSubmit,
    #[cfg(feature = "bitwarden")]
    VaultCreateUsername(String),
    #[cfg(feature = "bitwarden")]
    VaultCreatePassword(String),
    #[cfg(feature = "bitwarden")]
    VaultCreateUrl(String),
    #[cfg(feature = "bitwarden")]
    VaultCreateRegenerate,
    /// Tab / Shift+Tab while vault panel is open.
    VaultFocusNext,
    VaultFocusPrev,
    /// Worker → chrome (drained on Tick when bitwarden enabled).
    #[cfg(feature = "bitwarden")]
    VaultWorker(VaultEvent),
    // —— Profiles (menubar manage dialogs) ——
    ProfileNameInput(String),
    ProfileDialogSubmit,
    ProfileDialogCancel,
    /// Open / close the sidebar profile switcher.
    ProfilePickerToggle,
    /// Outside-click (or Escape) dismissed the sidebar profile switcher.
    ProfilePickerDismiss,
    /// Instant-switch to this profile from the sidebar menu.
    ProfileSwitch(String),
}

/// In-chrome dialog for creating / renaming / confirming delete of a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileDialog {
    New,
    Rename,
    DeleteConfirm,
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
    /// Site asked for a passkey — pick one from the vault.
    PasskeyPick,
    /// Compose a new login (username / generated password / apex URL).
    CreateLogin,
    /// Cipher saved; page had no fields to fill.
    CreateSaved,
}

/// In-flight WebAuthn get() waiting for the user to pick a passkey.
#[cfg(feature = "bitwarden")]
#[derive(Debug, Clone)]
struct PendingPasskey {
    req: PasskeyPageRequest,
    candidates: Vec<PasskeyCandidate>,
    loading: bool,
    error: Option<String>,
}

/// Where shell Edit → Paste (⌘V) should land while the vault panel is open.
#[cfg(feature = "bitwarden")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum VaultPasteTarget {
    #[default]
    Email,
    Password,
    Otp,
    CreateUsername,
    CreatePassword,
    CreateUrl,
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
    /// Tabs chrome has already dropped. Tick merge must not resurrect them
    /// from a lagging engine snapshot (close would flash gone → back → gone).
    closed_tabs: HashSet<TabId>,
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
    /// Tabs to open after the iced Wayland window exists (session restore).
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
    /// Page WebAuthn get() waiting for passkey selection.
    #[cfg(feature = "bitwarden")]
    pending_passkey: Option<PendingPasskey>,
    #[cfg(feature = "bitwarden")]
    vault_create_username: String,
    #[cfg(feature = "bitwarden")]
    vault_create_password: String,
    #[cfg(feature = "bitwarden")]
    vault_create_url: String,
    /// Waiting for `__sola_vault_fill__` after a create.
    #[cfg(feature = "bitwarden")]
    vault_awaiting_fill: bool,
    /// Tick count while waiting (close after ~2s if the page never reports).
    #[cfg(feature = "bitwarden")]
    vault_awaiting_fill_ticks: u8,
    /// Profiles menubar manage dialog (new / rename / delete confirm).
    pub profile_dialog: Option<ProfileDialog>,
    /// Name field for new/rename profile dialogs.
    pub profile_name_field: String,
    /// Inline error under the profile dialog (empty name, last profile, …).
    pub profile_dialog_error: Option<String>,
    /// Parked chrome snapshots per profile (mirrors CEF parks; same eviction).
    workspace_cache: std::collections::HashMap<String, crate::tab_cache::WorkspaceSnapshot>,
    /// Sidebar profile name is a select; true while the menu is open.
    profile_picker_open: bool,
    /// Cached registry rows — `profiles::list()` used to re-read the JSON
    /// from disk on every `view()` (60 Hz while a page animated).
    profile_options: Vec<crate::profiles::ProfileEntry>,
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
            closed_tabs: HashSet::new(),
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
            #[cfg(feature = "bitwarden")]
            pending_passkey: None,
            #[cfg(feature = "bitwarden")]
            vault_create_username: String::new(),
            #[cfg(feature = "bitwarden")]
            vault_create_password: String::new(),
            #[cfg(feature = "bitwarden")]
            vault_create_url: String::new(),
            #[cfg(feature = "bitwarden")]
            vault_awaiting_fill: false,
            #[cfg(feature = "bitwarden")]
            vault_awaiting_fill_ticks: 0,
            profile_dialog: None,
            profile_name_field: String::new(),
            profile_dialog_error: None,
            workspace_cache: std::collections::HashMap::new(),
            profile_picker_open: false,
            profile_options: crate::profiles::list(),
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
    fn open_create_login(&mut self) {
        let page_url = self
            .active_tab_info()
            .map(|t| t.url.as_str())
            .unwrap_or("");
        self.vault_create_url = if page_url.is_empty() || page_url == BLANK_URL {
            String::new()
        } else {
            apex_domain(page_url)
        };
        self.vault_create_username = crate::vault::VaultPrefs::load_last_username().unwrap_or_default();
        self.vault_create_password = generate_password();
        self.vault_error = None;
        self.vault_busy = false;
        self.vault_awaiting_fill = false;
        self.vault_awaiting_fill_ticks = 0;
        crate::vault::passkey_bridge::drain_fill_results();
        self.vault_phase = VaultPanelPhase::CreateLogin;
        self.vault_paste_target = VaultPasteTarget::CreateUsername;
    }

    #[cfg(feature = "bitwarden")]
    fn submit_create_login(&mut self) {
        if self.vault_busy || !self.vault_status.unlocked {
            return;
        }
        if self.vault_create_password.trim().is_empty() {
            self.vault_error = Some("Password is required.".into());
            return;
        }
        let uri = self.vault_create_url.trim().to_string();
        let name = if uri.is_empty() {
            "Login".to_string()
        } else {
            let apex = apex_domain(&uri);
            if apex.is_empty() {
                uri.clone()
            } else {
                apex
            }
        };
        self.vault_busy = true;
        self.vault_error = None;
        self.vault.send(VaultCmd::CreateLogin {
            name,
            username: self.vault_create_username.clone(),
            password: self.vault_create_password.clone(),
            uri,
        });
    }

    #[cfg(feature = "bitwarden")]
    fn finish_create_fill(&mut self, found_fields: bool) {
        self.vault_awaiting_fill = false;
        self.vault_awaiting_fill_ticks = 0;
        self.vault_busy = false;
        crate::vault::passkey_bridge::drain_fill_results();
        if found_fields {
            self.set_vault_panel_open(false);
            self.vault_phase = VaultPanelPhase::Credentials;
        } else {
            self.vault_phase = VaultPanelPhase::CreateSaved;
        }
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
            let title = if url == BLANK_URL && tab.title.is_empty() {
                "New Tab".to_string()
            } else {
                tab.title.clone()
            };
            self.cached_tabs.push(TabInfo {
                id,
                url: url.clone(),
                title: title.clone(),
                is_loading: url != BLANK_URL && !url.is_empty(),
                can_go_back: false,
                can_go_forward: false,
                load_progress: 0.0,
            });
            // One background frame may be imported to seed park cache.
            self.slot.need_park_prime.lock().unwrap().insert(id.0);
            let _ = self.cmd_tx.send(Cmd::OpenTab {
                id,
                url,
                title,
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

    /// Open a profiles manage dialog (from the Profiles menubar).
    pub fn open_profile_dialog(&mut self, kind: ProfileDialog) {
        self.profile_picker_open = false;
        self.profile_dialog_error = None;
        match kind {
            ProfileDialog::New => {
                self.profile_name_field.clear();
                self.profile_dialog = Some(ProfileDialog::New);
            }
            ProfileDialog::Rename => {
                self.profile_name_field = crate::profiles::active().name.clone();
                self.profile_dialog = Some(ProfileDialog::Rename);
            }
            ProfileDialog::DeleteConfirm => {
                if crate::profiles::list().len() <= 1 {
                    tracing::info!("cannot delete the only profile");
                    return;
                }
                self.profile_name_field.clear();
                self.profile_dialog = Some(ProfileDialog::DeleteConfirm);
            }
        }
    }

    pub fn close_profile_dialog(&mut self) {
        self.profile_dialog = None;
        self.profile_name_field.clear();
        self.profile_dialog_error = None;
    }

    /// Instant switch: park this window's tab chrome, activate `id`,
    /// point the engine router at that profile's helper (spawn if needed).
    pub fn switch_profile(&mut self, id: &str) -> Task<Msg> {
        self.profile_picker_open = false;
        let from = crate::profiles::active().id;
        if id == from {
            return Task::none();
        }
        self.persist_session();
        match crate::profiles::activate(id) {
            Ok(_) => {
                tracing::info!(from = %from, to = %id, "switching profile (same window)");
                self.enter_profile_workspace(from, false);
                // Wake the shader so it drops the previous profile's texture
                // this frame, not whenever the new helper happens to paint.
                return Task::done(Msg::NewFrame);
            }
            Err(e) => {
                tracing::warn!(error = %e, id, "switch profile failed");
            }
        }
        Task::none()
    }

    /// Park current chrome snapshot, then resume park or create from session.
    /// `force_cold` skips resume (e.g. brand-new profile with empty session).
    fn enter_profile_workspace(&mut self, park_as_profile_id: String, force_cold: bool) {
        use crate::tab_cache::WorkspaceSnapshot;
        use std::time::Instant;

        if !park_as_profile_id.is_empty() && !self.cached_tabs.is_empty() {
            self.workspace_cache.insert(
                park_as_profile_id.clone(),
                WorkspaceSnapshot {
                    tabs: self.cached_tabs.clone(),
                    active: self.cached_active,
                    sidebar_w: self.sidebar_w,
                    last_used: Instant::now(),
                },
            );
        }

        let profile = crate::profiles::active();
        let resume_id = profile.id.clone();
        let cef_path = profile.cef_user_data_dir().to_string_lossy().into_owned();

        if !force_cold {
            if let Some(snap) = self.workspace_cache.remove(&resume_id) {
                let active = snap.active;
                self.sidebar_w = snap.sidebar_w;
                self.cached_tabs = snap.tabs;
                self.cached_active = active;
                self.apply_workspace_chrome_focus();
                // Helper already has these browsers. Pass the parked list
                // only as a fallback if the helper was evicted/died.
                let create_tabs: Vec<(TabId, String, String)> = self
                    .cached_tabs
                    .iter()
                    .map(|t| (t.id, t.url.clone(), t.title.clone()))
                    .collect();
                tracing::info!(
                    profile = %resume_id,
                    tabs = create_tabs.len(),
                    "resuming parked profile workspace"
                );
                let _ = self.cmd_tx.send(Cmd::SwitchProfileWorkspace {
                    park_as_profile_id,
                    resume_profile_id: resume_id,
                    cef_cache_path: cef_path,
                    create_tabs: Some(create_tabs),
                    active,
                });
                self.evict_workspace_parks();
                crate::integration::republish_menus(self.app_id);
                self.session_fp.clear();
                self.profile_options = crate::profiles::list();
                self.persist_session();
                return;
            }
        }

        let (create_tabs, active) = self.cold_workspace_from_session();
        let _ = self.cmd_tx.send(Cmd::SwitchProfileWorkspace {
            park_as_profile_id,
            resume_profile_id: resume_id,
            cef_cache_path: cef_path,
            create_tabs: Some(create_tabs),
            active,
        });
        self.evict_workspace_parks();
        crate::integration::republish_menus(self.app_id);
        self.session_fp.clear();
        self.profile_options = crate::profiles::list();
        self.persist_session();
    }

    fn cold_workspace_from_session(&mut self) -> (Vec<(TabId, String, String)>, TabId) {
        let (tabs, active_index, sidebar_w) =
            crate::session::BrowserSession::load().bootstrap(None, BLANK_URL);
        self.sidebar_w = sidebar_w;

        let mut new_cached = Vec::with_capacity(tabs.len().max(1));
        let mut open_list = Vec::with_capacity(tabs.len().max(1));
        for tab in &tabs {
            let id = self.engine.alloc_tab_id();
            let url = crate::util::normalize_url(&tab.url);
            let url = if url.is_empty() {
                BLANK_URL.to_string()
            } else {
                url
            };
            let title = if url == BLANK_URL {
                "New Tab".to_string()
            } else {
                tab.title.clone()
            };
            new_cached.push(TabInfo {
                id,
                url: url.clone(),
                title: title.clone(),
                is_loading: url != BLANK_URL && !url.is_empty(),
                can_go_back: false,
                can_go_forward: false,
                load_progress: 0.0,
            });
            open_list.push((id, url, title));
        }
        if open_list.is_empty() {
            let id = self.engine.alloc_tab_id();
            new_cached.push(TabInfo {
                id,
                url: BLANK_URL.to_string(),
                title: "New Tab".into(),
                is_loading: false,
                can_go_back: false,
                can_go_forward: false,
                load_progress: 0.0,
            });
            open_list.push((id, BLANK_URL.to_string(), "New Tab".into()));
        }
        let active = open_list
            .get(active_index.min(open_list.len() - 1))
            .map(|(id, _, _)| *id)
            .unwrap_or(open_list[0].0);

        self.cached_tabs = new_cached;
        self.cached_active = active;
        self.apply_workspace_chrome_focus();
        (open_list, active)
    }

    fn apply_workspace_chrome_focus(&mut self) {
        let active = self.cached_active;
        self.active_handle
            .store(active.0, std::sync::atomic::Ordering::Relaxed);
        // Present *before* zeroing last_size so a same-size park hits.
        self.slot.present_tab(active);
        // Force the next shader prepare to re-send Resize. The router
        // would otherwise leave a newly-front helper at 1280×800 because
        // chrome last_size already matches the widget.
        *self.slot.last_size.lock().unwrap() = (0, 0);
        self.slot.need_park_prime.lock().unwrap().clear();
        self.slot.drop_paint_tabs.lock().unwrap().clear();
        if let Some(info) = self.cached_tabs.iter().find(|t| t.id == active) {
            self.url_field = if info.url == BLANK_URL {
                String::new()
            } else {
                info.url.clone()
            };
            self.last_seen_url = info.url.clone();
        }
    }

    fn evict_workspace_parks(&mut self) {
        use crate::tab_cache::eviction_victims;
        use std::time::Instant;

        let live = self.cached_tabs.len();
        let victims = eviction_victims(&self.workspace_cache, live, Instant::now());
        for id in victims {
            if let Some(snap) = self.workspace_cache.remove(&id) {
                for t in snap.tabs {
                    self.slot.forget_tab(t.id);
                }
            }
            let _ = self.cmd_tx.send(Cmd::DropParkedProfile {
                profile_id: id.clone(),
            });
            tracing::info!(profile = %id, "evicted parked profile workspace");
        }
    }

    fn submit_profile_dialog(&mut self) -> Task<Msg> {
        match self.profile_dialog.clone() {
            Some(ProfileDialog::New) => {
                self.persist_session();
                let from = crate::profiles::active().id.clone();
                match crate::profiles::create_and_activate(&self.profile_name_field) {
                    Ok(profile) => {
                        tracing::info!(id = %profile.id, name = %profile.name, "new profile");
                        self.close_profile_dialog();
                        self.enter_profile_workspace(from, true);
                        return Task::done(Msg::NewFrame);
                    }
                    Err(e) => {
                        self.profile_dialog_error = Some(e);
                    }
                }
            }
            Some(ProfileDialog::Rename) => {
                let id = crate::profiles::active().id.clone();
                match crate::profiles::rename(&id, &self.profile_name_field) {
                    Ok(()) => {
                        self.close_profile_dialog();
                        crate::integration::republish_menus(self.app_id);
                    }
                    Err(e) => {
                        self.profile_dialog_error = Some(e);
                    }
                }
            }
            Some(ProfileDialog::DeleteConfirm) => {
                let id = crate::profiles::active().id.clone();
                for t in &self.cached_tabs {
                    self.slot.forget_tab(t.id);
                }
                self.workspace_cache.remove(&id);
                match crate::profiles::delete(&id) {
                    Ok(Some(_new_active)) => {
                        self.close_profile_dialog();
                        let _ = self.cmd_tx.send(Cmd::DropParkedProfile {
                            profile_id: id.clone(),
                        });
                        self.enter_profile_workspace(id, false);
                        return Task::done(Msg::NewFrame);
                    }
                    Ok(None) => {
                        self.close_profile_dialog();
                        crate::integration::republish_menus(self.app_id);
                    }
                    Err(e) => {
                        self.profile_dialog_error = Some(e);
                    }
                }
            }
            None => {}
        }
        Task::none()
    }

    /// Write session to disk if the tab list / active / sidebar changed.
    pub fn persist_session(&mut self) {
        // Merge so a mid-navigation engine `about:blank` does not persist
        // over the URL we just committed.
        let live = self.tabs_handle.lock().unwrap().clone();
        let tabs = if live.is_empty() {
            self.cached_tabs.clone()
        } else {
            merge_tab_snapshot(&self.cached_tabs, &live, &self.closed_tabs)
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
                // Iced is connected; open restored tabs now.
                if let Some((tabs, active_index)) = self.pending_session.take() {
                    self.bootstrap_tabs(tabs, active_index);
                }
                return Task::none();
            }
            Msg::TitleDrag => return sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => return sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => {
                self.persist_session();
                sola_kit::close_app(self.app_id);
                return iced::exit();
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
            Msg::VaultPasskeyPick(cipher_id) => {
                #[cfg(feature = "bitwarden")]
                {
                    self.confirm_passkey_pick(cipher_id);
                }
            }
            Msg::VaultPasskeyCancel => {
                #[cfg(feature = "bitwarden")]
                {
                    self.cancel_pending_passkey("User cancelled.");
                }
            }
            Msg::VaultRefreshMatches => {
                #[cfg(feature = "bitwarden")]
                {
                    self.request_vault_matches();
                }
            }
            Msg::VaultCreateOpen => {
                #[cfg(feature = "bitwarden")]
                {
                    if !self.vault_status.unlocked {
                        return Task::none();
                    }
                    self.open_create_login();
                    return Task::batch([
                        iced::widget::operation::focus(vault_create_username_id()),
                        iced::advanced::widget::operate(
                            iced::advanced::widget::operation::text_input::select_all::<Msg>(
                                vault_create_username_id(),
                            ),
                        ),
                    ]);
                }
            }
            Msg::VaultCreateCancel => {
                #[cfg(feature = "bitwarden")]
                {
                    self.vault_awaiting_fill = false;
                    self.vault_awaiting_fill_ticks = 0;
                    self.vault_busy = false;
                    self.vault_error = None;
                    self.vault_phase = VaultPanelPhase::Credentials;
                    self.request_vault_matches();
                }
            }
            Msg::VaultCreateSubmit => {
                #[cfg(feature = "bitwarden")]
                {
                    self.submit_create_login();
                }
            }
            Msg::VaultCreateUsername(s) => {
                #[cfg(feature = "bitwarden")]
                {
                    self.vault_create_username = s;
                    self.vault_paste_target = VaultPasteTarget::CreateUsername;
                }
            }
            Msg::VaultCreatePassword(s) => {
                #[cfg(feature = "bitwarden")]
                {
                    self.vault_create_password = s;
                    self.vault_paste_target = VaultPasteTarget::CreatePassword;
                }
            }
            Msg::VaultCreateUrl(s) => {
                #[cfg(feature = "bitwarden")]
                {
                    self.vault_create_url = s;
                    self.vault_paste_target = VaultPasteTarget::CreateUrl;
                }
            }
            Msg::VaultCreateRegenerate => {
                #[cfg(feature = "bitwarden")]
                {
                    if !self.vault_busy {
                        self.vault_create_password = generate_password();
                    }
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
                        VaultPanelPhase::Credentials
                        | VaultPanelPhase::PasskeyPick
                        | VaultPanelPhase::CreateLogin
                        | VaultPanelPhase::CreateSaved => {
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
                        VaultPanelPhase::Credentials
                        | VaultPanelPhase::PasskeyPick
                        | VaultPanelPhase::CreateLogin
                        | VaultPanelPhase::CreateSaved => {
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
                    if self.pending_passkey.is_some() {
                        self.cancel_pending_passkey("User cancelled.");
                    } else {
                        if matches!(self.vault_phase, VaultPanelPhase::CreateSaved) {
                            self.vault_phase = VaultPanelPhase::Credentials;
                        }
                        self.vault_awaiting_fill = false;
                        self.set_vault_panel_open(false);
                    }
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
            Msg::ProfileNameInput(s) => {
                self.profile_name_field = s;
                self.profile_dialog_error = None;
            }
            Msg::ProfileDialogCancel => {
                self.close_profile_dialog();
            }
            Msg::ProfileDialogSubmit => {
                return self.submit_profile_dialog();
            }
            Msg::ProfilePickerToggle => {
                self.profile_picker_open = !self.profile_picker_open;
            }
            Msg::ProfilePickerDismiss => {
                self.profile_picker_open = false;
            }
            Msg::ProfileSwitch(id) => {
                return self.switch_profile(&id);
            }
            Msg::NewFrame => {
                // Allow the next kick if the shader pump stops. While the
                // shader is request_redraw-pumping, frame_stream skips this.
                self.slot
                    .redraw_queued
                    .store(false, std::sync::atomic::Ordering::Release);
                self.slot
                    .pumping
                    .store(true, std::sync::atomic::Ordering::Release);
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
                if self.profile_picker_open {
                    self.profile_picker_open = false;
                    return Task::none();
                }
                if self.profile_dialog.is_some() {
                    self.close_profile_dialog();
                    return Task::none();
                }
                #[cfg(feature = "bitwarden")]
                if self.pending_passkey.is_some() {
                    self.cancel_pending_passkey("User cancelled.");
                    return Task::none();
                }
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
                // Instant typed → resolved, then drop the caret so the field
                // is not an editable well while CEF settles the canonical URL.
                self.url_field = url.clone();
                self.last_seen_url = url.clone();
                self.url_bar_focused = false;
                // Optimistic: update cached tab url so session persists immediately.
                if let Some(t) = self
                    .cached_tabs
                    .iter_mut()
                    .find(|t| t.id == self.cached_active)
                {
                    t.url = url.clone();
                    t.is_loading = true;
                    t.load_progress = 0.0;
                }
                let _ = self.cmd_tx.send(Cmd::Nav(NavCmd::LoadUrl(url)));
                let _ = self.cmd_tx.send(Cmd::Focus(true));
                self.persist_session();
                return crate::integration::unfocus_chrome();
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
                let closed_idx = self.cached_tabs.iter().position(|t| t.id == id);
                self.slot.forget_tab(id);
                self.slot.drop_paint_tabs.lock().unwrap().push(id.0);
                self.slot.need_park_prime.lock().unwrap().remove(&id.0);
                let _ = self.cmd_tx.send(Cmd::CloseTab(id));
                // Drop from optimistic cache immediately so persist sees it
                // and Tick cannot paint the row again.
                self.closed_tabs.insert(id);
                self.cached_tabs.retain(|t| t.id != id);
                if let (Some(idx), Some(h)) = (closed_idx, self.hovered_tab) {
                    if h > idx {
                        self.hovered_tab = Some(h - 1);
                    } else if h == idx && h >= self.cached_tabs.len() {
                        self.hovered_tab = self.cached_tabs.len().checked_sub(1);
                    }
                }
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
                    // WebAuthn intercepts from CEF console → vault passkey sign.
                    while let Some(req) = crate::vault::passkey_bridge::try_recv() {
                        self.dispatch_passkey_request(req);
                    }
                    if self.vault_awaiting_fill {
                        if let Some(found) = crate::vault::passkey_bridge::try_recv_fill() {
                            self.finish_create_fill(found);
                        } else {
                            self.vault_awaiting_fill_ticks =
                                self.vault_awaiting_fill_ticks.saturating_add(1);
                            // 250ms tick × 8 ≈ 2s — page never reported.
                            if self.vault_awaiting_fill_ticks >= 8 {
                                self.finish_create_fill(true);
                            }
                        }
                    }
                }
                // Merge engine snapshot with prior cache: WebKit often reports
                // empty title until the page finishes loading (esp. inactive
                // restored tabs). Keep the last known title so the strip does
                // not blank out after session restore.
                let live = self.tabs_handle.lock().unwrap().clone();
                if !live.is_empty() {
                    self.cached_tabs =
                        merge_tab_snapshot(&self.cached_tabs, &live, &self.closed_tabs);
                    self.closed_tabs
                        .retain(|id| live.iter().any(|t| t.id == *id));
                }
                // Chrome `paint_tab` is the strip/omnibox authority. The
                // worker `active_handle` can lag a pump tick behind and was
                // clobbering optimistic activate (new-tab had no highlight).
                let paint = self.slot.paint_tab.load(Ordering::Relaxed);
                let candidate = if paint != u64::MAX {
                    TabId(paint)
                } else {
                    TabId(self.active_handle.load(Ordering::Relaxed))
                };
                if candidate.0 != u64::MAX
                    && self.cached_tabs.iter().any(|t| t.id == candidate)
                    && !self.closed_tabs.contains(&candidate)
                {
                    self.cached_active = candidate;
                }
                let active_url = self.active_tab_info().map(|t| t.url.clone());
                if let Some(url) = active_url {
                    let (field, seen) = apply_omnibar_url(
                        &self.url_field,
                        &self.last_seen_url,
                        &url,
                        self.url_bar_focused,
                    );
                    self.url_field = field;
                    self.last_seen_url = seen;
                }
                self.persist_session();
                self.profile_options = crate::profiles::list();
                // Drain any page-selection text the engine extracted for a copy
                // and put it on the system clipboard via iced. The engine's own
                // clipboard can't reach Wayland (headless display); iced's can.
                if let Some(text) = self
                    .engine
                    .clipboard_handle()
                    .lock()
                    .unwrap()
                    .take()
                    .and_then(|t| crate::util::usable_clipboard_text(Some(t)))
                {
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
                DIVIDER_DRAGGING.store(true, Ordering::Relaxed);
                let x = f32::from_bits(CURSOR_X_BITS.load(Ordering::Relaxed));
                self.last_cursor_x = Some(x);
                self.drag_anchor = Some((x, self.sidebar_w));
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
                DIVIDER_DRAGGING.store(false, Ordering::Relaxed);
            }
            Msg::TabHover(i) => self.hovered_tab = i,
            Msg::WebViewFocused => {
                // Page took the click: drop iced chrome focus so keys go to
                // the shader → CEF, and tell the host it is the focused OSR
                // widget (caret blink / IME require SetFocus).
                self.url_bar_focused = false;
                let _ = self.cmd_tx.send(Cmd::Focus(true));
                return crate::integration::unfocus_chrome();
            }
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
                    // Omnibox owns keys; CEF must not keep the OSR caret.
                    let _ = self.cmd_tx.send(Cmd::Focus(false));
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
                            VaultPasteTarget::CreateUsername => {
                                iced::widget::operation::focus(vault_create_username_id())
                            }
                            VaultPasteTarget::CreatePassword => {
                                iced::widget::operation::focus(vault_create_password_id())
                            }
                            VaultPasteTarget::CreateUrl => {
                                iced::widget::operation::focus(vault_create_url_id())
                            }
                        },
                        EditCmd::Copy => {
                            let raw = match self.vault_paste_target {
                                VaultPasteTarget::Email => self.vault_email.clone(),
                                VaultPasteTarget::Password => return Task::none(),
                                VaultPasteTarget::Otp => self.vault_otp.clone(),
                                VaultPasteTarget::CreateUsername => {
                                    self.vault_create_username.clone()
                                }
                                VaultPasteTarget::CreatePassword => {
                                    self.vault_create_password.clone()
                                }
                                VaultPasteTarget::CreateUrl => self.vault_create_url.clone(),
                            };
                            match crate::util::usable_clipboard_text(Some(raw)) {
                                Some(t) => iced::clipboard::write(t),
                                None => Task::none(),
                            }
                        }
                        EditCmd::Cut | EditCmd::Undo | EditCmd::Redo => Task::none(),
                    };
                }
                if url_bar_focused {
                    tracing::debug!(?cmd, "edit → URL bar (iced clipboard)");
                    return match cmd {
                        EditCmd::Copy => {
                            match crate::util::usable_clipboard_text(Some(self.url_field.clone()))
                            {
                                Some(t) => iced::clipboard::write(t),
                                None => Task::none(),
                            }
                        }
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
                // ⌘V: chrome reads (it has seat focus), restores the offer,
                // then injects into the focused page field. CEF `paste()`
                // after a chrome read hits an empty clipboard and can *set*
                // that empty selection as the new source.
                if cmd == EditCmd::Paste {
                    return iced::clipboard::read().map(Msg::PagePasted);
                }
                if cmd == EditCmd::Copy || cmd == EditCmd::Cut {
                    // frame.copy() only fills Chromium's clipboard. Extract
                    // the selection via JS and write it to Wayland ourselves.
                    let _ = self
                        .cmd_tx
                        .send(Cmd::EvaluateJs(crate::paste_js::copy_selection_script()));
                    if cmd == EditCmd::Cut {
                        let _ = self.cmd_tx.send(Cmd::Edit(EditCmd::Cut));
                    }
                    return Task::none();
                }
                let _ = self.cmd_tx.send(Cmd::Edit(cmd));
            }
            Msg::UrlPasted(text) => {
                let Some(s) = crate::util::usable_clipboard_text(text) else {
                    return Task::none();
                };
                // Best-effort: iced exposes no caret/selection, so append
                // at the end (cursor-at-end assumption).
                self.url_field.push_str(&s);
                // Restore: smithay receive can drop the original offer.
                return iced::clipboard::write(s);
            }
            Msg::PagePasted(text) => {
                let Some(s) = crate::util::usable_clipboard_text(text) else {
                    return Task::none();
                };
                let script = crate::paste_js::paste_into_focused_script(&s);
                let _ = self.cmd_tx.send(Cmd::EvaluateJs(script));
                return iced::clipboard::write(s);
            }
            #[cfg(feature = "bitwarden")]
            Msg::VaultClipboardPaste(text) => {
                let Some(cleaned) = crate::util::usable_clipboard_text(text) else {
                    return Task::none();
                };
                match self.vault_phase {
                    VaultPanelPhase::TwoFactor { .. } => {
                        self.vault_otp =
                            cleaned.chars().filter(|c| !c.is_whitespace()).collect();
                        self.vault_paste_target = VaultPasteTarget::Otp;
                    }
                    VaultPanelPhase::Credentials => match self.vault_paste_target {
                        VaultPasteTarget::Email => {
                            self.vault_email = cleaned.clone();
                            self.vault_paste_target = VaultPasteTarget::Email;
                        }
                        VaultPasteTarget::Password | VaultPasteTarget::Otp => {
                            self.vault_password = cleaned.clone();
                            self.vault_paste_target = VaultPasteTarget::Password;
                        }
                        _ => {}
                    },
                    VaultPanelPhase::CreateLogin => match self.vault_paste_target {
                        VaultPasteTarget::CreatePassword => {
                            self.vault_create_password = cleaned.clone();
                        }
                        VaultPasteTarget::CreateUrl => {
                            self.vault_create_url = cleaned.clone();
                        }
                        _ => {
                            self.vault_create_username = cleaned.clone();
                            self.vault_paste_target = VaultPasteTarget::CreateUsername;
                        }
                    },
                    VaultPanelPhase::PasskeyPick | VaultPanelPhase::CreateSaved => {
                        return Task::none();
                    }
                }
                return iced::clipboard::write(cleaned);
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
            is_loading: url != BLANK_URL && !url.is_empty(),
            can_go_back: false,
            can_go_forward: false,
            load_progress: 0.0,
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
            if !loading {
                t.load_progress = 0.0;
            }
        }
    }

    /// Switch which tab paints: update chrome state, drop any queued
    /// frame for the previous tab, and ask the worker to activate.
    /// Without clearing `pending` / `paint_tab`, the shader keeps
    /// sampling the previous tab's texture until a new frame arrives
    /// (and static pages may never produce one).
    pub fn switch_active_tab(&mut self, id: TabId) {
        self.cached_active = id;
        // Optimistic: worker frame filter reads this before SetActiveTab is
        // pumped. Without this, every buffer-rendered is drop_bg → black page.
        self.active_handle
            .store(id.0, std::sync::atomic::Ordering::Relaxed);
        // Same-size park → pending this frame. Miss → blank now (do not
        // keep sampling the previous tab until CEF answers).
        self.slot.present_tab(id);
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
        let page_owns_keys = !self.url_bar_focused
            && self.profile_dialog.is_none()
            && {
                #[cfg(feature = "bitwarden")]
                {
                    !self.vault_panel_open
                }
                #[cfg(not(feature = "bitwarden"))]
                {
                    true
                }
            };
        let webview = crate::cef::page_ime::page_ime(
            webview,
            self.slot.clone(),
            page_owns_keys,
        );

        // Full-width chrome (profile + nav + omnibox), then tabs | page.
        let lower = row![
            container(self.view_tab_sidebar())
                .width(Length::Fixed(self.sidebar_w))
                .height(Length::Fill),
            vertical_divider_with(
                Msg::DividerPress,
                sola_kit::components::DividerColors::raised_to_canvas(&self.theme),
            ),
            container(webview).width(Length::Fill).height(Length::Fill),
        ]
        .height(Length::Fill);
        let main = column![self.view_chrome_bar(), horizontal_divider(), lower]
            .width(Length::Fill)
            .height(Length::Fill);

        // Opaque canvas under chrome chrome (sidebar / omnibox / tabs). The
        // window is `transparent: true` so iced's clear is α=0 — without a
        // solid fill, every unpainted pixel shows the app under the browser.
        // The webview shader still punches α=0 only in the content scissor.
        let canvas = self.theme.extended_palette().background.base.color;
        let canvas = iced::Color {
            a: 1.0,
            ..canvas
        };
        let body: Element<'_, Msg> = container(main)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_t: &iced::Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(canvas)),
                ..iced::widget::container::Style::default()
            })
            .into();

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

        let content: Element<'_, Msg> = if self.profile_dialog.is_some() {
            stack![content, self.view_profile_dialog()].into()
        } else {
            content
        };

        sola_kit::wrap_if_floating(
            self.float.is_floating_any(),
            crate::profiles::active().name.as_str(),
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            content,
        )
    }

    /// Left vertical tab column. Profile switch lives in the full-width
    /// chrome bar; this is just the title stack. New tabs come from `⌘T`.
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

    /// Identity select — kit `select`, enamel mark per profile.
    fn view_profile_picker(&self) -> Element<'_, Msg> {
        let active = crate::profiles::active();
        let options = self.profile_options.iter().map(|p| {
            SelectOption::new(
                p.name.clone(),
                p.id == active.id,
                Msg::ProfileSwitch(p.id.clone()),
            )
            .mark(p.id.clone())
        });
        let inner = (self.sidebar_w - 12.0).max(140.0);
        select_sized(
            active.name,
            options,
            self.profile_picker_open,
            Msg::ProfilePickerToggle,
            Msg::ProfilePickerDismiss,
            inner,
        )
    }

    /// Full-width chrome strip: profile (aligned to the tab column),
    /// back / forward / reload, omnibox, vault.
    ///
    /// The URL field isn't wrapped in a `mouse_area`: `text_input` captures
    /// the click to place its caret, and `mouse_area` skips `on_press` for
    /// captured events. Click-into-focus + select-all is handled instead via
    /// the global press subscription (`Msg::LeftPressed`) plus a live focus
    /// query, which sees the press regardless of widget capture.
    pub fn view_chrome_bar(&self) -> Element<'_, Msg> {
        use sola_kit::components::style::{SPACE_MD, SPACE_SM};
        const NAV_BTN_W: f32 = 34.0;
        let info = self.active_tab_info();
        let can_back = info.map(|t| t.can_go_back).unwrap_or(false);
        let can_fwd = info.map(|t| t.can_go_forward).unwrap_or(false);
        let muted = {
            let t = self.theme.extended_palette().secondary.base.text;
            iced::Color { a: 0.55, ..t }
        };
        let back = self.nav_icon_btn(nav_icon_back(), 16, can_back, NAV_BTN_W, Msg::NavBack, muted);
        let forward =
            self.nav_icon_btn(nav_icon_forward(), 16, can_fwd, NAV_BTN_W, Msg::NavForward, muted);
        let reload_handle = if self.active_is_loading() {
            nav_icon_stop()
        } else {
            nav_icon_reload()
        };
        let reload_or_stop = self.nav_icon_btn(
            reload_handle,
            16,
            true,
            NAV_BTN_W,
            Msg::NavReloadOrStop,
            muted,
        );

        #[cfg(feature = "bitwarden")]
        let vault_btn = {
            let unlocked = self.vault_status.unlocked;
            let handle = if unlocked {
                self.vault_icon_unlocked.clone()
            } else {
                self.vault_icon_locked.clone()
            };
            let icon = if unlocked {
                icon_svg_colored(
                    handle,
                    18,
                    self.theme.extended_palette().primary.base.color,
                )
            } else {
                icon_svg_colored(handle, 18, muted)
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

        let profile = container(self.view_profile_picker())
            .width(Length::Fixed(self.sidebar_w))
            .padding(Padding::from([0, 6]))
            .align_y(Alignment::Center);

        let bar = row![
            profile,
            Space::new().width(Length::Fixed(DIVIDER_HIT_PX)),
            back,
            forward,
            reload_or_stop,
            self.view_omnibox(),
            vault_btn,
        ]
        .spacing(SPACE_SM)
        .padding([SPACE_SM, SPACE_MD])
        .align_y(Alignment::Center)
        .height(Length::Fixed(CHROME_HEIGHT));

        container(bar)
            .width(Length::Fill)
            .style(|_t: &iced::Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(CHROME_SURFACE)),
                ..Default::default()
            })
            .into()
    }

    fn view_omnibox(&self) -> Element<'_, Msg> {
        let field = text_input("Search or enter URL", &self.url_field)
            .id(crate::integration::url_input_id())
            .on_input(Msg::UrlInput)
            .on_submit(Msg::UrlSubmit)
            .size(13)
            .style(sola_kit::components::text_input::style)
            .width(Length::Fill);
        match self.active_load_frac() {
            Some(frac) => stack![field, omnibox_progress_overlay(frac)].into(),
            None => field.into(),
        }
    }

    /// Determinate load fraction for the omnibox hairline, if the active tab
    /// is loading. A small floor so the line appears the moment navigation
    /// starts (CEF often sits at 0 for the first callback).
    fn active_load_frac(&self) -> Option<f32> {
        let info = self.active_tab_info()?;
        // A fresh about:blank tab loads internally; don't paint a bar on it.
        if !info.is_loading || is_transient_nav_url(&info.url) {
            return None;
        }
        Some(info.load_progress.clamp(0.0, 1.0).max(0.08))
    }

    fn nav_icon_btn(
        &self,
        handle: iced::widget::svg::Handle,
        size: u16,
        enabled: bool,
        width: f32,
        msg: Msg,
        muted: iced::Color,
    ) -> Element<'_, Msg> {
        let icon: Element<'_, Msg> = if enabled {
            icon_svg(handle, size)
        } else {
            icon_svg_colored(handle, size, muted)
        };
        let b = button(icon)
            .padding(PAD_CONTROL_SM)
            .width(Length::Fixed(width))
            .style(kit_toolbar::style);
        if enabled {
            b.on_press(msg).into()
        } else {
            b.into()
        }
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
                // Passkey ceremony mid-unlock → stay on picker. Otherwise open
                // the fill/password panel for the active page.
                if self.pending_passkey.is_some() {
                    self.vault_phase = VaultPanelPhase::PasskeyPick;
                    self.set_vault_panel_open(true);
                    self.request_passkey_candidates();
                } else {
                    self.vault_phase = VaultPanelPhase::Credentials;
                    self.set_vault_panel_open(true);
                    self.request_vault_matches();
                }
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
            VaultEvent::Created {
                id: _,
                mut username,
                mut password,
            } => {
                crate::vault::passkey_bridge::drain_fill_results();
                let script = fill_credentials_script_ex(
                    username.as_deref(),
                    password.as_deref(),
                    true,
                );
                if let Some(ref mut p) = password {
                    p.zeroize();
                }
                if let Some(ref mut u) = username {
                    u.zeroize();
                }
                self.vault_create_password.clear();
                self.vault_awaiting_fill = true;
                self.vault_awaiting_fill_ticks = 0;
                let _ = self.cmd_tx.send(Cmd::EvaluateJs(script));
                tracing::info!("vault: created login — fill injected");
            }
            VaultEvent::PasskeyCandidates { req_id, candidates } => {
                if let Some(ref mut pending) = self.pending_passkey {
                    if pending.req.id != req_id {
                        return;
                    }
                    pending.loading = false;
                    pending.candidates = candidates;
                    if pending.candidates.is_empty() {
                        pending.error = Some("No passkeys in the vault for this site.".into());
                    } else {
                        pending.error = None;
                    }
                    self.vault_phase = VaultPanelPhase::PasskeyPick;
                    self.set_vault_panel_open(true);
                }
            }
            VaultEvent::PasskeyReady {
                req_id,
                ok,
                payload,
            } => {
                self.vault_busy = false;
                // Clear pending only if this is the current request.
                if self
                    .pending_passkey
                    .as_ref()
                    .map(|p| p.req.id == req_id)
                    .unwrap_or(false)
                {
                    self.pending_passkey = None;
                    if matches!(self.vault_phase, VaultPanelPhase::PasskeyPick) {
                        self.vault_phase = VaultPanelPhase::Credentials;
                        self.set_vault_panel_open(false);
                    }
                }
                let script = crate::vault::resolve_webauthn_script(req_id, ok, &payload);
                let _ = self.cmd_tx.send(Cmd::EvaluateJs(script));
                if ok {
                    tracing::info!(req_id, "vault: passkey response injected");
                } else {
                    tracing::warn!(req_id, error = %payload, "vault: passkey response error injected");
                    if self.vault_panel_open {
                        self.vault_error = Some(payload);
                    }
                }
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

    /// Page asked for a passkey: open the vault panel picker (or unlock first).
    #[cfg(feature = "bitwarden")]
    fn dispatch_passkey_request(&mut self, req: PasskeyPageRequest) {
        // Replace any prior pending request (reject the old page promise).
        if let Some(old) = self.pending_passkey.take() {
            let script = crate::vault::resolve_webauthn_script(
                old.req.id,
                false,
                "Superseded by another passkey request.",
            );
            let _ = self.cmd_tx.send(Cmd::EvaluateJs(script));
        }

        tracing::info!(
            req_id = req.id,
            origin = %req.origin,
            rp_id = %req.rp_id,
            "vault: page requested passkey — opening picker"
        );

        self.pending_passkey = Some(PendingPasskey {
            req,
            candidates: Vec::new(),
            loading: true,
            error: None,
        });
        self.vault_error = None;

        if !self.vault_status.unlocked {
            self.vault_phase = VaultPanelPhase::Credentials;
            self.set_vault_panel_open(true);
            // User unlocks; LoginOk continues into request_passkey_candidates.
            return;
        }

        self.vault_phase = VaultPanelPhase::PasskeyPick;
        self.set_vault_panel_open(true);
        self.request_passkey_candidates();
    }

    #[cfg(feature = "bitwarden")]
    fn request_passkey_candidates(&mut self) {
        let Some(pending) = self.pending_passkey.as_mut() else {
            return;
        };
        pending.loading = true;
        pending.error = None;
        let mut rp_id = pending.req.rp_id.clone();
        if rp_id.is_empty() {
            if let Some(host) = pending
                .req
                .origin
                .strip_prefix("https://")
                .or_else(|| pending.req.origin.strip_prefix("http://"))
            {
                rp_id = host.split('/').next().unwrap_or(host).to_string();
            }
        }
        let req_id = pending.req.id;
        self.vault.send(VaultCmd::PasskeyList { req_id, rp_id });
    }

    #[cfg(feature = "bitwarden")]
    fn confirm_passkey_pick(&mut self, cipher_id: String) {
        let Some(pending) = self.pending_passkey.clone() else {
            return;
        };
        if self.vault_busy {
            return;
        }
        self.vault_busy = true;
        self.vault_error = None;
        if let Some(p) = self.pending_passkey.as_mut() {
            p.error = None;
        }
        self.vault.send(VaultCmd::PasskeyAssert {
            req_id: pending.req.id,
            origin: pending.req.origin,
            public_key_json: pending.req.public_key_json,
            cipher_id,
        });
    }

    #[cfg(feature = "bitwarden")]
    fn cancel_pending_passkey(&mut self, reason: &str) {
        if let Some(pending) = self.pending_passkey.take() {
            let script =
                crate::vault::resolve_webauthn_script(pending.req.id, false, reason);
            let _ = self.cmd_tx.send(Cmd::EvaluateJs(script));
        }
        if matches!(self.vault_phase, VaultPanelPhase::PasskeyPick) {
            self.vault_phase = VaultPanelPhase::Credentials;
            self.set_vault_panel_open(false);
        }
        self.vault_busy = false;
    }

    /// Centered modal for Profiles menubar manage actions.
    fn view_profile_dialog(&self) -> Element<'_, Msg> {
        use sola_kit::components::style::{SPACE_MD, SPACE_SM};

        let Some(kind) = self.profile_dialog.as_ref() else {
            return Space::new().width(Length::Shrink).height(Length::Shrink).into();
        };

        let title = match kind {
            ProfileDialog::New => "New Profile",
            ProfileDialog::Rename => "Rename Profile",
            ProfileDialog::DeleteConfirm => "Delete Profile",
        };
        let title_el = text(title)
            .size(15)
            .font(sola_kit::fonts::ui_medium());

        let body: Element<'_, Msg> = match kind {
            ProfileDialog::New | ProfileDialog::Rename => {
                let hint = match kind {
                    ProfileDialog::New => "Name for the new profile.",
                    ProfileDialog::Rename => "New name for this profile.",
                    ProfileDialog::DeleteConfirm => unreachable!(),
                };
                let name_field = text_input("Profile name", &self.profile_name_field)
                    .id(crate::integration::profile_name_input_id())
                    .size(13)
                    .style(sola_kit::components::text_input::style)
                    .width(Length::Fill)
                    .on_input(Msg::ProfileNameInput)
                    .on_submit(Msg::ProfileDialogSubmit);

                let mut col = column![
                    title_el,
                    text(hint).size(12).style(|theme: &iced::Theme| {
                        let t = theme.extended_palette().background.base.text;
                        iced::widget::text::Style {
                            color: Some(iced::Color { a: 0.72, ..t }),
                        }
                    }),
                    Space::new().height(SPACE_SM),
                    name_field,
                ]
                .spacing(SPACE_SM)
                .width(Length::Fixed(300.0));

                if let Some(err) = &self.profile_dialog_error {
                    col = col.push(
                        text(err.clone())
                            .size(12)
                            .style(|theme: &iced::Theme| iced::widget::text::Style {
                                color: Some(theme.extended_palette().danger.base.color),
                            }),
                    );
                }

                let submit_label = match kind {
                    ProfileDialog::New => "Create",
                    ProfileDialog::Rename => "Rename",
                    ProfileDialog::DeleteConfirm => unreachable!(),
                };
                let actions = row![
                    kit_button::labeled(submit_label, kit_button::primary)
                        .on_press(Msg::ProfileDialogSubmit),
                    kit_button::labeled("Cancel", kit_button::ghost)
                        .on_press(Msg::ProfileDialogCancel),
                ]
                .spacing(SPACE_SM)
                .align_y(Alignment::Center);
                col.push(actions).into()
            }
            ProfileDialog::DeleteConfirm => {
                let name = crate::profiles::active().name.clone();
                let mut col = column![
                    title_el,
                    text(format!(
                        "Delete “{name}”? Open tabs and site data for this profile will be removed."
                    ))
                    .size(12)
                    .style(|theme: &iced::Theme| {
                        let t = theme.extended_palette().background.base.text;
                        iced::widget::text::Style {
                            color: Some(iced::Color { a: 0.72, ..t }),
                        }
                    }),
                ]
                .spacing(SPACE_SM)
                .width(Length::Fixed(300.0));

                if let Some(err) = &self.profile_dialog_error {
                    col = col.push(
                        text(err.clone())
                            .size(12)
                            .style(|theme: &iced::Theme| iced::widget::text::Style {
                                color: Some(theme.extended_palette().danger.base.color),
                            }),
                    );
                }

                let actions = row![
                    kit_button::labeled("Delete", kit_button::primary)
                        .on_press(Msg::ProfileDialogSubmit),
                    kit_button::labeled("Cancel", kit_button::ghost)
                        .on_press(Msg::ProfileDialogCancel),
                ]
                .spacing(SPACE_SM)
                .align_y(Alignment::Center);
                col.push(actions).into()
            }
        };

        let panel = card::modal(container(body).padding(SPACE_MD + SPACE_SM))
            .width(Length::Fixed(340.0));

        let backdrop = mouse_area(
            container(Space::new().width(Length::Fill).height(Length::Fill)).style(|_t| {
                container::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgba(
                        0.0, 0.0, 0.0, 0.22,
                    ))),
                    ..container::Style::default()
                }
            }),
        )
        .on_press(Msg::ProfileDialogCancel);

        let centered = container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center);

        stack![backdrop, centered].into()
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

        let body: Element<'_, Msg> = if matches!(self.vault_phase, VaultPanelPhase::PasskeyPick)
            || (self.pending_passkey.is_some() && self.vault_status.unlocked)
        {
            // Site asked for a passkey — pick one.
            const MATCH_LIST_H: f32 = 420.0;
            let title = text("Choose a passkey")
                .size(15)
                .font(sola_kit::fonts::ui_medium());
            let pending = self.pending_passkey.as_ref();
            let host = pending
                .map(|p| page_host_hint(&p.req.origin))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "this site".into());

            let mut col = column![title, soft(format!("Sign in to {host}"))]
                .spacing(SPACE_SM)
                .width(Length::Fixed(340.0));

            if let Some(err) = err_line {
                col = col.push(err);
            }
            if let Some(pending) = pending {
                if let Some(err) = pending.error.as_ref() {
                    col = col.push(
                        text(err.clone())
                            .size(12)
                            .style(|theme: &iced::Theme| iced::widget::text::Style {
                                color: Some(theme.extended_palette().danger.base.color),
                            }),
                    );
                }
                if pending.loading {
                    col = col.push(text("Looking up passkeys…").size(13));
                } else if pending.candidates.is_empty() {
                    col = col.push(text("No passkeys saved for this site.").size(13));
                    col = col.push(soft_sm(
                        "Add a passkey in Bitwarden, then try again.".into(),
                    ));
                } else {
                    let mut list = column![].spacing(4.0);
                    for c in &pending.candidates {
                        let title_line = if c.name.is_empty() {
                            "Passkey".to_string()
                        } else {
                            c.name.clone()
                        };
                        let sub = c
                            .user_display_name
                            .as_deref()
                            .or(c.username.as_deref())
                            .filter(|s| !s.is_empty())
                            .unwrap_or(c.rp_id.as_str());
                        let row_body = column![
                            text(title_line).size(13).font(sola_kit::fonts::ui_medium()),
                            soft_sm(sub.to_string()),
                        ]
                        .spacing(2);
                        let id = c.cipher_id.clone();
                        let mut btn = button(row_body)
                            .padding(Padding::from([8, 10]))
                            .width(Length::Fill)
                            .style(|theme: &iced::Theme, status| {
                                let p = theme.extended_palette();
                                let bg = match status {
                                    iced::widget::button::Status::Hovered
                                    | iced::widget::button::Status::Pressed => {
                                        p.background.strong.color
                                    }
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
                            btn = btn.on_press(Msg::VaultPasskeyPick(id));
                        }
                        list = list.push(btn);
                    }
                    col = col.push(
                        scrollable(list)
                            .height(Length::Fixed(MATCH_LIST_H))
                            .width(Length::Fill),
                    );
                }
            }

            let cancel = kit_button::labeled(
                if self.vault_busy { "Signing…" } else { "Cancel" },
                kit_button::ghost,
            )
            .on_press(Msg::VaultPasskeyCancel);
            col = col.push(cancel);
            col.into()
        } else if self.vault_status.unlocked
            && matches!(self.vault_phase, VaultPanelPhase::CreateSaved)
        {
            let title = text("Saved to vault")
                .size(15)
                .font(sola_kit::fonts::ui_medium());
            let mut col = column![
                title,
                soft("No username or password field on this page.".into()),
            ]
            .spacing(SPACE_SM)
            .width(Length::Fixed(340.0));
            if let Some(err) = err_line {
                col = col.push(err);
            }
            col = col.push(
                kit_button::labeled("Close", kit_button::primary).on_press(Msg::VaultPanelClose),
            );
            col.into()
        } else if self.vault_status.unlocked
            && matches!(self.vault_phase, VaultPanelPhase::CreateLogin)
        {
            let title = text("New login")
                .size(15)
                .font(sola_kit::fonts::ui_medium());
            let busy = self.vault_busy;
            let mut username = text_input("Username", &self.vault_create_username)
                .id(vault_create_username_id())
                .size(13)
                .style(sola_kit::components::text_input::style)
                .width(Length::Fill);
            let mut password = text_input("Password", &self.vault_create_password)
                .id(vault_create_password_id())
                .size(13)
                .style(sola_kit::components::text_input::style)
                .width(Length::Fill);
            let mut url = text_input("URL", &self.vault_create_url)
                .id(vault_create_url_id())
                .size(13)
                .style(sola_kit::components::text_input::style)
                .width(Length::Fill);
            if !busy {
                username = username
                    .on_input(Msg::VaultCreateUsername)
                    .on_submit(Msg::VaultCreateSubmit);
                password = password
                    .on_input(Msg::VaultCreatePassword)
                    .on_submit(Msg::VaultCreateSubmit);
                url = url
                    .on_input(Msg::VaultCreateUrl)
                    .on_submit(Msg::VaultCreateSubmit);
            }
            let mut regen = kit_button::labeled_sm("Regenerate", kit_button::ghost);
            if !busy {
                regen = regen.on_press(Msg::VaultCreateRegenerate);
            }
            let password_row = column![
                field("Password", password, None, None),
                regen,
            ]
            .spacing(4.0);
            let mut create = kit_button::labeled(
                if busy { "Creating…" } else { "Create" },
                kit_button::primary,
            );
            if !busy {
                create = create.on_press(Msg::VaultCreateSubmit);
            }
            let cancel = kit_button::labeled("Cancel", kit_button::ghost)
                .on_press(Msg::VaultCreateCancel);
            let mut col = column![title]
                .spacing(SPACE_SM)
                .width(Length::Fixed(340.0));
            if let Some(err) = err_line {
                col = col.push(err);
            }
            col = col
                .push(field("Username", username, None, None))
                .push(password_row)
                .push(field("URL", url, None, None))
                .push(
                    row![create, cancel]
                        .spacing(SPACE_SM)
                        .align_y(Alignment::Center),
                );
            col.into()
        } else if self.vault_status.unlocked {
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

            // Wide enough for emails; tall enough that ~10–12 logins rarely scroll.
            const MATCH_LIST_H: f32 = 420.0;
            let mut col = column![title]
                .spacing(SPACE_SM)
                .width(Length::Fixed(340.0));

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
                col = col.push(text("No saved login for this site.").size(13));
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
                    let title_row: Element<'_, Msg> = if m.has_passkey {
                        row![
                            text(title_line)
                                .size(13)
                                .font(sola_kit::fonts::ui_medium()),
                            text("passkey")
                                .size(10)
                                .font(sola_kit::fonts::ui_medium())
                                .style(|theme: &iced::Theme| iced::widget::text::Style {
                                    color: Some(theme.extended_palette().primary.base.color),
                                }),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center)
                        .into()
                    } else {
                        text(title_line)
                            .size(13)
                            .font(sola_kit::fonts::ui_medium())
                            .into()
                    };
                    let row_body = column![title_row, soft_sm(sub.to_string())].spacing(2);
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
                        .height(Length::Fixed(MATCH_LIST_H))
                        .width(Length::Fill),
                );
            }

            let empty = self.vault_matches.is_empty() && !self.vault_matches_loading;
            let mut create = kit_button::labeled(
                "Create login",
                if empty {
                    kit_button::primary
                } else {
                    kit_button::ghost
                },
            );
            if !self.vault_busy {
                create = create.on_press(Msg::VaultCreateOpen);
            }
            let mut refresh = kit_button::labeled_sm("Refresh", kit_button::ghost);
            if !self.vault_busy && !self.vault_matches_loading {
                refresh = refresh.on_press(Msg::VaultRefreshMatches);
            }
            let close = kit_button::labeled("Close", kit_button::secondary)
                .on_press(Msg::VaultPanelClose);
            col = col.push(
                row![create, refresh, close]
                    .spacing(SPACE_SM)
                    .align_y(Alignment::Center),
            );
            col.into()
        } else {
            match &self.vault_phase {
                VaultPanelPhase::PasskeyPick => {
                    // Locked but phase stuck — fall through to credentials.
                    text("Unlock the vault to use a passkey.").size(13).into()
                }
                VaultPanelPhase::CreateLogin | VaultPanelPhase::CreateSaved => {
                    text("Unlock the vault to create a login.").size(13).into()
                }
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
                    ]
                    .spacing(SPACE_SM)
                    .width(Length::Fixed(300.0));
                    if self.pending_passkey.is_some() {
                        col = col.push(soft(
                            "A site asked for a passkey — unlock to choose one.".into(),
                        ));
                    }
                    col = col
                        .push(Space::new().height(SPACE_SM))
                        .push(email)
                        .push(password);

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
        // Slightly wider than the old 320 so fill list + passkey badge fit.
        let panel = card::modal(container(body).padding(SPACE_MD + SPACE_SM))
            .width(Length::Fixed(360.0));

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
                    CURSOR_X_BITS.store(position.x.to_bits(), Ordering::Relaxed);
                    // Rebuilding chrome on every pixel move starved menus and
                    // typing. Only the divider drag needs a message.
                    if DIVIDER_DRAGGING.load(Ordering::Relaxed) {
                        Some(Msg::CursorMoved(position.x))
                    } else {
                        None
                    }
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

/// Divider drag: `listen_with` is a fn pointer and cannot close over App.
static DIVIDER_DRAGGING: AtomicBool = AtomicBool::new(false);
/// Last pointer x (bits) so DividerPress has a current anchor without
/// emitting CursorMoved on every pixel.
static CURSOR_X_BITS: AtomicU32 = AtomicU32::new(0);

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

fn nav_icon_back() -> iced::widget::svg::Handle {
    static H: OnceLock<iced::widget::svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/arrow-left")).clone()
}

fn nav_icon_forward() -> iced::widget::svg::Handle {
    static H: OnceLock<iced::widget::svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/arrow-right")).clone()
}

fn nav_icon_reload() -> iced::widget::svg::Handle {
    static H: OnceLock<iced::widget::svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/rotate-cw")).clone()
}

fn nav_icon_stop() -> iced::widget::svg::Handle {
    static H: OnceLock<iced::widget::svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/x")).clone()
}

#[cfg(feature = "bitwarden")]
fn vault_password_id() -> iced::widget::Id {
    iced::widget::Id::new("sola-browser-vault-password")
}

#[cfg(feature = "bitwarden")]
fn vault_otp_id() -> iced::widget::Id {
    iced::widget::Id::new("sola-browser-vault-otp")
}

#[cfg(feature = "bitwarden")]
fn vault_create_username_id() -> iced::widget::Id {
    iced::widget::Id::new("sola-browser-vault-create-username")
}

#[cfg(feature = "bitwarden")]
fn vault_create_password_id() -> iced::widget::Id {
    iced::widget::Id::new("sola-browser-vault-create-password")
}

#[cfg(feature = "bitwarden")]
fn vault_create_url_id() -> iced::widget::Id {
    iced::widget::Id::new("sola-browser-vault-create-url")
}

impl<E: Engine> Drop for App<E> {
    fn drop(&mut self) {
        // Flush tab session before killing the worker.
        self.persist_session();
        // Orderly engine teardown on iced exit (Cmd::Quit + join worker).
        self.engine.shutdown();
    }
}

/// Empty / `about:blank` mid-navigation — do not flash these in the omnibar
/// over a URL the user just committed.
fn is_transient_nav_url(url: &str) -> bool {
    url.is_empty() || url == BLANK_URL
}

/// Apply an engine URL to the omnibar. Never blanks a committed field, and
/// never overwrites while the user is typing.
fn apply_omnibar_url(
    url_field: &str,
    last_seen_url: &str,
    engine_url: &str,
    url_bar_focused: bool,
) -> (String, String) {
    if url_bar_focused {
        return (url_field.to_string(), last_seen_url.to_string());
    }
    if is_transient_nav_url(engine_url) {
        return (url_field.to_string(), last_seen_url.to_string());
    }
    if engine_url == last_seen_url {
        return (url_field.to_string(), last_seen_url.to_string());
    }
    (engine_url.to_string(), engine_url.to_string())
}

/// 2px accent hairline along the bottom of the omnibox well.
fn omnibox_progress_overlay<'a>(frac: f32) -> Element<'a, Msg> {
    let fill_w = ((frac * 1000.0) as u16).max(1);
    let rest_w = ((1.0 - frac) * 1000.0).max(1.0) as u16;
    let fill = container(Space::new().width(Length::Fill).height(Length::Fixed(2.0)))
        .width(Length::FillPortion(fill_w))
        .height(Length::Fixed(2.0))
        .style(|theme: &iced::Theme| {
            let accent = theme.extended_palette().primary.base.color;
            iced::widget::container::Style {
                background: Some(iced::Background::Color(accent)),
                border: iced::Border {
                    radius: iced::border::Radius::new(0.0).bottom(RADIUS_MD),
                    ..Default::default()
                },
                ..Default::default()
            }
        });
    column![
        Space::new().height(Length::Fill),
        row![
            fill,
            Space::new()
                .width(Length::FillPortion(rest_w))
                .height(Length::Fixed(2.0)),
        ]
        .width(Length::Fill)
        .height(Length::Fixed(2.0)),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Chrome owns which tabs exist and their order. Engine owns field updates
/// (url / title / loading). Engine-only ids (popups) are appended unless
/// chrome already closed them.
fn merge_tab_snapshot(
    prev: &[TabInfo],
    live: &[TabInfo],
    closed: &HashSet<TabId>,
) -> Vec<TabInfo> {
    let mut out: Vec<TabInfo> = prev
        .iter()
        .filter(|p| !closed.contains(&p.id))
        .map(|p| match live.iter().find(|t| t.id == p.id) {
            Some(t) => merge_tab_fields(p, t),
            None => p.clone(),
        })
        .collect();
    for t in live {
        if closed.contains(&t.id) {
            continue;
        }
        if prev.iter().any(|p| p.id == t.id) {
            continue;
        }
        out.push(t.clone());
    }
    out
}

fn merge_tab_fields(prior: &TabInfo, live: &TabInfo) -> TabInfo {
    let title = if live.title.is_empty() && !prior.title.is_empty() {
        prior.title.clone()
    } else {
        live.title.clone()
    };
    let url = if is_transient_nav_url(&live.url) && !is_transient_nav_url(&prior.url) {
        prior.url.clone()
    } else {
        live.url.clone()
    };
    let is_loading =
        live.is_loading || (prior.is_loading && is_transient_nav_url(&live.url));
    let load_progress = if is_loading {
        live.load_progress.max(prior.load_progress)
    } else {
        0.0
    };
    TabInfo {
        id: prior.id,
        url,
        title,
        is_loading,
        can_go_back: live.can_go_back,
        can_go_forward: live.can_go_forward,
        load_progress,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: u64, url: &str, title: &str) -> TabInfo {
        TabInfo {
            id: TabId(id),
            url: url.to_string(),
            title: title.to_string(),
            is_loading: false,
            can_go_back: false,
            can_go_forward: false,
            load_progress: 0.0,
        }
    }

    #[test]
    fn merge_keeps_committed_url_over_blank() {
        let prev = vec![{
            let mut t = tab(1, "https://example.com/", "Example");
            t.is_loading = true;
            t
        }];
        let live = vec![tab(1, BLANK_URL, "")];
        let out = merge_tab_snapshot(&prev, &live, &HashSet::new());
        assert_eq!(out[0].url, "https://example.com/");
        assert_eq!(out[0].title, "Example");
        assert!(out[0].is_loading);
    }

    #[test]
    fn merge_takes_canonical_url() {
        let prev = vec![{
            let mut t = tab(1, "https://example.com/", "");
            t.is_loading = true;
            t
        }];
        let live = vec![{
            let mut t = tab(1, "https://www.example.com/", "Example Domain");
            t.is_loading = true;
            t.load_progress = 0.4;
            t
        }];
        let out = merge_tab_snapshot(&prev, &live, &HashSet::new());
        assert_eq!(out[0].url, "https://www.example.com/");
        assert_eq!(out[0].title, "Example Domain");
        assert_eq!(out[0].load_progress, 0.4);
    }

    #[test]
    fn merge_leaves_genuine_blank_tab() {
        let prev = vec![tab(1, BLANK_URL, "New Tab")];
        let live = vec![tab(1, BLANK_URL, "")];
        let out = merge_tab_snapshot(&prev, &live, &HashSet::new());
        assert_eq!(out[0].url, BLANK_URL);
        assert_eq!(out[0].title, "New Tab");
        assert!(!out[0].is_loading);
    }

    #[test]
    fn merge_does_not_resurrect_closed_tab() {
        let prev = vec![tab(1, "https://keep.example/", "Keep")];
        let live = vec![
            tab(1, "https://keep.example/", "Keep"),
            tab(2, "https://gone.example/", "Gone"),
        ];
        let closed = HashSet::from([TabId(2)]);
        let out = merge_tab_snapshot(&prev, &live, &closed);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, TabId(1));
    }

    #[test]
    fn merge_keeps_chrome_new_tab_before_engine() {
        let prev = vec![
            tab(1, "https://a.example/", "A"),
            tab(2, BLANK_URL, "New Tab"),
        ];
        let live = vec![tab(1, "https://a.example/", "A")];
        let out = merge_tab_snapshot(&prev, &live, &HashSet::new());
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].id, TabId(2));
        assert_eq!(out[1].title, "New Tab");
    }

    #[test]
    fn merge_appends_engine_popup() {
        let prev = vec![tab(1, "https://a.example/", "A")];
        let live = vec![
            tab(1, "https://a.example/", "A"),
            tab(3, "https://popup.example/", "Popup"),
        ];
        let out = merge_tab_snapshot(&prev, &live, &HashSet::new());
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].id, TabId(3));
    }

    #[test]
    fn omnibar_does_not_blank_mid_navigation() {
        let (field, seen) = apply_omnibar_url(
            "https://example.com/",
            "https://example.com/",
            BLANK_URL,
            false,
        );
        assert_eq!(field, "https://example.com/");
        assert_eq!(seen, "https://example.com/");
    }

    #[test]
    fn omnibar_swaps_to_canonical_instantly() {
        let (field, seen) = apply_omnibar_url(
            "https://example.com/",
            "https://example.com/",
            "https://www.example.com/",
            false,
        );
        assert_eq!(field, "https://www.example.com/");
        assert_eq!(seen, "https://www.example.com/");
    }

    #[test]
    fn omnibar_ignores_engine_while_focused() {
        let (field, seen) = apply_omnibar_url(
            "exa",
            BLANK_URL,
            "https://elsewhere.example/",
            true,
        );
        assert_eq!(field, "exa");
        assert_eq!(seen, BLANK_URL);
    }
}
