//! Browser chrome: message type, layout constants, and the generic `App<E>`.
//!
//! `Msg` and the consts were stubbed out in Task 1 and are kept here. Task 2
//! adds `App<E>`, its constructor, and all update/view/subscription methods.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use iced::widget::{
    Shader, Space, button, column, container, mouse_area, row, scrollable, stack, text,
};
use iced::{
    Alignment, Element, Event, Length, Padding, Subscription, Task, event, keyboard, mouse,
};
use sola_kit::components::button as kit_button;
use sola_kit::components::card;
use sola_kit::components::divider::DIVIDER_HIT_PX;
use sola_kit::components::icon::{icon_handle, icon_svg, icon_svg_colored};
use sola_kit::components::select::{SelectOption, select_sized};
use sola_kit::components::style::{CHROME_SURFACE, PAD_CONTROL_SM, RADIUS_MD};
use sola_kit::components::text_input::text_input;
use sola_kit::components::toolbar as kit_toolbar;
use sola_kit::components::{
    DividerColors, MenuItem, PANEL_REORDER_THRESHOLD, PANEL_ROW_H, ReorderAnim, ReorderCfg,
    SidebarDensity, SidebarItem, SidebarPanel, SidebarSection, field, horizontal_divider, menu_at,
    panel_drop_index_relative,
};

use crate::engine::{
    Cmd, EditCmd, Engine, FrameSlot, NavCmd, PageContext, PageMenusHandle, TabId, TabInfo,
    TabsHandle,
};
use crate::groups::Groups;
use crate::page_menu::{self, PageMenuKind};
use crate::session::{self, SessionGroup, SessionTab};
#[cfg(feature = "bitwarden")]
use crate::vault::{
    CardSummary, MatchSummary, PasskeyCandidate, PasskeyPageRequest, TotpSummary, TwoFactorKind,
    VaultCmd, VaultEvent, VaultHandle, VaultStatus, apex_domain, create_account_hint,
    fill_card_script, fill_credentials_script, fill_credentials_script_ex, fill_totp_script,
    generate_password, totp_remaining_secs,
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
const NAV_BTN_W: f32 = 34.0;
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
    /// Press on back/forward — starts a hold timer for the history menu.
    NavHoldStart(HistoryDir),
    /// Hold elapsed — open the session-history menu.
    NavHoldFire,
    /// Jump to a session-history index (from the hold menu).
    NavJump(i32),
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
    /// Press on a tab row (potential reorder), carrying the strip index.
    ReorderStart(usize),
    /// Animation tick while a tab reorder drag is live (sibling glides).
    ReorderTick,
    /// Global cursor moved — acted on while dragging the divider or
    /// reordering tabs. `(x, y)` in window-logical pixels.
    CursorMoved(f32, f32),
    /// Global left-button released — ends a divider drag or tab reorder.
    CursorReleased,
    /// Hovered tab row changed (string id of [`TabId`]), or `None`.
    TabHover(Option<String>),
    ToggleGroup(String),
    TabContext(TabId),
    GroupContext(String),
    MenuDismiss,
    NewGroup(TabId),
    AddToGroup {
        tab: TabId,
        group: String,
    },
    UngroupTab(TabId),
    UngroupGroup(String),
    RenameStart(String),
    RenameInput(String),
    RenameCommit,
    RenameCancel,
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
    EditRouted {
        cmd: EditCmd,
        url_bar_focused: bool,
    },
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
    /// Toolbar credit-card: open the cards panel (unlock first if needed).
    #[cfg(feature = "bitwarden")]
    CardsToggle,
    #[cfg(feature = "bitwarden")]
    CardsFill(String),
    #[cfg(feature = "bitwarden")]
    CardsRefresh,
    /// Toolbar authenticator: TOTP codes (unlock first if needed).
    #[cfg(feature = "bitwarden")]
    TotpToggle,
    #[cfg(feature = "bitwarden")]
    TotpFill(String),
    #[cfg(feature = "bitwarden")]
    TotpRefresh,
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
    /// Confirm `credentials.create` as a new personal login.
    #[cfg(feature = "bitwarden")]
    VaultPasskeyCreateNew,
    /// Confirm `credentials.create` attached to an existing login.
    #[cfg(feature = "bitwarden")]
    VaultPasskeyCreateOn(String),
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
    /// Toolbar download icon: open / close the downloads panel.
    DownloadsToggle,
    DownloadsPanelClose,
    /// Cancel an in-progress download (`entry.id`).
    DownloadCancel(String),
    /// Open a completed file (`entry.id`).
    DownloadOpen(String),
    /// Drop a row from the list (file stays on disk).
    DownloadRemove(String),
    /// Page context-menu action (after CEF cancelled the native OSR menu).
    PageMenu(PageMenuAction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryDir {
    Back,
    Forward,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageMenuAction {
    OpenLink(String),
    CopyLink(String),
    Copy(String),
    Cut,
    Paste,
    Back,
    Forward,
    Reload,
}

#[derive(Debug, Clone)]
enum CtxTarget {
    Tab(TabId),
    Group(String),
    Page(PageContext),
    History { forward: bool },
}

struct NavHold {
    dir: HistoryDir,
    started: Instant,
    menu: bool,
}

const NAV_HOLD_MS: u128 = 400;

/// In-chrome dialog for creating / renaming / confirming delete of a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileDialog {
    New,
    Rename,
    DeleteConfirm,
}

/// After unlock, which chrome panel to show.
#[cfg(feature = "bitwarden")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum VaultResume {
    #[default]
    Logins,
    Cards,
    Totp,
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
    /// Site asked to register a passkey — confirm + optional attach.
    PasskeyCreate,
    /// Compose a new login (username / generated password / apex URL).
    CreateLogin,
    /// Cipher saved; page had no fields to fill.
    CreateSaved,
}

/// In-flight WebAuthn get() / create() waiting for the user.
#[cfg(feature = "bitwarden")]
#[derive(Debug, Clone)]
struct PendingPasskey {
    req: PasskeyPageRequest,
    /// Extra page promise ids for the same origin/RP (duplicate
    /// delivery or a same-ceremony retry). Resolved together so the
    /// site does not see "Superseded" / NotAllowedError mid-pick.
    extra_ids: Vec<u64>,
    candidates: Vec<PasskeyCandidate>,
    loading: bool,
    error: Option<String>,
}

#[cfg(feature = "bitwarden")]
impl PendingPasskey {
    fn all_ids(&self) -> Vec<u64> {
        let mut ids = Vec::with_capacity(1 + self.extra_ids.len());
        ids.push(self.req.id);
        for id in &self.extra_ids {
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
        ids
    }

    fn is_create(&self) -> bool {
        self.req.is_create()
    }
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
    /// Hovered tab id (string form of [`TabId`]) — drives the float-in
    /// close button. Wired through [`sola_kit::components::SidebarPanel::item_hover`].
    pub hovered_tab: Option<String>,
    /// Active tab-reorder gesture (`from_index`, start_y). `start_y` is
    /// `0.0` until the first cursor sample after press (same sentinel as
    /// the terminal strip).
    reorder: Option<(usize, f32)>,
    reorder_cursor_y: f32,
    /// True once movement has crossed [`PANEL_REORDER_THRESHOLD`].
    reorder_dragging: bool,
    reorder_anim: ReorderAnim,
    groups: Groups,
    context_menu: Option<(iced::Point, CtxTarget)>,
    nav_hold: Option<NavHold>,
    page_menus: PageMenusHandle,
    renaming: Option<(String, String)>,
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
    pending_session: Option<(Vec<SessionTab>, usize, Vec<SessionGroup>)>,
    /// Bitwarden vault worker + login panel state.
    #[cfg(feature = "bitwarden")]
    vault: VaultHandle,
    #[cfg(feature = "bitwarden")]
    vault_panel_open: bool,
    /// Separate cards panel (mutually exclusive with the login panel).
    #[cfg(feature = "bitwarden")]
    cards_panel_open: bool,
    /// Authenticator / TOTP panel.
    #[cfg(feature = "bitwarden")]
    totp_panel_open: bool,
    #[cfg(feature = "bitwarden")]
    vault_resume: VaultResume,
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
    #[cfg(feature = "bitwarden")]
    cards_icon: iced::widget::svg::Handle,
    #[cfg(feature = "bitwarden")]
    vault_cards: Vec<CardSummary>,
    #[cfg(feature = "bitwarden")]
    vault_cards_loading: bool,
    #[cfg(feature = "bitwarden")]
    totp_icon: iced::widget::svg::Handle,
    #[cfg(feature = "bitwarden")]
    vault_totp: Vec<TotpSummary>,
    #[cfg(feature = "bitwarden")]
    vault_totp_loading: bool,
    /// Fill the MRU authenticator once the list arrives.
    #[cfg(feature = "bitwarden")]
    totp_autofill_pending: bool,
    /// Next `TotpFillReady` came from a row click — copy the code.
    #[cfg(feature = "bitwarden")]
    totp_copy_next: bool,
    #[cfg(feature = "bitwarden")]
    pending_totp_clipboard: Option<String>,
    #[cfg(feature = "bitwarden")]
    totp_last_refresh_secs: u64,
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
    downloads: crate::downloads::DownloadList,
    downloads_panel_open: bool,
    download_icon: iced::widget::svg::Handle,
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
        session_groups: Vec<SessionGroup>,
    ) -> Self {
        let page_menus = engine.page_menus_handle();
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
            reorder: None,
            reorder_cursor_y: 0.0,
            reorder_dragging: false,
            reorder_anim: ReorderAnim::new(),
            groups: Groups::default(),
            context_menu: None,
            nav_hold: None,
            page_menus,
            renaming: None,
            float: sola_kit::FloatState::new(app_id),
            window_id: None,
            app_id,
            url_bar_focused: false,
            session_fp: String::new(),
            pending_session: Some((tabs, active_index, session_groups)),
            #[cfg(feature = "bitwarden")]
            vault: VaultHandle::spawn(),
            #[cfg(feature = "bitwarden")]
            vault_panel_open: false,
            #[cfg(feature = "bitwarden")]
            cards_panel_open: false,
            #[cfg(feature = "bitwarden")]
            totp_panel_open: false,
            #[cfg(feature = "bitwarden")]
            vault_resume: VaultResume::Logins,
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
            cards_icon: icon_handle("lucide/credit-card"),
            #[cfg(feature = "bitwarden")]
            totp_icon: icon_handle("lucide/shield"),
            #[cfg(feature = "bitwarden")]
            vault_totp: Vec::new(),
            #[cfg(feature = "bitwarden")]
            vault_totp_loading: false,
            #[cfg(feature = "bitwarden")]
            totp_autofill_pending: false,
            #[cfg(feature = "bitwarden")]
            totp_copy_next: false,
            #[cfg(feature = "bitwarden")]
            pending_totp_clipboard: None,
            #[cfg(feature = "bitwarden")]
            totp_last_refresh_secs: 0,
            #[cfg(feature = "bitwarden")]
            vault_cards: Vec::new(),
            #[cfg(feature = "bitwarden")]
            vault_cards_loading: false,
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
            downloads: crate::downloads::DownloadList::load(),
            downloads_panel_open: false,
            download_icon: icon_handle("lucide/download"),
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
        if open {
            self.set_downloads_panel_open(false);
            self.cards_panel_open = false;
            self.totp_panel_open = false;
        }
    }

    fn set_downloads_panel_open(&mut self, open: bool) {
        self.downloads_panel_open = open;
        if open {
            self.downloads.mark_seen();
            #[cfg(feature = "bitwarden")]
            {
                self.vault_panel_open = false;
                VAULT_PANEL_OPEN.store(false, Ordering::Relaxed);
                self.cards_panel_open = false;
                self.totp_panel_open = false;
            }
        }
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
    fn request_vault_cards(&mut self) {
        if !self.vault_status.unlocked {
            self.vault_cards.clear();
            self.vault_cards_loading = false;
            return;
        }
        self.vault_cards_loading = true;
        self.vault.send(VaultCmd::ListCards);
    }

    #[cfg(feature = "bitwarden")]
    fn set_cards_panel_open(&mut self, open: bool) {
        self.cards_panel_open = open;
        if open {
            self.set_downloads_panel_open(false);
            self.set_vault_panel_open(false);
            self.totp_panel_open = false;
        }
    }

    #[cfg(feature = "bitwarden")]
    fn set_totp_panel_open(&mut self, open: bool) {
        self.totp_panel_open = open;
        if open {
            self.set_downloads_panel_open(false);
            self.set_vault_panel_open(false);
            self.cards_panel_open = false;
        }
    }

    #[cfg(feature = "bitwarden")]
    fn request_vault_totp(&mut self) {
        if !self.vault_status.unlocked {
            self.vault_totp.clear();
            self.vault_totp_loading = false;
            return;
        }
        let url = self
            .active_tab_info()
            .map(|t| t.url.clone())
            .unwrap_or_default();
        self.vault_totp_loading = true;
        self.vault.send(VaultCmd::ListTotp { url });
    }

    #[cfg(feature = "bitwarden")]
    fn open_create_login(&mut self) {
        let page_url = self.active_tab_info().map(|t| t.url.as_str()).unwrap_or("");
        self.vault_create_url = if page_url.is_empty() || page_url == BLANK_URL {
            String::new()
        } else {
            apex_domain(page_url)
        };
        self.vault_create_username =
            crate::vault::VaultPrefs::load_last_username().unwrap_or_default();
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
            if apex.is_empty() { uri.clone() } else { apex }
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
    fn bootstrap_tabs(
        &mut self,
        tabs: Vec<SessionTab>,
        active_index: usize,
        session_groups: Vec<SessionGroup>,
    ) {
        debug_assert!(!tabs.is_empty(), "bootstrap always has ≥1 tab");
        let session_tabs = tabs;
        let mut ids = Vec::with_capacity(session_tabs.len());
        for tab in &session_tabs {
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
            let (history, history_index) = session::history_from_session(tab);
            self.cached_tabs.push(TabInfo {
                is_loading: url != BLANK_URL && !url.is_empty(),
                history,
                history_index,
                ..TabInfo::chrome(id, url.clone(), title.clone())
            });
            // One background frame may be imported to seed park cache.
            self.slot.need_park_prime.lock().unwrap().insert(id.0);
            let _ = self.cmd_tx.send(Cmd::OpenTab { id, url, title });
            ids.push(id);
        }
        let active = ids
            .get(active_index)
            .copied()
            .or_else(|| ids.first().copied())
            .unwrap_or(TabId(0));
        self.switch_active_tab(active);
        self.groups = Groups::restore(&session_tabs, &ids, &session_groups);
        self.groups.normalize(&mut self.cached_tabs);
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
                    groups: self.groups.clone(),
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
                self.groups = snap.groups;
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
        let session = crate::session::BrowserSession::load();
        let session_groups = session.groups.clone();
        let (tabs, active_index, sidebar_w) = session.bootstrap(None, BLANK_URL);
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
            let (history, history_index) = session::history_from_session(tab);
            new_cached.push(TabInfo {
                is_loading: url != BLANK_URL && !url.is_empty(),
                history,
                history_index,
                ..TabInfo::chrome(id, url.clone(), title.clone())
            });
            open_list.push((id, url, title));
        }
        if open_list.is_empty() {
            let id = self.engine.alloc_tab_id();
            new_cached.push(TabInfo::chrome(id, BLANK_URL, "New Tab"));
            open_list.push((id, BLANK_URL.to_string(), "New Tab".into()));
        }
        let active = open_list
            .get(active_index.min(open_list.len() - 1))
            .map(|(id, _, _)| *id)
            .unwrap_or(open_list[0].0);

        self.cached_tabs = new_cached;
        self.cached_active = active;
        let ids: Vec<TabId> = self.cached_tabs.iter().map(|t| t.id).collect();
        self.groups = Groups::restore(&tabs, &ids, &session_groups);
        self.groups.normalize(&mut self.cached_tabs);
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
        let session = session::session_from_tabs(&tabs, active, self.sidebar_w, &self.groups);
        let fp = session::fingerprint(&session);
        if fp == self.session_fp {
            return;
        }
        session.save();
        self.session_fp = fp;
    }

    /// Drive sibling-offset animations for the live tab-reorder preview.
    fn sync_reorder_anim(&mut self) {
        let Some((from, start_y)) = self.reorder else {
            return;
        };
        if !self.reorder_dragging {
            return;
        }
        let n = self.groups.visible_rows(&self.cached_tabs).len();
        if n == 0 {
            return;
        }
        let to = panel_drop_index_relative(from, start_y, self.reorder_cursor_y, PANEL_ROW_H, n);
        self.reorder_anim
            .sync(from, to, n, iced::time::Instant::now());
    }

    /// Finish a strip gesture: click → activate / toggle; drag → drop.
    fn finish_reorder(&mut self) {
        let gesture = self.reorder.take();
        let final_cursor_y = self.reorder_cursor_y;
        let was_dragging = self.reorder_dragging;
        self.reorder_cursor_y = 0.0;
        self.reorder_dragging = false;
        self.reorder_anim.clear();
        REORDER_TRACKING.store(false, Ordering::Relaxed);

        let Some((from, start_y)) = gesture else {
            return;
        };

        let rows = self.groups.visible_rows(&self.cached_tabs);
        if !was_dragging || start_y == 0.0 {
            match rows.get(from) {
                Some(crate::groups::StripRow::Tab(id)) => {
                    self.switch_active_tab(*id);
                    self.persist_session();
                }
                Some(crate::groups::StripRow::Header(gid)) => {
                    self.groups.toggle(gid);
                    self.persist_session();
                }
                None => {}
            }
            return;
        }

        let n = rows.len();
        if n == 0 {
            return;
        }
        let to = panel_drop_index_relative(from, start_y, final_cursor_y, PANEL_ROW_H, n);
        if from == to {
            return;
        }
        self.groups.apply_drop(&mut self.cached_tabs, from, to);
        self.persist_session();
    }

    pub fn active_tab_info(&self) -> Option<&TabInfo> {
        self.cached_tabs.iter().find(|t| t.id == self.cached_active)
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::WindowReady(id) => {
                self.window_id = id;
                // Iced is connected; open restored tabs now.
                if let Some((tabs, active_index, groups)) = self.pending_session.take() {
                    self.bootstrap_tabs(tabs, active_index, groups);
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
                    self.vault_resume = VaultResume::Logins;
                    self.cards_panel_open = false;
                    self.totp_panel_open = false;
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
            #[cfg(feature = "bitwarden")]
            Msg::CardsToggle => {
                if self.cards_panel_open {
                    self.set_cards_panel_open(false);
                    return Task::none();
                }
                self.vault_error = None;
                self.vault_resume = VaultResume::Cards;
                if self.vault_status.unlocked {
                    self.set_cards_panel_open(true);
                    self.request_vault_cards();
                    return Task::none();
                }
                // Unlock first (same form as the key button), then open cards.
                self.cards_panel_open = false;
                self.set_vault_panel_open(true);
                if !self.vault_email.trim().is_empty() {
                    self.vault_paste_target = VaultPasteTarget::Password;
                    return iced::widget::operation::focus(vault_password_id());
                }
                return iced::widget::operation::focus(vault_email_id());
            }
            #[cfg(feature = "bitwarden")]
            Msg::CardsFill(id) => {
                if self.vault_busy || !self.vault_status.unlocked {
                    return Task::none();
                }
                self.vault_busy = true;
                self.vault_error = None;
                self.vault.send(VaultCmd::FillCard { id });
            }
            #[cfg(feature = "bitwarden")]
            Msg::CardsRefresh => {
                self.request_vault_cards();
            }
            #[cfg(feature = "bitwarden")]
            Msg::TotpToggle => {
                if self.totp_panel_open {
                    self.set_totp_panel_open(false);
                    return Task::none();
                }
                self.vault_error = None;
                self.vault_resume = VaultResume::Totp;
                if self.vault_status.unlocked {
                    self.totp_autofill_pending = true;
                    self.set_totp_panel_open(true);
                    self.request_vault_totp();
                    return Task::none();
                }
                self.totp_panel_open = false;
                self.set_vault_panel_open(true);
                if !self.vault_email.trim().is_empty() {
                    self.vault_paste_target = VaultPasteTarget::Password;
                    return iced::widget::operation::focus(vault_password_id());
                }
                return iced::widget::operation::focus(vault_email_id());
            }
            #[cfg(feature = "bitwarden")]
            Msg::TotpFill(id) => {
                if self.vault_busy || !self.vault_status.unlocked {
                    return Task::none();
                }
                self.vault_busy = true;
                self.vault_error = None;
                self.totp_copy_next = true;
                self.vault.send(VaultCmd::FillTotp { id });
            }
            #[cfg(feature = "bitwarden")]
            Msg::TotpRefresh => {
                self.request_vault_totp();
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
            Msg::VaultPasskeyCreateNew => {
                #[cfg(feature = "bitwarden")]
                {
                    self.confirm_passkey_create(None);
                }
            }
            Msg::VaultPasskeyCreateOn(cipher_id) => {
                #[cfg(feature = "bitwarden")]
                {
                    self.confirm_passkey_create(Some(cipher_id));
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
                        self.vault_error = Some("Email and master password are required.".into());
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
                        | VaultPanelPhase::PasskeyCreate
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
                        | VaultPanelPhase::PasskeyCreate
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
                        self.set_cards_panel_open(false);
                        self.set_totp_panel_open(false);
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
            Msg::DownloadsToggle => {
                let open = !self.downloads_panel_open;
                self.set_downloads_panel_open(open);
            }
            Msg::DownloadsPanelClose => {
                self.set_downloads_panel_open(false);
            }
            Msg::DownloadCancel(id) => {
                if let Some(e) = self.downloads.items().iter().find(|e| e.id == id).cloned() {
                    let _ = self.cmd_tx.send(Cmd::CancelDownload {
                        profile_id: e.profile_id.clone(),
                        id: e.cef_id,
                    });
                    // Optimistic: CEF will also emit Canceled; drop_live is idempotent.
                    self.downloads.apply(
                        &e.profile_id,
                        crate::cef::ipc::DownloadEvent {
                            id: e.cef_id,
                            filename: e.filename.clone(),
                            path: e.path.to_string_lossy().into_owned(),
                            url: e.url.clone(),
                            received: e.received as i64,
                            total: e.total.map(|t| t as i64).unwrap_or(-1),
                            percent: e.percent.map(|p| (p * 100.0) as i32).unwrap_or(-1),
                            state: crate::cef::ipc::DownloadPhase::Canceled,
                        },
                        self.downloads_panel_open,
                    );
                }
            }
            Msg::DownloadOpen(id) => {
                if let Some(e) = self.downloads.items().iter().find(|e| e.id == id) {
                    if e.status == crate::downloads::DownloadStatus::Complete {
                        let _ = crate::downloads::open_file(&e.path);
                    }
                }
            }
            Msg::DownloadRemove(id) => {
                self.downloads.remove(&id);
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
                self.nav_hold = None;
                return self.nav_step(HistoryDir::Back);
            }
            Msg::NavForward => {
                self.nav_hold = None;
                return self.nav_step(HistoryDir::Forward);
            }
            Msg::NavHoldStart(dir) => {
                self.nav_hold = Some(NavHold {
                    dir,
                    started: Instant::now(),
                    menu: false,
                });
            }
            Msg::NavHoldFire => {
                if self
                    .nav_hold
                    .as_ref()
                    .is_some_and(|h| h.started.elapsed().as_millis() >= NAV_HOLD_MS)
                {
                    self.open_history_menu();
                }
            }
            Msg::NavJump(index) => {
                self.context_menu = None;
                self.nav_hold = None;
                let info = self.active_tab_info();
                let current = info.map(|t| t.history_index).unwrap_or(0);
                let delta = index - current;
                let target_url = info.and_then(|t| {
                    t.history
                        .iter()
                        .find(|e| e.index == index)
                        .map(|e| e.url.clone())
                });
                let cef_can = info.is_some_and(|t| {
                    if delta < 0 {
                        t.can_go_back
                    } else {
                        t.can_go_forward
                    }
                });
                if delta != 0 {
                    self.set_active_loading(true);
                    if cef_can {
                        let _ = self.cmd_tx.send(Cmd::Nav(NavCmd::GoHistory { delta }));
                    } else if let Some(url) = target_url {
                        let _ = self.cmd_tx.send(Cmd::Nav(NavCmd::LoadUrl(url)));
                    }
                }
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
                if self.renaming.is_some() {
                    self.renaming = None;
                    return Task::none();
                }
                if self.context_menu.is_some() {
                    self.context_menu = None;
                    return Task::none();
                }
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
                if self.downloads_panel_open {
                    self.set_downloads_panel_open(false);
                    return Task::none();
                }
                #[cfg(feature = "bitwarden")]
                if self.vault_panel_open || self.cards_panel_open || self.totp_panel_open {
                    // Dismiss panel only — keep login / 2FA state for re-open.
                    self.set_vault_panel_open(false);
                    self.set_cards_panel_open(false);
                    self.set_totp_panel_open(false);
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
                self.slot.forget_tab(id);
                self.slot.drop_paint_tabs.lock().unwrap().push(id.0);
                self.slot.need_park_prime.lock().unwrap().remove(&id.0);
                let _ = self.cmd_tx.send(Cmd::CloseTab(id));
                // Drop from optimistic cache immediately so persist sees it
                // and Tick cannot paint the row again.
                self.closed_tabs.insert(id);
                self.cached_tabs.retain(|t| t.id != id);
                self.groups.on_tab_closed(id);
                if self.hovered_tab.as_deref() == Some(&id.0.to_string()) {
                    self.hovered_tab = None;
                }
                self.persist_session();
            }
            Msg::ActivateTab(id) => {
                self.switch_active_tab(id);
                self.persist_session();
            }
            Msg::Tick => {
                while let Some(h) = crate::instance::try_recv_handoff() {
                    match h {
                        crate::instance::Handoff::OpenUrl(url) => {
                            tracing::info!(%url, "opening handed-off URL in this chrome");
                            self.open_tab(url, true);
                        }
                        crate::instance::Handoff::Activate => {
                            tracing::info!("activate handoff — chrome already front");
                        }
                    }
                }
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
                    if self.totp_panel_open && self.vault_status.unlocked {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        if now != self.totp_last_refresh_secs {
                            self.totp_last_refresh_secs = now;
                            self.request_vault_totp();
                        }
                    }
                }
                self.take_page_menu();
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
                {
                    let evs: Vec<_> = self
                        .engine
                        .downloads_handle()
                        .lock()
                        .unwrap()
                        .drain(..)
                        .collect();
                    for (profile_id, ev) in evs {
                        self.downloads
                            .apply(&profile_id, ev, self.downloads_panel_open);
                    }
                }
                #[cfg(feature = "bitwarden")]
                {
                    let pks: Vec<_> = self
                        .engine
                        .passkeys_handle()
                        .lock()
                        .unwrap()
                        .drain(..)
                        .collect();
                    for ev in pks {
                        self.dispatch_passkey_request(crate::vault::PasskeyPageRequest {
                            id: ev.id,
                            action: ev.action,
                            origin: ev.origin,
                            rp_id: ev.rp_id,
                            public_key_json: ev.public_key_json,
                        });
                    }
                }
                // ⌘-click / popup: helper found a URL; chrome owns the tab id.
                let bg: Vec<String> = self
                    .engine
                    .background_tabs_handle()
                    .lock()
                    .unwrap()
                    .drain(..)
                    .collect();
                for url in bg {
                    tracing::info!(%url, "cmd-click → background tab");
                    self.open_tab(url, false);
                }
                // Drain any page-selection / in-page copy the engine extracted.
                // The engine's own clipboard can't reach Wayland; iced's can.
                let clip = self.take_page_clipboard();
                #[cfg(feature = "bitwarden")]
                let totp_clip = self
                    .pending_totp_clipboard
                    .take()
                    .map(iced::clipboard::write)
                    .unwrap_or_else(Task::none);
                #[cfg(feature = "bitwarden")]
                if focus_otp {
                    return Task::batch([
                        clip,
                        totp_clip,
                        iced::widget::operation::focus(vault_otp_id()),
                    ]);
                }
                #[cfg(feature = "bitwarden")]
                return Task::batch([clip, totp_clip]);
                #[cfg(not(feature = "bitwarden"))]
                return clip;
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
            Msg::ReorderStart(index) => {
                // start_y = 0.0 sentinel; captured on first CursorMoved.
                // Live-reorder stays off until movement crosses the threshold.
                self.reorder = Some((index, 0.0));
                self.reorder_cursor_y = 0.0;
                self.reorder_dragging = false;
                self.reorder_anim.clear();
                REORDER_TRACKING.store(true, Ordering::Relaxed);
            }
            Msg::ReorderTick => {
                if self.reorder_dragging {
                    self.sync_reorder_anim();
                }
                self.take_page_menu();
                return self.take_page_clipboard();
            }
            Msg::CursorMoved(x, y) => {
                self.last_cursor_x = Some(x);
                if self.dragging_divider {
                    if let Some((anchor_x, anchor_w)) = self.drag_anchor {
                        // Sidebar is on the LEFT: it grows as the cursor
                        // moves right of the anchor, shrinks moving left.
                        let desired = anchor_w + (x - anchor_x);
                        self.sidebar_w = desired.clamp(SIDEBAR_W_MIN, SIDEBAR_W_MAX);
                    }
                }
                if let Some((_from, ref mut start_y)) = self.reorder {
                    if *start_y == 0.0 {
                        *start_y = y;
                    }
                    self.reorder_cursor_y = y;
                    if (y - *start_y).abs() >= PANEL_REORDER_THRESHOLD {
                        self.reorder_dragging = true;
                    }
                    if self.reorder_dragging {
                        self.sync_reorder_anim();
                    }
                }
            }
            // sidebar width / tab order persisted on Tick / drop
            Msg::CursorReleased => {
                if self.dragging_divider {
                    self.dragging_divider = false;
                    self.drag_anchor = None;
                }
                DIVIDER_DRAGGING.store(false, Ordering::Relaxed);
                if self.reorder.is_some() {
                    self.finish_reorder();
                }
                return self.finish_nav_hold();
            }
            Msg::TabHover(i) => self.hovered_tab = i,
            Msg::ToggleGroup(id) => {
                self.groups.toggle(&id);
                self.persist_session();
            }
            Msg::TabContext(id) => {
                self.context_menu = Some((last_cursor_point(), CtxTarget::Tab(id)));
            }
            Msg::GroupContext(id) => {
                self.context_menu = Some((last_cursor_point(), CtxTarget::Group(id)));
            }
            Msg::MenuDismiss => {
                self.context_menu = None;
                self.nav_hold = None;
            }
            Msg::NewGroup(id) => {
                self.context_menu = None;
                self.groups.new_group(id);
                self.groups.normalize(&mut self.cached_tabs);
                self.persist_session();
            }
            Msg::AddToGroup { tab, group } => {
                self.context_menu = None;
                self.groups.add_to(tab, &group);
                self.groups.normalize(&mut self.cached_tabs);
                self.persist_session();
            }
            Msg::UngroupTab(id) => {
                self.context_menu = None;
                self.groups.ungroup_tab(id);
                self.groups.normalize(&mut self.cached_tabs);
                self.persist_session();
            }
            Msg::UngroupGroup(id) => {
                self.context_menu = None;
                self.groups.ungroup_all(&id);
                self.groups.normalize(&mut self.cached_tabs);
                self.persist_session();
            }
            Msg::RenameStart(id) => {
                self.context_menu = None;
                let name = self
                    .groups
                    .group(&id)
                    .map(|g| g.name.clone())
                    .unwrap_or_default();
                self.renaming = Some((id, name));
            }
            Msg::RenameInput(s) => {
                if let Some((_, draft)) = &mut self.renaming {
                    *draft = s;
                }
            }
            Msg::RenameCommit => {
                if let Some((id, name)) = self.renaming.take() {
                    self.groups.rename(&id, name);
                    self.persist_session();
                }
            }
            Msg::RenameCancel => self.renaming = None,
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
            Msg::EditRouted {
                cmd,
                url_bar_focused,
            } => {
                // Vault panel owns Edit shortcuts while open — shell grabs ⌘V
                // globally and would otherwise paste into the page.
                #[cfg(feature = "bitwarden")]
                if self.vault_panel_open {
                    tracing::debug!(?cmd, "edit → vault panel");
                    return match cmd {
                        EditCmd::Paste => iced::clipboard::read().map(Msg::VaultClipboardPaste),
                        EditCmd::SelectAll => match self.vault_paste_target {
                            VaultPasteTarget::Email => {
                                iced::widget::operation::focus(vault_email_id())
                            }
                            VaultPasteTarget::Password => {
                                iced::widget::operation::focus(vault_password_id())
                            }
                            VaultPasteTarget::Otp => iced::widget::operation::focus(vault_otp_id()),
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
                            match crate::util::usable_clipboard_text(Some(self.url_field.clone())) {
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
                    // Focused-frame JS insert via PasteText — not EvaluateJs
                    // (that runs in every frame and triple-pastes).
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
                let _ = self.cmd_tx.send(Cmd::PasteText(s.clone()));
                return iced::clipboard::write(s);
            }
            Msg::PageMenu(action) => {
                self.context_menu = None;
                return self.run_page_menu(action);
            }
            #[cfg(feature = "bitwarden")]
            Msg::VaultClipboardPaste(text) => {
                let Some(cleaned) = crate::util::usable_clipboard_text(text) else {
                    return Task::none();
                };
                match self.vault_phase {
                    VaultPanelPhase::TwoFactor { .. } => {
                        self.vault_otp = cleaned.chars().filter(|c| !c.is_whitespace()).collect();
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
                    VaultPanelPhase::PasskeyPick
                    | VaultPanelPhase::PasskeyCreate
                    | VaultPanelPhase::CreateSaved => {
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
        let info = TabInfo {
            is_loading: url != BLANK_URL && !url.is_empty(),
            ..TabInfo::chrome(id, url.clone(), title.clone())
        };
        self.groups
            .insert_beside(&mut self.cached_tabs, self.cached_active, info);
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
            && !self.downloads_panel_open
            && {
                #[cfg(feature = "bitwarden")]
                {
                    !self.vault_panel_open && !self.cards_panel_open && !self.totp_panel_open
                }
                #[cfg(not(feature = "bitwarden"))]
                {
                    true
                }
            };
        let webview = crate::cef::page_ime::page_ime(webview, self.slot.clone(), page_owns_keys);

        // Full-width chrome (profile + nav + omnibox), then tabs | page.
        // SidebarPanel owns the kit divider + drag overlay.
        let lower = row![
            self.view_tab_sidebar(),
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
        let canvas = iced::Color { a: 1.0, ..canvas };
        let content: Element<'_, Msg> = container(main)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_t: &iced::Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(canvas)),
                ..iced::widget::container::Style::default()
            })
            .into();

        let content: Element<'_, Msg> = if self.downloads_panel_open {
            stack![content, self.view_downloads_panel()].into()
        } else {
            content
        };

        #[cfg(feature = "bitwarden")]
        let content: Element<'_, Msg> = if self.totp_panel_open {
            stack![content, self.view_totp_panel()].into()
        } else if self.cards_panel_open {
            stack![content, self.view_cards_panel()].into()
        } else if self.vault_panel_open {
            stack![content, self.view_vault_panel()].into()
        } else {
            content
        };

        let content: Element<'_, Msg> = if self.profile_dialog.is_some() {
            stack![content, self.view_profile_dialog()].into()
        } else {
            content
        };

        let content: Element<'_, Msg> = if let Some((at, target)) = &self.context_menu {
            stack![
                content,
                menu_at(*at, self.menu_items(target), Msg::MenuDismiss)
            ]
            .into()
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

    fn menu_items(&self, target: &CtxTarget) -> Vec<MenuItem<Msg>> {
        match target {
            CtxTarget::Tab(id) => {
                let mut items = vec![MenuItem::action("New group", Msg::NewGroup(*id))];
                let current = self.groups.of_tab(*id);
                for g in &self.groups.groups {
                    if current == Some(g.id.as_str()) {
                        continue;
                    }
                    items.push(MenuItem::action(
                        format!("Add to {}", g.name),
                        Msg::AddToGroup {
                            tab: *id,
                            group: g.id.clone(),
                        },
                    ));
                }
                if current.is_some() {
                    items.push(MenuItem::action("Ungroup", Msg::UngroupTab(*id)));
                }
                items
            }
            CtxTarget::Group(gid) => vec![
                MenuItem::action("Rename", Msg::RenameStart(gid.clone())),
                MenuItem::action("Ungroup", Msg::UngroupGroup(gid.clone())),
            ],
            CtxTarget::Page(ctx) => page_menu_items(ctx),
            CtxTarget::History { forward } => {
                let (entries, current) = self
                    .active_tab_info()
                    .map(|t| (t.history.as_slice(), t.history_index))
                    .unwrap_or((&[], 0));
                let items = page_menu::history_jump_items(entries, current, *forward, 12);
                if items.is_empty() {
                    vec![MenuItem::disabled(if *forward {
                        "No forward history"
                    } else {
                        "No back history"
                    })]
                } else {
                    items
                        .into_iter()
                        .map(|(index, label)| MenuItem::action(label, Msg::NavJump(index)))
                        .collect()
                }
            }
        }
    }

    /// Left vertical tab column. Profile switch lives in the full-width
    /// chrome bar; this is just the title stack. New tabs come from `⌘T`.
    pub fn view_tab_sidebar(&self) -> Element<'_, Msg> {
        let active_id = {
            let paint = self.slot.paint_tab.load(Ordering::Relaxed);
            if paint != u64::MAX {
                TabId(paint)
            } else {
                self.cached_active
            }
        };
        let mk_tab = |t: &TabInfo| {
            let label = crate::util::tab_strip_label(&t.title, &t.url, self.sidebar_w);
            SidebarItem::new(label, Msg::ActivateTab(t.id))
                .active(t.id == active_id)
                .on_close(Msg::CloseTab(t.id))
                .on_context(Msg::TabContext(t.id))
                .id(t.id.0.to_string())
        };
        let mut sections: Vec<SidebarSection<'_, Msg>> = Vec::new();
        for g in &self.groups.groups {
            let members: Vec<SidebarItem<'_, Msg>> = self
                .cached_tabs
                .iter()
                .filter(|t| self.groups.of_tab(t.id) == Some(g.id.as_str()))
                .map(mk_tab)
                .collect();
            let n = members.len();
            let header_active =
                g.collapsed && self.groups.of_tab(active_id).is_some_and(|id| id == g.id);
            let mut section = SidebarSection::new(g.name.clone(), members)
                .collapsible(g.collapsed, Msg::ToggleGroup(g.id.clone()))
                .header_active(header_active)
                .header_context(Msg::GroupContext(g.id.clone()))
                .header_count(n);
            if let Some((rid, draft)) = &self.renaming {
                if rid == &g.id {
                    let field = text_input("Group name", draft)
                        .on_input(Msg::RenameInput)
                        .on_submit(Msg::RenameCommit)
                        .style(sola_kit::components::text_input::style)
                        .padding(Padding::from([2, 4]));
                    section = section.header_content(field);
                }
            }
            sections.push(section);
        }
        let loose: Vec<SidebarItem<'_, Msg>> = self
            .cached_tabs
            .iter()
            .filter(|t| self.groups.of_tab(t.id).is_none())
            .map(mk_tab)
            .collect();
        if !loose.is_empty() || sections.is_empty() {
            sections.push(SidebarSection::unlabeled(loose));
        }
        SidebarPanel::new(sections)
            .density(SidebarDensity::Large)
            .item_hover(self.hovered_tab.clone(), Msg::TabHover)
            .resizable_with(
                self.sidebar_w,
                self.dragging_divider,
                Msg::DividerPress,
                DividerColors::raised_to_canvas(&self.theme),
            )
            .reorderable(ReorderCfg {
                on_press: Box::new(Msg::ReorderStart),
                active: if self.reorder_dragging {
                    self.reorder
                } else {
                    None
                },
                cursor_y: self.reorder_cursor_y,
                anim: self.reorder_dragging.then_some(&self.reorder_anim),
            })
            .build()
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
        let info = self.active_tab_info();
        let can_back = info.is_some_and(chrome_can_nav_back);
        let can_fwd = info.is_some_and(chrome_can_nav_forward);
        let muted = {
            let t = self.theme.extended_palette().secondary.base.text;
            iced::Color { a: 0.55, ..t }
        };
        let back = self.nav_hold_btn(nav_icon_back(), can_back, muted, HistoryDir::Back);
        let forward = self.nav_hold_btn(nav_icon_forward(), can_fwd, muted, HistoryDir::Forward);
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
            self.vault_tool_btn(
                handle,
                unlocked,
                self.vault_panel_open,
                muted,
                Msg::VaultToggle,
            )
        };
        #[cfg(feature = "bitwarden")]
        let cards_btn = self.vault_tool_btn(
            self.cards_icon.clone(),
            self.vault_status.unlocked,
            self.cards_panel_open,
            muted,
            Msg::CardsToggle,
        );
        #[cfg(feature = "bitwarden")]
        let totp_btn = self.vault_tool_btn(
            self.totp_icon.clone(),
            self.vault_status.unlocked,
            self.totp_panel_open,
            muted,
            Msg::TotpToggle,
        );
        #[cfg(not(feature = "bitwarden"))]
        let vault_btn = Space::new().width(Length::Fixed(0.0));
        #[cfg(not(feature = "bitwarden"))]
        let cards_btn = Space::new().width(Length::Fixed(0.0));
        #[cfg(not(feature = "bitwarden"))]
        let totp_btn = Space::new().width(Length::Fixed(0.0));

        let downloads_btn = self.downloads_tool_btn(muted);

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
            totp_btn,
            cards_btn,
            downloads_btn,
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

    fn downloads_tool_btn(&self, muted: iced::Color) -> Element<'_, Msg> {
        let accent = self.theme.extended_palette().primary.base.color;
        let active = self.downloads.has_in_progress() || self.downloads.has_unseen();
        let color = if self.downloads_panel_open || active {
            accent
        } else {
            muted
        };
        let icon = button(icon_svg_colored(self.download_icon.clone(), 18, color))
            .padding(PAD_CONTROL_SM)
            .width(Length::Fixed(NAV_BTN_W))
            .style(
                if self.downloads_panel_open || self.downloads.has_in_progress() {
                    vault_toolbar_btn_unlocked
                } else {
                    kit_toolbar::style
                },
            )
            .on_press(Msg::DownloadsToggle);
        match self.downloads.progress_frac() {
            Some(frac) => stack![icon, omnibox_progress_overlay(frac)].into(),
            None => icon.into(),
        }
    }

    /// Shared lock / card toolbar control.
    ///
    /// Locked: muted glyph (looks idle). Unlocked: full chrome foreground
    /// on *both* so the pair reads ready — accent is reserved for the
    /// panel that is actually open (not a second “I’m unlocked” blue).
    #[cfg(feature = "bitwarden")]
    fn vault_tool_btn(
        &self,
        handle: iced::widget::svg::Handle,
        unlocked: bool,
        open: bool,
        muted: iced::Color,
        msg: Msg,
    ) -> Element<'_, Msg> {
        let fg = self.theme.extended_palette().background.base.text;
        let accent = self.theme.extended_palette().primary.base.color;
        let color = if open {
            accent
        } else if unlocked {
            fg
        } else {
            muted
        };
        button(icon_svg_colored(handle, 18, color))
            .padding(PAD_CONTROL_SM)
            .width(Length::Fixed(NAV_BTN_W))
            .style(if open {
                vault_toolbar_btn_unlocked
            } else {
                kit_toolbar::style
            })
            .on_press(msg)
            .into()
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

    /// Back / forward: click = one step; hold = session history menu.
    ///
    /// iced `Button::on_press` fires on *release*, so hold must be a
    /// `mouse_area` around a non-button (a button with `on_press` would
    /// capture the down and look disabled without one).
    fn nav_hold_btn(
        &self,
        handle: iced::widget::svg::Handle,
        enabled: bool,
        muted: iced::Color,
        dir: HistoryDir,
    ) -> Element<'_, Msg> {
        let icon: Element<'_, Msg> = if enabled {
            icon_svg(handle, 16)
        } else {
            icon_svg_colored(handle, 16, muted)
        };
        let inner = container(icon)
            .padding(PAD_CONTROL_SM)
            .width(Length::Fixed(NAV_BTN_W))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center);
        if !enabled {
            return inner.into();
        }
        mouse_area(inner).on_press(Msg::NavHoldStart(dir)).into()
    }

    fn take_page_clipboard(&mut self) -> Task<Msg> {
        let Some(text) = self
            .engine
            .clipboard_handle()
            .lock()
            .unwrap()
            .take()
            .and_then(|t| crate::util::usable_clipboard_text(Some(t)))
        else {
            return Task::none();
        };
        tracing::debug!(len = text.len(), "page copy → system clipboard");
        iced::clipboard::write(text)
    }

    fn take_page_menu(&mut self) {
        let menus: Vec<PageContext> = self.page_menus.lock().unwrap().drain(..).collect();
        if let Some(ctx) = menus.into_iter().last() {
            self.context_menu = Some((last_cursor_point(), CtxTarget::Page(ctx)));
        }
    }

    fn nav_step(&mut self, dir: HistoryDir) -> Task<Msg> {
        let info = self.active_tab_info();
        let cef_can = info.is_some_and(|t| match dir {
            HistoryDir::Back => t.can_go_back,
            HistoryDir::Forward => t.can_go_forward,
        });
        if cef_can {
            self.set_active_loading(true);
            let _ = self.cmd_tx.send(Cmd::Nav(match dir {
                HistoryDir::Back => NavCmd::Back,
                HistoryDir::Forward => NavCmd::Forward,
            }));
            return Task::none();
        }
        let Some(info) = info else {
            return Task::none();
        };
        let items = page_menu::history_jump_items(
            &info.history,
            info.history_index,
            matches!(dir, HistoryDir::Forward),
            1,
        );
        let Some((index, _)) = items.into_iter().next() else {
            return Task::none();
        };
        let Some(url) = info
            .history
            .iter()
            .find(|e| e.index == index)
            .map(|e| e.url.clone())
        else {
            return Task::none();
        };
        self.set_active_loading(true);
        let _ = self.cmd_tx.send(Cmd::Nav(NavCmd::LoadUrl(url)));
        Task::none()
    }

    fn finish_nav_hold(&mut self) -> Task<Msg> {
        let Some(hold) = self.nav_hold.take() else {
            return Task::none();
        };
        if hold.menu {
            return Task::none();
        }
        match hold.dir {
            HistoryDir::Back => self.update(Msg::NavBack),
            HistoryDir::Forward => self.update(Msg::NavForward),
        }
    }

    fn open_history_menu(&mut self) {
        let Some(hold) = self.nav_hold.as_ref() else {
            return;
        };
        if hold.menu {
            return;
        }
        let forward = matches!(hold.dir, HistoryDir::Forward);
        let has = self
            .active_tab_info()
            .map(|t| {
                !page_menu::history_jump_items(&t.history, t.history_index, forward, 12).is_empty()
            })
            .unwrap_or(false);
        if !has {
            return;
        }
        if let Some(hold) = self.nav_hold.as_mut() {
            hold.menu = true;
        }
        self.context_menu = Some((last_cursor_point(), CtxTarget::History { forward }));
    }

    fn run_page_menu(&mut self, action: PageMenuAction) -> Task<Msg> {
        match action {
            PageMenuAction::OpenLink(url) => {
                if !url.is_empty() {
                    self.open_tab(url, false);
                }
                Task::none()
            }
            PageMenuAction::CopyLink(url) | PageMenuAction::Copy(url) => {
                match crate::util::usable_clipboard_text(Some(url)) {
                    Some(t) => iced::clipboard::write(t),
                    None => Task::none(),
                }
            }
            PageMenuAction::Cut => {
                let _ = self
                    .cmd_tx
                    .send(Cmd::EvaluateJs(crate::paste_js::copy_selection_script()));
                let _ = self.cmd_tx.send(Cmd::Edit(EditCmd::Cut));
                Task::none()
            }
            PageMenuAction::Paste => iced::clipboard::read().map(Msg::PagePasted),
            PageMenuAction::Back => self.update(Msg::NavBack),
            PageMenuAction::Forward => self.update(Msg::NavForward),
            PageMenuAction::Reload => self.update(Msg::NavReloadOrStop),
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
                // Passkey ceremony mid-unlock → stay on picker. Cards button
                // unlocks then opens the cards panel. Otherwise fill-login.
                if self.pending_passkey.as_ref().is_some_and(|p| p.is_create()) {
                    self.vault_phase = VaultPanelPhase::PasskeyCreate;
                    self.set_vault_panel_open(true);
                    self.request_passkey_create_matches();
                } else if self.pending_passkey.is_some() {
                    self.vault_phase = VaultPanelPhase::PasskeyPick;
                    self.set_vault_panel_open(true);
                    self.request_passkey_candidates();
                } else if self.vault_resume == VaultResume::Cards {
                    self.vault_phase = VaultPanelPhase::Credentials;
                    self.set_cards_panel_open(true);
                    self.request_vault_cards();
                } else if self.vault_resume == VaultResume::Totp {
                    self.vault_phase = VaultPanelPhase::Credentials;
                    self.totp_autofill_pending = true;
                    self.set_totp_panel_open(true);
                    self.request_vault_totp();
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
                if self.cards_panel_open && self.vault_status.unlocked {
                    self.request_vault_cards();
                }
                if self.totp_panel_open && self.vault_status.unlocked {
                    self.request_vault_totp();
                }
            }
            VaultEvent::SyncFailed { message } => {
                tracing::warn!(%message, "vault: sync failed");
                if (self.vault_panel_open || self.cards_panel_open || self.totp_panel_open)
                    && self.vault_status.unlocked
                {
                    self.vault_error = Some(format!("Signed in, but sync failed: {message}"));
                }
            }
            VaultEvent::Matches(list) => {
                self.vault_matches_loading = false;
                tracing::info!(n = list.len(), url = %self.vault_matches_url, "vault: matches");
                self.vault_matches = list;
            }
            VaultEvent::Totp(list) => {
                self.vault_totp_loading = false;
                tracing::info!(n = list.len(), "vault: totp");
                self.vault_totp = list;
                if self.totp_autofill_pending {
                    self.totp_autofill_pending = false;
                    if let Some(first) = self.vault_totp.first() {
                        if !self.vault_busy {
                            self.vault_busy = true;
                            self.vault.send(VaultCmd::FillTotp {
                                id: first.id.clone(),
                            });
                        }
                    }
                }
            }
            VaultEvent::TotpFillReady { code } => {
                self.vault_busy = false;
                let script = fill_totp_script(&code);
                let _ = self.cmd_tx.send(Cmd::EvaluateJs(script));
                if self.totp_copy_next {
                    self.totp_copy_next = false;
                    self.pending_totp_clipboard = Some(code);
                    tracing::info!("vault: totp copied to clipboard");
                } else {
                    tracing::info!("vault: totp fill injected");
                }
                // Keep the panel open so the user can see the timeout / pick another.
            }
            VaultEvent::Cards(list) => {
                self.vault_cards_loading = false;
                tracing::info!(n = list.len(), "vault: cards");
                self.vault_cards = list;
            }
            VaultEvent::FillReady {
                mut username,
                mut password,
            } => {
                self.vault_busy = false;
                let script = fill_credentials_script(username.as_deref(), password.as_deref());
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
            VaultEvent::CardFillReady {
                cardholder_name,
                mut number,
                exp_month,
                exp_year,
                mut code,
                brand,
            } => {
                self.vault_busy = false;
                let script = fill_card_script(
                    cardholder_name.as_deref(),
                    number.as_deref(),
                    exp_month.as_deref(),
                    exp_year.as_deref(),
                    code.as_deref(),
                    brand.as_deref(),
                );
                if let Some(ref mut n) = number {
                    n.zeroize();
                }
                if let Some(ref mut c) = code {
                    c.zeroize();
                }
                let _ = self.cmd_tx.send(Cmd::EvaluateJs(script));
                tracing::info!("vault: card fill injected into active page");
                self.set_cards_panel_open(false);
            }
            VaultEvent::Created {
                id: _,
                mut username,
                mut password,
            } => {
                crate::vault::passkey_bridge::drain_fill_results();
                let script =
                    fill_credentials_script_ex(username.as_deref(), password.as_deref(), true);
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
                // Clear pending only if this is the current ceremony
                // (primary id or a coalesced extra).
                let ids = self
                    .pending_passkey
                    .as_ref()
                    .filter(|p| p.all_ids().contains(&req_id))
                    .map(|p| p.all_ids());
                let keep_create = !ok
                    && self
                        .pending_passkey
                        .as_ref()
                        .is_some_and(|p| p.is_create() && p.all_ids().contains(&req_id));
                if keep_create {
                    if let Some(pending) = self.pending_passkey.as_mut() {
                        pending.error = Some(payload.clone());
                    }
                    if self.vault_panel_open {
                        self.vault_error = Some(payload.clone());
                    }
                    tracing::warn!(req_id, error = %payload, "vault: passkey create failed — panel stays open");
                } else if let Some(ids) = ids {
                    self.pending_passkey = None;
                    if matches!(
                        self.vault_phase,
                        VaultPanelPhase::PasskeyPick | VaultPanelPhase::PasskeyCreate
                    ) {
                        self.vault_phase = VaultPanelPhase::Credentials;
                        self.set_vault_panel_open(false);
                    }
                    let script = crate::vault::resolve_webauthn_scripts(&ids, ok, &payload);
                    let _ = self.cmd_tx.send(Cmd::EvaluateJs(script));
                    if ok {
                        tracing::info!(req_id, "vault: passkey response injected");
                    } else {
                        tracing::warn!(req_id, error = %payload, "vault: passkey response error injected");
                    }
                } else {
                    let script = crate::vault::resolve_webauthn_script(req_id, ok, &payload);
                    let _ = self.cmd_tx.send(Cmd::EvaluateJs(script));
                    if !ok && self.vault_panel_open {
                        self.vault_error = Some(payload);
                    }
                }
            }
            VaultEvent::Error { message } => {
                tracing::warn!(%message, "vault: error");
                self.vault_busy = false;
                self.vault_matches_loading = false;
                self.vault_cards_loading = false;
                self.vault_totp_loading = false;
                if self.vault_panel_open || self.cards_panel_open || self.totp_panel_open {
                    self.vault_error = Some(message);
                }
            }
        }
    }

    /// Page asked for a passkey: open the vault panel picker (or unlock first).
    #[cfg(feature = "bitwarden")]
    fn dispatch_passkey_request(&mut self, req: PasskeyPageRequest) {
        // Same click can arrive more than once (console + leftover
        // beacon, helper IPC fan-out). Same origin/RP also retries
        // while the picker is open (Gemini Exchange). Rejecting those
        // as "Superseded" makes the page show failure before a pick.
        if let Some(cur) = self.pending_passkey.as_mut() {
            let same_site = cur.req.origin == req.origin
                && cur.req.action == req.action
                && (cur.req.rp_id == req.rp_id || cur.req.rp_id.is_empty() || req.rp_id.is_empty());
            if same_site {
                if cur.req.id != req.id && !cur.extra_ids.contains(&req.id) {
                    tracing::info!(
                        keep_id = cur.req.id,
                        extra_id = req.id,
                        origin = %req.origin,
                        "vault: coalescing passkey request (same site)"
                    );
                    cur.extra_ids.push(req.id);
                }
                return;
            }
        }
        // Different site: replace any prior pending request.
        if let Some(old) = self.pending_passkey.take() {
            let script = crate::vault::resolve_webauthn_scripts(
                &old.all_ids(),
                false,
                "Superseded by another passkey request.",
            );
            let _ = self.cmd_tx.send(Cmd::EvaluateJs(script));
        }

        let create = req.is_create();
        tracing::info!(
            req_id = req.id,
            origin = %req.origin,
            rp_id = %req.rp_id,
            action = %req.action,
            "vault: page requested passkey — opening panel"
        );

        self.pending_passkey = Some(PendingPasskey {
            req,
            extra_ids: Vec::new(),
            candidates: Vec::new(),
            loading: create,
            error: None,
        });
        self.vault_error = None;

        if !self.vault_status.unlocked {
            self.vault_phase = VaultPanelPhase::Credentials;
            self.set_vault_panel_open(true);
            // User unlocks; LoginOk continues into candidates / create confirm.
            return;
        }

        if create {
            self.vault_phase = VaultPanelPhase::PasskeyCreate;
            self.set_vault_panel_open(true);
            self.request_passkey_create_matches();
        } else {
            self.vault_phase = VaultPanelPhase::PasskeyPick;
            self.set_vault_panel_open(true);
            self.request_passkey_candidates();
        }
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
    fn request_passkey_create_matches(&mut self) {
        let Some(pending) = self.pending_passkey.as_ref() else {
            return;
        };
        let url = pending.req.origin.clone();
        if url.is_empty() {
            return;
        }
        self.vault_matches_url = url.clone();
        self.vault_matches_loading = true;
        self.vault.send(VaultCmd::Matches { url });
    }

    #[cfg(feature = "bitwarden")]
    fn confirm_passkey_create(&mut self, cipher_id: Option<String>) {
        let Some(pending) = self.pending_passkey.clone() else {
            return;
        };
        if !pending.is_create() {
            return;
        }
        if self.vault_busy {
            return;
        }
        self.vault_busy = true;
        self.vault_error = None;
        if let Some(p) = self.pending_passkey.as_mut() {
            p.error = None;
        }
        self.vault.send(VaultCmd::PasskeyRegister {
            req_id: pending.req.id,
            origin: pending.req.origin,
            public_key_json: pending.req.public_key_json,
            cipher_id,
        });
    }

    #[cfg(feature = "bitwarden")]
    fn cancel_pending_passkey(&mut self, reason: &str) {
        if let Some(pending) = self.pending_passkey.take() {
            let script = crate::vault::resolve_webauthn_scripts(&pending.all_ids(), false, reason);
            let _ = self.cmd_tx.send(Cmd::EvaluateJs(script));
        }
        if matches!(
            self.vault_phase,
            VaultPanelPhase::PasskeyPick | VaultPanelPhase::PasskeyCreate
        ) {
            self.vault_phase = VaultPanelPhase::Credentials;
            self.set_vault_panel_open(false);
        }
        self.vault_busy = false;
    }

    /// Centered modal for Profiles menubar manage actions.
    fn view_profile_dialog(&self) -> Element<'_, Msg> {
        use sola_kit::components::style::{SPACE_MD, SPACE_SM};

        let Some(kind) = self.profile_dialog.as_ref() else {
            return Space::new()
                .width(Length::Shrink)
                .height(Length::Shrink)
                .into();
        };

        let title = match kind {
            ProfileDialog::New => "New Profile",
            ProfileDialog::Rename => "Rename Profile",
            ProfileDialog::DeleteConfirm => "Delete Profile",
        };
        let title_el = text(title).size(15).font(sola_kit::fonts::ui_medium());

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
                    col = col.push(text(err.clone()).size(12).style(|theme: &iced::Theme| {
                        iced::widget::text::Style {
                            color: Some(theme.extended_palette().danger.base.color),
                        }
                    }));
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
                    col = col.push(text(err.clone()).size(12).style(|theme: &iced::Theme| {
                        iced::widget::text::Style {
                            color: Some(theme.extended_palette().danger.base.color),
                        }
                    }));
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

        let panel =
            card::modal(container(body).padding(SPACE_MD + SPACE_SM)).width(Length::Fixed(340.0));

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

    /// Authenticator panel — current TOTP + countdown; click fills the page.
    #[cfg(feature = "bitwarden")]
    fn view_totp_panel(&self) -> Element<'_, Msg> {
        use sola_kit::components::style::{SPACE_MD, SPACE_SM};

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

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

        let title = text("Authenticator")
            .size(15)
            .font(sola_kit::fonts::ui_medium());
        const MATCH_LIST_H: f32 = 420.0;
        let page_url = self
            .active_tab_info()
            .map(|t| t.url.as_str())
            .unwrap_or("");
        let host_hint = page_host_hint(page_url);
        let mut col = column![
            title,
            soft("Last-used fills the page. Click a code to copy it.".into()),
        ]
        .spacing(SPACE_SM)
        .width(Length::Fixed(340.0));

        if !host_hint.is_empty() {
            col = col.push(soft(host_hint));
        }

        if let Some(err) = self.vault_error.as_ref() {
            col = col.push(text(err.clone()).size(12).style(|theme: &iced::Theme| {
                iced::widget::text::Style {
                    color: Some(theme.extended_palette().danger.base.color),
                }
            }));
        }

        if self.vault_totp_loading && self.vault_totp.is_empty() {
            col = col.push(text("Looking up authenticators…").size(13));
        } else if page_url.is_empty() || page_url == BLANK_URL {
            col = col.push(text("Open a website to fill a code.").size(13));
        } else if self.vault_totp.is_empty() {
            col = col.push(text("No authenticator codes for this site.").size(13));
            col = col.push(soft_sm(
                "Add a TOTP secret on a login for this site in Bitwarden, then Refresh.".into(),
            ));
        } else {
            let mut list = column![].spacing(4.0);
            for t in &self.vault_totp {
                let title_line = if t.name.is_empty() {
                    t.username
                        .clone()
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "Authenticator".into())
                } else {
                    t.name.clone()
                };
                let remaining = totp_remaining_secs(t.period, now);
                let pretty = pretty_totp_code(&t.code);
                let sub = if let Some(u) = t.username.as_deref().filter(|s| !s.is_empty()) {
                    format!("{u} · {remaining}s")
                } else {
                    format!("{remaining}s")
                };
                let code_line = text(pretty)
                    .size(18)
                    .font(sola_kit::fonts::mono());
                let row_body = column![
                    text(title_line).size(13).font(sola_kit::fonts::ui_medium()),
                    code_line,
                    soft_sm(sub),
                ]
                .spacing(2);
                let id = t.id.clone();
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
                    btn = btn.on_press(Msg::TotpFill(id));
                }
                list = list.push(btn);
            }
            col = col.push(
                scrollable(list)
                    .height(Length::Fixed(MATCH_LIST_H))
                    .width(Length::Fill),
            );
        }

        let mut refresh = kit_button::labeled_sm("Refresh", kit_button::ghost);
        if !self.vault_busy && !self.vault_totp_loading {
            refresh = refresh.on_press(Msg::TotpRefresh);
        }
        let close =
            kit_button::labeled("Close", kit_button::secondary).on_press(Msg::VaultPanelClose);
        col = col.push(
            row![refresh, close]
                .spacing(SPACE_SM)
                .align_y(Alignment::Center),
        );

        let panel =
            card::modal(container(col).padding(SPACE_MD + SPACE_SM)).width(Length::Fixed(360.0));
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

    /// Cards panel — list every Bitwarden card; click fills the page.
    #[cfg(feature = "bitwarden")]
    fn view_cards_panel(&self) -> Element<'_, Msg> {
        use sola_kit::components::style::{SPACE_MD, SPACE_SM};

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

        let title = text("Fill card")
            .size(15)
            .font(sola_kit::fonts::ui_medium());
        const MATCH_LIST_H: f32 = 420.0;
        let mut col = column![title, soft("Cards from your Bitwarden vault.".into())]
            .spacing(SPACE_SM)
            .width(Length::Fixed(340.0));

        if let Some(err) = self.vault_error.as_ref() {
            col = col.push(text(err.clone()).size(12).style(|theme: &iced::Theme| {
                iced::widget::text::Style {
                    color: Some(theme.extended_palette().danger.base.color),
                }
            }));
        }

        if self.vault_cards_loading {
            col = col.push(text("Looking up cards…").size(13));
        } else if self.vault_cards.is_empty() {
            col = col.push(text("No cards saved in Bitwarden.").size(13));
            col = col.push(soft_sm("Add a card in Bitwarden, then Refresh.".into()));
        } else {
            let mut list = column![].spacing(4.0);
            for c in &self.vault_cards {
                let title_line = if c.name.is_empty() {
                    c.brand.clone().unwrap_or_else(|| "Card".into())
                } else {
                    c.name.clone()
                };
                let sub = {
                    let mut s = c.subtitle();
                    if let Some(ref exp) = c.exp {
                        if s.is_empty() {
                            s = exp.clone();
                        } else {
                            s.push_str(" · ");
                            s.push_str(exp);
                        }
                    }
                    if s.is_empty() {
                        s = "—".into();
                    }
                    s
                };
                let row_body = column![
                    text(title_line).size(13).font(sola_kit::fonts::ui_medium()),
                    soft_sm(sub),
                ]
                .spacing(2);
                let id = c.id.clone();
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
                    btn = btn.on_press(Msg::CardsFill(id));
                }
                list = list.push(btn);
            }
            col = col.push(
                scrollable(list)
                    .height(Length::Fixed(MATCH_LIST_H))
                    .width(Length::Fill),
            );
        }

        let mut refresh = kit_button::labeled_sm("Refresh", kit_button::ghost);
        if !self.vault_busy && !self.vault_cards_loading {
            refresh = refresh.on_press(Msg::CardsRefresh);
        }
        let close =
            kit_button::labeled("Close", kit_button::secondary).on_press(Msg::VaultPanelClose);
        col = col.push(
            row![refresh, close]
                .spacing(SPACE_SM)
                .align_y(Alignment::Center),
        );

        let panel =
            card::modal(container(col).padding(SPACE_MD + SPACE_SM)).width(Length::Fixed(360.0));
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

        let passkey_create = matches!(self.vault_phase, VaultPanelPhase::PasskeyCreate)
            || self
                .pending_passkey
                .as_ref()
                .is_some_and(|p| p.is_create() && self.vault_status.unlocked);
        let passkey_pick = matches!(self.vault_phase, VaultPanelPhase::PasskeyPick)
            || self
                .pending_passkey
                .as_ref()
                .is_some_and(|p| !p.is_create() && self.vault_status.unlocked);

        let body: Element<'_, Msg> = if passkey_create {
            let title = text("Save a passkey")
                .size(15)
                .font(sola_kit::fonts::ui_medium());
            let pending = self.pending_passkey.as_ref();
            let host = pending
                .map(|p| {
                    page_host_hint(&p.req.origin)
                        .strip_prefix("For ")
                        .unwrap_or("this site")
                        .to_string()
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "this site".into());
            let account = pending
                .map(|p| create_account_hint(&p.req.public_key_json))
                .unwrap_or(None);

            let mut col = column![title, soft(format!("For {host}"))]
                .spacing(SPACE_SM)
                .width(Length::Fixed(340.0));
            if let Some(account) = account {
                col = col.push(soft_sm(account));
            }
            if let Some(err) = err_line {
                col = col.push(err);
            }
            if let Some(err) = pending.and_then(|p| p.error.as_ref()) {
                col = col.push(text(err.clone()).size(12).style(|theme: &iced::Theme| {
                    iced::widget::text::Style {
                        color: Some(theme.extended_palette().danger.base.color),
                    }
                }));
            }

            if self.vault_matches_loading {
                col = col.push(text("Looking up logins…").size(13));
            } else if !self.vault_matches.is_empty() {
                col = col.push(soft_sm("Add to an existing login".into()));
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
                        btn = btn.on_press(Msg::VaultPasskeyCreateOn(id));
                    }
                    list = list.push(btn);
                }
                col = col.push(
                    scrollable(list)
                        .height(Length::Fixed(220.0))
                        .width(Length::Fill),
                );
            }

            let save_label = if self.vault_busy {
                "Saving…"
            } else if self.vault_matches.is_empty() {
                "Save passkey"
            } else {
                "Save as new login"
            };
            let mut save = kit_button::labeled(save_label, kit_button::primary);
            if !self.vault_busy {
                save = save.on_press(Msg::VaultPasskeyCreateNew);
            }
            let cancel = kit_button::labeled(
                if self.vault_busy {
                    "Saving…"
                } else {
                    "Cancel"
                },
                kit_button::ghost,
            )
            .on_press(Msg::VaultPasskeyCancel);
            col = col.push(
                row![save, cancel]
                    .spacing(SPACE_SM)
                    .align_y(Alignment::Center),
            );
            col.into()
        } else if passkey_pick {
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
                    col = col.push(text(err.clone()).size(12).style(|theme: &iced::Theme| {
                        iced::widget::text::Style {
                            color: Some(theme.extended_palette().danger.base.color),
                        }
                    }));
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
                if self.vault_busy {
                    "Signing…"
                } else {
                    "Cancel"
                },
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
            let password_row =
                column![field("Password", password, None, None), regen,].spacing(4.0);
            let mut create = kit_button::labeled(
                if busy { "Creating…" } else { "Create" },
                kit_button::primary,
            );
            if !busy {
                create = create.on_press(Msg::VaultCreateSubmit);
            }
            let cancel =
                kit_button::labeled("Cancel", kit_button::ghost).on_press(Msg::VaultCreateCancel);
            let mut col = column![title].spacing(SPACE_SM).width(Length::Fixed(340.0));
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
                self.active_tab_info().map(|t| t.url.as_str()).unwrap_or("")
            } else {
                self.vault_matches_url.as_str()
            };
            let host_hint = page_host_hint(page_url);

            // Wide enough for emails; tall enough that ~10–12 logins rarely scroll.
            const MATCH_LIST_H: f32 = 420.0;
            let mut col = column![title].spacing(SPACE_SM).width(Length::Fixed(340.0));

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
                            text(title_line).size(13).font(sola_kit::fonts::ui_medium()),
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
            let close =
                kit_button::labeled("Close", kit_button::secondary).on_press(Msg::VaultPanelClose);
            col = col.push(
                row![create, refresh, close]
                    .spacing(SPACE_SM)
                    .align_y(Alignment::Center),
            );
            col.into()
        } else {
            match &self.vault_phase {
                VaultPanelPhase::PasskeyPick | VaultPanelPhase::PasskeyCreate => {
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

                    let mut col = column![title, soft("Bitwarden".into()),]
                        .spacing(SPACE_SM)
                        .width(Length::Fixed(300.0));
                    if let Some(pending) = self.pending_passkey.as_ref() {
                        let copy = if pending.is_create() {
                            "A site wants to save a passkey — unlock to continue."
                        } else {
                            "A site asked for a passkey — unlock to choose one."
                        };
                        col = col.push(soft(copy.into()));
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
                    let title = text("Verify").size(15).font(sola_kit::fonts::ui_medium());
                    let hint = email_hint
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("your email");
                    let (subtitle, placeholder, show_resend) = match kind {
                        // New-device protection emails a code automatically on the
                        // password grant; complete with form field `newDeviceOtp`.
                        TwoFactorKind::NewDevice => (
                            format!("Enter the code Bitwarden emailed to {hint}."),
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

                    let mut col =
                        column![title, soft(subtitle), Space::new().height(SPACE_SM), otp,]
                            .spacing(SPACE_SM)
                            .width(Length::Fixed(300.0));

                    if let Some(err) = err_line {
                        col = col.push(err);
                    }

                    let mut actions = row![verify_btn].spacing(SPACE_SM);
                    if show_resend {
                        let mut resend = kit_button::labeled("Resend", kit_button::ghost);
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
        let panel =
            card::modal(container(body).padding(SPACE_MD + SPACE_SM)).width(Length::Fixed(360.0));

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

    fn view_downloads_panel(&self) -> Element<'_, Msg> {
        use crate::downloads::{DownloadStatus, ellipsize_middle, format_bytes, format_progress};
        use iced::widget::text::Wrapping;
        use sola_kit::components::style::SPACE_LG;

        const PANEL_W: f32 = 300.0;
        const NAME_CHARS: usize = 26;

        let caption = |s: String, danger: bool| {
            text(s).size(11).style(move |theme: &iced::Theme| {
                let p = theme.extended_palette();
                let color = if danger {
                    p.danger.base.color
                } else {
                    iced::Color {
                        a: 0.58,
                        ..p.background.base.text
                    }
                };
                iced::widget::text::Style { color: Some(color) }
            })
        };

        let title = text("Downloads")
            .size(13)
            .font(sola_kit::fonts::ui_medium())
            .style(|theme: &iced::Theme| {
                let t = theme.extended_palette().background.base.text;
                iced::widget::text::Style {
                    color: Some(iced::Color { a: 0.82, ..t }),
                }
            });

        let items = self.downloads.items();
        let body: Element<'_, Msg> = if items.is_empty() {
            column![title, caption("Nothing downloaded yet.".into(), false)]
                .spacing(SPACE_LG)
                .width(Length::Fill)
                .into()
        } else {
            let mut list = column![].spacing(0.0);
            for (i, e) in items.iter().enumerate() {
                if i > 0 {
                    list = list.push(horizontal_divider());
                }
                let name = ellipsize_middle(&e.filename, NAME_CHARS);
                let name_el = text(name)
                    .size(13)
                    .font(sola_kit::fonts::ui())
                    .wrapping(Wrapping::None);
                let name_el = container(name_el).width(Length::Fill).clip(true);

                let meta = match e.status {
                    DownloadStatus::InProgress => format_progress(e),
                    DownloadStatus::Complete => e
                        .total
                        .or(Some(e.received))
                        .filter(|n| *n > 0)
                        .map(format_bytes)
                        .unwrap_or_default(),
                    DownloadStatus::Failed => "Failed".into(),
                };
                let failed = e.status == DownloadStatus::Failed;
                let meta_el = caption(meta, failed);

                let dismiss = match e.status {
                    DownloadStatus::InProgress => Msg::DownloadCancel(e.id.clone()),
                    _ => Msg::DownloadRemove(e.id.clone()),
                };
                let x = button(icon_svg(nav_icon_stop(), 12))
                    .padding(4)
                    .width(Length::Fixed(22.0))
                    .style(kit_toolbar::style)
                    .on_press(dismiss);

                let main = row![name_el, meta_el]
                    .spacing(SPACE_LG)
                    .align_y(Alignment::Center)
                    .width(Length::Fill);

                let mut face: Element<'_, Msg> = match e.status {
                    DownloadStatus::Complete => button(main)
                        .padding(Padding::from([7, 4]))
                        .width(Length::Fill)
                        .style(download_row_style)
                        .on_press(Msg::DownloadOpen(e.id.clone()))
                        .into(),
                    DownloadStatus::InProgress => {
                        let frac = e.percent.unwrap_or(0.08).clamp(0.08, 1.0);
                        column![main, download_row_progress(frac)]
                            .spacing(5)
                            .width(Length::Fill)
                            .padding(Padding::from([7, 4]))
                            .into()
                    }
                    DownloadStatus::Failed => container(main)
                        .padding(Padding::from([7, 4]))
                        .width(Length::Fill)
                        .into(),
                };

                if e.status == DownloadStatus::InProgress {
                    // Keep cancel off the progress column so the x stays top-aligned.
                    face = container(face).width(Length::Fill).into();
                }

                list = list.push(
                    row![face, x]
                        .spacing(2)
                        .align_y(Alignment::Center)
                        .width(Length::Fill),
                );
            }

            let list: Element<'_, Msg> = if items.len() > 7 {
                scrollable(list)
                    .height(Length::Fixed(7.0 * 38.0))
                    .width(Length::Fill)
                    .into()
            } else {
                list.into()
            };

            column![title, list]
                .spacing(SPACE_LG)
                .width(Length::Fill)
                .into()
        };

        let panel = card::modal(container(body).padding(Padding {
            top: 12.0,
            right: 10.0,
            bottom: 8.0,
            left: 12.0,
        }))
        .width(Length::Fixed(PANEL_W));

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
        .on_press(Msg::DownloadsPanelClose);

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
        let mut subs = vec![
            crate::run::frame_subscription::<E>(frames, slot, active),
            iced::time::every(Duration::from_millis(250)).map(|_| Msg::Tick),
            sola_kit::app::bus_subscription().map(Msg::Bus),
            // Always registered; `update` no-ops when not dragging.
            iced::time::every(Duration::from_millis(16)).map(|_| Msg::ReorderTick),
            event::listen_with(|event, status, _| {
                match &event {
                    Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                        crate::input::store_modifiers(*m);
                    }
                    Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                        crate::input::store_modifiers(*modifiers);
                        if crate::input::is_super_key(key) {
                            crate::input::note_super_key(true);
                        }
                    }
                    Event::Keyboard(keyboard::Event::KeyReleased { key, modifiers, .. }) => {
                        crate::input::store_modifiers(*modifiers);
                        if crate::input::is_super_key(key) {
                            crate::input::note_super_key(false);
                        }
                    }
                    _ => {}
                }
                match event {
                    Event::Mouse(mouse::Event::CursorMoved { position }) => {
                        CURSOR_X_BITS.store(position.x.to_bits(), Ordering::Relaxed);
                        CURSOR_Y_BITS.store(position.y.to_bits(), Ordering::Relaxed);
                        // Rebuilding chrome on every pixel move starved menus and
                        // typing. Divider resize and tab reorder both need samples.
                        if DIVIDER_DRAGGING.load(Ordering::Relaxed)
                            || REORDER_TRACKING.load(Ordering::Relaxed)
                        {
                            Some(Msg::CursorMoved(position.x, position.y))
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
                }
            }),
        ];
        if self
            .nav_hold
            .as_ref()
            .is_some_and(|h| !h.menu && h.started.elapsed().as_millis() < NAV_HOLD_MS + 200)
        {
            subs.push(iced::time::every(Duration::from_millis(50)).map(|_| Msg::NavHoldFire));
        }
        Subscription::batch(subs)
    }
}

/// Divider drag: `listen_with` is a fn pointer and cannot close over App.
static DIVIDER_DRAGGING: AtomicBool = AtomicBool::new(false);
/// Tab-reorder press is live (before and after the movement threshold).
static REORDER_TRACKING: AtomicBool = AtomicBool::new(false);
/// Last pointer x (bits) so DividerPress has a current anchor without
/// emitting CursorMoved on every pixel.
static CURSOR_X_BITS: AtomicU32 = AtomicU32::new(0);
static CURSOR_Y_BITS: AtomicU32 = AtomicU32::new(0);

fn chrome_can_nav_back(t: &TabInfo) -> bool {
    t.can_go_back
        || !page_menu::history_jump_items(&t.history, t.history_index, false, 1).is_empty()
}

fn chrome_can_nav_forward(t: &TabInfo) -> bool {
    t.can_go_forward
        || !page_menu::history_jump_items(&t.history, t.history_index, true, 1).is_empty()
}

fn page_menu_items(ctx: &PageContext) -> Vec<MenuItem<Msg>> {
    page_menu::page_menu_kinds(ctx)
        .into_iter()
        .map(|kind| match kind {
            PageMenuKind::OpenLink => {
                let url = ctx.link_url.clone().unwrap_or_default();
                MenuItem::action(
                    "Open Link in New Tab",
                    Msg::PageMenu(PageMenuAction::OpenLink(url)),
                )
            }
            PageMenuKind::CopyLink => {
                let url = ctx.link_url.clone().unwrap_or_default();
                MenuItem::action(
                    "Copy Link Address",
                    Msg::PageMenu(PageMenuAction::CopyLink(url)),
                )
            }
            PageMenuKind::Copy => {
                let t = ctx.selection.clone().unwrap_or_default();
                MenuItem::action("Copy", Msg::PageMenu(PageMenuAction::Copy(t)))
            }
            PageMenuKind::Cut => MenuItem::action("Cut", Msg::PageMenu(PageMenuAction::Cut)),
            PageMenuKind::Paste => MenuItem::action("Paste", Msg::PageMenu(PageMenuAction::Paste)),
            PageMenuKind::Back => {
                if ctx.can_go_back {
                    MenuItem::action("Back", Msg::PageMenu(PageMenuAction::Back))
                } else {
                    MenuItem::disabled("Back")
                }
            }
            PageMenuKind::Forward => {
                if ctx.can_go_forward {
                    MenuItem::action("Forward", Msg::PageMenu(PageMenuAction::Forward))
                } else {
                    MenuItem::disabled("Forward")
                }
            }
            PageMenuKind::Reload => {
                MenuItem::action("Reload", Msg::PageMenu(PageMenuAction::Reload))
            }
            PageMenuKind::Separator => MenuItem::separator(),
        })
        .collect()
}

fn last_cursor_point() -> iced::Point {
    iced::Point::new(
        f32::from_bits(CURSOR_X_BITS.load(Ordering::Relaxed)),
        f32::from_bits(CURSOR_Y_BITS.load(Ordering::Relaxed)),
    )
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

fn download_row_style(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let p = theme.extended_palette();
    let bg = match status {
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
            Some(iced::Background::Color(p.background.strong.color))
        }
        _ => None,
    };
    iced::widget::button::Style {
        background: bg,
        text_color: p.background.base.text,
        border: iced::Border {
            color: iced::Color::TRANSPARENT,
            width: 0.0,
            radius: 5.0.into(),
        },
        ..Default::default()
    }
}

fn download_row_progress<'a>(frac: f32) -> Element<'a, Msg> {
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
                    radius: 1.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        });
    let track = container(Space::new().width(Length::Fill).height(Length::Fixed(2.0)))
        .width(Length::FillPortion(rest_w))
        .height(Length::Fixed(2.0))
        .style(|theme: &iced::Theme| {
            let t = theme.extended_palette().background.strong.color;
            iced::widget::container::Style {
                background: Some(iced::Background::Color(t)),
                border: iced::Border {
                    radius: 1.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        });
    row![fill, track]
        .width(Length::Fill)
        .height(Length::Fixed(2.0))
        .into()
}

/// Unlocked vault toolbar control — subtle accent wash so “ready” ≠ locked.
fn vault_toolbar_btn_unlocked(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    use iced::{Background, Border, Color};
    use sola_kit::components::style::RADIUS_SM;

    let p = theme.extended_palette();
    let accent = p.primary.base.color;
    let bg = match status {
        iced::widget::button::Status::Hovered => Color { a: 0.22, ..accent },
        iced::widget::button::Status::Pressed => Color { a: 0.30, ..accent },
        _ => Color { a: 0.14, ..accent },
    };
    iced::widget::button::Style {
        background: Some(Background::Color(bg)),
        text_color: accent,
        border: Border {
            color: Color { a: 0.35, ..accent },
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

#[cfg(feature = "bitwarden")]
fn pretty_totp_code(code: &str) -> String {
    if code.len() == 6 {
        format!("{} {}", &code[..3], &code[3..])
    } else if code.len() == 8 {
        format!("{} {}", &code[..4], &code[4..])
    } else {
        code.to_string()
    }
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
fn merge_tab_snapshot(prev: &[TabInfo], live: &[TabInfo], closed: &HashSet<TabId>) -> Vec<TabInfo> {
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
    let is_loading = live.is_loading || (prior.is_loading && is_transient_nav_url(&live.url));
    let load_progress = if is_loading {
        live.load_progress.max(prior.load_progress)
    } else {
        0.0
    };
    let (history, history_index) = page_menu::merge_tab_history(
        &prior.history,
        prior.history_index,
        &live.history,
        &url,
        &title,
    );
    TabInfo {
        id: prior.id,
        url,
        title,
        is_loading,
        can_go_back: live.can_go_back,
        can_go_forward: live.can_go_forward,
        load_progress,
        history,
        history_index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: u64, url: &str, title: &str) -> TabInfo {
        TabInfo::chrome(TabId(id), url, title)
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
    fn merge_preserves_chrome_tab_order() {
        // Engine snapshot order must not undo a user reorder.
        let prev = vec![
            tab(2, "https://b.example/", "B"),
            tab(1, "https://a.example/", "A"),
        ];
        let live = vec![
            tab(1, "https://a.example/", "A"),
            tab(2, "https://b.example/", "B"),
        ];
        let out = merge_tab_snapshot(&prev, &live, &HashSet::new());
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, TabId(2));
        assert_eq!(out[1].id, TabId(1));
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
        let (field, seen) = apply_omnibar_url("exa", BLANK_URL, "https://elsewhere.example/", true);
        assert_eq!(field, "exa");
        assert_eq!(seen, BLANK_URL);
    }
}
