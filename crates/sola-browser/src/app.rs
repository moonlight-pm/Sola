//! Browser chrome: message type, layout constants, and the generic `App<E>`.
//!
//! `Msg` and the consts were stubbed out in Task 1 and are kept here. Task 2
//! adds `App<E>`, its constructor, and all update/view/subscription methods.

mod desk_menu;
mod favicons;
mod pages;
#[cfg(feature = "vault")]
mod vault;
mod videos;
mod wallpaper;
mod webapps;

pub use pages::{boot_pages, count_by_profile};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use iced::widget::{
    Shader, Space, button, column, container, mouse_area, row, stack, text,
};
use iced::{Alignment, Element, Event, Length, Padding, Subscription, Task, event, keyboard, mouse};
use sola_kit::components::button as kit_button;
use sola_kit::components::card;
use sola_kit::components::icon::{icon_handle, icon_svg, icon_svg_colored};

use sola_kit::components::style::{chrome_surface, PAD_CONTROL_SM, RADIUS_MD};
use sola_kit::components::text_input::{MenuEdit, MenuEdited, edit_focused, text_input};
use sola_kit::components::toolbar as kit_toolbar;
use sola_kit::components::{DividerColors, MenuItem, horizontal_divider, menu_at};

use crate::engine::{
    Cmd, EditCmd, Engine, FrameSlot, NavCmd, PageContext, PageMenusHandle, TabId, TabInfo,
    TabsHandle,
};
use crate::page_menu;
use crate::session::{self, SessionTab};
use sola_kit::components::scroll;

/// Cold-start default when the profile session is empty (and for new profiles).
pub const DEFAULT_URL: &str = "about:blank";
/// A fresh blank tab (⌘T). Loaded as an empty page; the chrome shows an empty,
/// focused URL bar rather than the literal "about:blank".
pub const BLANK_URL: &str = "about:blank";
pub const VIEW_W: u32 = 1280;
pub const VIEW_H: u32 = 800;
pub const CHROME_HEIGHT: f32 = sola_kit::components::style::BAR_H;
pub(crate) const NAV_BTN_W: f32 = 34.0;

/// Left/right padding so the history tray matches the location field.
/// Mirrors `view_chrome_bar`: pad + 4 nav buttons before the omnibox,
/// vault + downloads after.
fn omnibox_tray_insets() -> (f32, f32) {
    use sola_kit::components::style::{SPACE_MD, SPACE_SM};
    let pad = SPACE_MD;
    let gap = SPACE_SM;
    let left = pad + NAV_BTN_W * 4.0 + gap * 4.0;
    let vault = if cfg!(feature = "vault") {
        NAV_BTN_W
    } else {
        0.0
    };
    let right = pad + vault + NAV_BTN_W + gap * 2.0;
    (left, right)
}
/// How far in from the bar's right edge the key button's right edge sits
/// (the downloads button is right of it): the vault sheet hangs there.
#[cfg(feature = "vault")]
pub(crate) fn key_button_inset() -> f32 {
    use sola_kit::components::style::{SPACE_MD, SPACE_SM};
    SPACE_MD + NAV_BTN_W + SPACE_SM
}

/// How long the copy-URL glyph stays on the check after a successful copy.
const COPY_URL_FLASH: Duration = Duration::from_millis(1200);

#[derive(Debug, Clone)]
pub enum Msg {
    NewFrame,
    /// Page window `.0` is up (iced's answer to opening it).
    PageWindowUp(iced::window::Id),
    NavBack,
    NavForward,
    /// Press on back/forward — starts a hold timer for the history menu.
    NavHoldStart(HistoryDir),
    /// Hold elapsed — open the session-history menu.
    NavHoldFire,
    /// Jump to a session-history index (from the hold menu).
    NavJump(i32),
    /// Reload when idle; stop when the active tab is loading.
    /// Shift-click the toolbar button is a hard reload (same as [`Self::NavHardReload`]).
    NavReloadOrStop,
    /// `⌘⇧R` — reload ignoring HTTP cache (Chromium hard reload).
    NavHardReload,
    /// Copy the current page URL to the system clipboard.
    CopyUrl,
    /// Escape / explicit stop — always `NavCmd::Stop`.
    NavStop,
    UrlInput(String),
    UrlSubmit,
    /// Shift+Enter: always the Kagi results page for the typed query.
    UrlSearchResults,
    OmniboxSelectNext,
    OmniboxSelectPrev,
    OmniboxPick(usize),
    CloseTab(TabId),
    /// ⌘⇧T — pop the recently-closed stack.
    ReopenClosedTab,
    /// Wrap This Page sheet: open it (the page menu), the name typed, Wrap,
    /// Cancel.
    WrapOpen,
    /// Use as Desktop Background: its lightbox (`app/wallpaper.rs`).
    Wallpaper(wallpaper::Event),
    WrapName(String),
    WrapSubmit,
    WrapCancel,
    /// A web app's Remove Web App… sheet.
    RemoveAppConfirm,
    RemoveAppCancel,
    /// A web app window's titlebar: move, resize, close (by window).
    WindowDrag(iced::window::Id),
    WindowResize(iced::window::Id, iced::window::Direction),
    WindowClose(iced::window::Id),
    /// Drain helper queues (tabs, downloads, copy, menus, vault). Fired
    /// by [`crate::chrome_wake`] or a short-lived 250 ms timer (copy-URL
    /// flash / TOTP / fill wait) — never as an idle vsync pump.
    Tick,
    /// A message delivered over the Sola bus (theme, open-url, menu
    /// action, close-app). Handled by `integration::handle_bus`.
    Bus(Arc<sola_bus::Message>),
    /// `solactl browser` invoke.
    Call(sola_call::Incoming),
    /// Global cursor moved (nav-hold, the DevTools divider).
    CursorMoved(f32, f32),
    /// Global left-button released — ends a nav-hold or a divider drag.
    CursorReleased,
    MenuDismiss,
    /// A left button press landed inside the web view — the page took
    /// keyboard focus, so edit commands route to the engine (not the URL bar).
    WebViewFocused,
    /// Inspector pane took the click.
    DevToolsFocused,
    /// Docked inspector divider pressed.
    DevToolsSplitDrag,
    /// Window height (logical) for inspector divider math.
    WindowResized(f32),
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
    /// What Undo / Redo (`cmd`) did in the focused chrome field (the
    /// field's own history); `None`: no field had focus.
    EditHistory {
        cmd: EditCmd,
        url_bar_focused: bool,
        done: Option<MenuEdited>,
    },
    /// Result of an `iced::clipboard::read` kicked off by a URL-bar paste.
    UrlPasted(Option<String>),
    /// A desk-menu vault value for the page in this tab, redeemed.
    DeskRedeemed(TabId, Result<sola_kit::desk_menu::Secret, sola_kit::desk_menu::VaultError>),
    /// Result of an `iced::clipboard::read` for paste into page content.
    PagePasted(Option<String>),
    /// Compositor clipboard probe for page paste (image or text).
    PageOffer(sola_kit::clipboard::Offer),
    WindowReady(Option<iced::window::Id>),
    /// A page window opened, resized, gained focus, or was asked to close.
    PageWindow(iced::window::Id, iced::window::Event),
    /// A task finished with nothing to do (a page window opened or closed).
    Noop,
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
    /// The vault panel (`app/vault`).
    #[cfg(feature = "vault")]
    Vault(vault::VaultMsg),
    /// Toolbar download icon: open / close the downloads panel.
    DownloadsToggle,
    DownloadsPanelClose,
    /// Cancel an in-progress download (`entry.id`).
    DownloadCancel(String),
    /// Open a completed file (`entry.id`).
    DownloadOpen(String),
    /// Drop a row from the list (file stays on disk).
    DownloadRemove(String),
    /// Site Notification.requestPermission — Allow / Block.
    NotifyAllow,
    NotifyBlock,
    /// getUserMedia / huddle mic (and camera) — Allow / Block.
    MediaAllow,
    MediaBlock,
    /// Page `alert` / `confirm` / `prompt` / leave-page.
    JsDialogOk,
    /// The kit panel over a page answering its file chooser.
    FileDialog(sola_kit::components::file_picker::Message),
    JsDialogCancel,
    JsDialogInput(String),
    HttpAuthUser(String),
    HttpAuthPass(String),
    HttpAuthOk,
    HttpAuthCancel,
    HttpAuthFocusPass,
    /// Clicked the idle omnibox label — become an edit field.
    UrlBarActivate,
    FindOpen,
    FindNext,
    FindPrev,
    FindInput(String),
    FindClose,
    FindResult {
        count: i32,
        ordinal: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryDir {
    Back,
    Forward,
}

/// What an in-window menu is for (a page's right-click is the desk
/// menu, `app/desk_menu.rs`).
#[derive(Debug, Clone)]
enum CtxTarget {
    /// Back or forward held: the session history that way.
    History { forward: bool },
}

struct NavHold {
    dir: HistoryDir,
    started: Instant,
    menu: bool,
}

const NAV_HOLD_MS: u128 = 400;

/// Browser chrome application state, generic over the web engine.
///
/// `engine` owns the worker and keeps it alive for the lifetime of the
/// process — no process-wide statics needed.
pub struct App<E: Engine> {
    /// The running web engine. Kept alive here so neither a static nor an
    /// `Arc` is needed to keep the worker alive.
    pub engine: E,
    /// The page slot template (single-window paths, and where page
    /// windows' slots take their channels). [`Self::slot`] is the live one.
    pub slot: Arc<FrameSlot<E>>,
    /// A window per page (Next): each page's window and frame slot.
    pub windows: crate::page_windows::PageWindows<E>,
    /// The active page the shell was last asked to show: switching to a
    /// page from outside its window (a notification, a call) shows it.
    shown_active: TabId,
    /// The last active switch came from a window gaining focus.
    focus_switch: bool,
    /// Command channel to the engine worker.
    pub cmd_tx: Sender<Cmd<E>>,
    /// Live tab snapshot, owned by the engine. We re-read on
    /// every Tick; `cached_tabs` is the value at last read.
    pub tabs_handle: TabsHandle,
    /// The workspace each page window is on (sola-river id → workspace id,
    /// from the shell's `WindowPlace`).
    window_ws: HashMap<u32, String>,
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
    pub(crate) closed_tabs: HashSet<TabId>,
    /// Pages reopened under another profile: their old ids never come back
    /// from a helper's lagging tab list.
    retired_tabs: HashSet<TabId>,
    /// True after the helper has reported a non-empty tab list this run.
    /// When it later goes empty (helper death), chrome re-opens `cached_tabs`.
    engine_tabs_seen: bool,
    /// LIFO recently-closed tabs (⌘⇧T). Persisted in `session.json`.
    recently_closed: Vec<crate::session::ClosedTab>,
    /// Each page's stable id ([`crate::session::SessionTab::id`]).
    page_ids: HashMap<TabId, String>,
    /// The windows of web apps that were quit, to bring back when opened.
    quit_apps: crate::quit_apps::QuitApps,
    /// Each page's favicon file, named on its tab ([`favicons`]).
    page_icons: HashMap<TabId, String>,
    /// Each page's videos, told to sola-river ([`videos`]).
    videos: videos::Videos,
    /// Web apps: the pages hosted as their own apps ([`webapps`]).
    pub(crate) webapps: webapps::WebApps,
    /// Editable contents of the URL bar.
    pub url_field: String,
    /// The URL we last copied from the engine into `url_field`,
    /// so we only overwrite on actual change.
    pub last_seen_url: String,
    /// Active iced theme, refreshed live from `Topic::Theme` so the
    /// chrome (URL bar, buttons) tracks the system theme.
    pub theme: iced::Theme,
    pub(crate) agent: crate::agent::AgentState,
    context_menu: Option<(iced::Point, CtxTarget)>,
    /// The page's desk menu while the shell has it up.
    desk_menu: Option<desk_menu::OpenMenu>,
    /// Use as Desktop Background, while it runs.
    wallpaper: Option<wallpaper::Lightbox>,
    /// The output's size (`Topic::OutputGeometry`): the shape a desktop
    /// background is cut to.
    screen_size: Option<(u32, u32)>,
    /// Light or dark as last told the pages (`Appearance.light`).
    pages_light: Option<bool>,
    /// The window is up but the restored pages wait for `pages_light`: a
    /// page that reads `prefers-color-scheme` once as it loads would keep
    /// the light it was born with ([`Self::open_restored`]). Until
    /// `Appearance` comes, or the bus says it has replayed everything
    /// (`Topic::Replayed`: there is none).
    scheme_wait: bool,
    /// The bus has replayed what it holds (`Topic::Replayed`).
    replayed: bool,
    nav_hold: Option<NavHold>,
    page_menus: PageMenusHandle,
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
    /// A new blank page (⌘T, a launch) whose URL bar takes the keys once
    /// its window has keyboard focus: the window does not exist yet when
    /// the page opens, so a focus then lands nowhere.
    pub(crate) url_bar_on_focus: Option<TabId>,
    /// Ignore the LeftPressed focus query from the click that entered edit.
    url_bar_activating: bool,
    visits: crate::visit_history::VisitHistory,
    /// Highlight in the omnibox history list (`None` = none yet).
    omnibox_sel: Option<usize>,
    /// Set only by ↓ / ↑ / Ctrl+N / Ctrl+P or a click on a history row.
    /// Enter ignores the list until this is set — the first hit is not implied.
    omnibox_armed: bool,
    /// Load we asked for, held until the engine leaves the previous URL.
    pending_nav: Option<PendingNav>,
    find_open: bool,
    find_query: String,
    find_count: i32,
    find_ordinal: i32,
    find_field_focused: bool,
    /// Last written session fingerprint per profile — skip disk when
    /// unchanged.
    session_fp: HashMap<String, String>,
    /// Pages (with their profiles) to open after the iced Wayland window
    /// exists (session restore).
    pending_session: Option<(Vec<(String, SessionTab)>, usize)>,
    /// The vault panel and its client (`app/vault`).
    #[cfg(feature = "vault")]
    vault: vault::Vault,
    downloads: crate::downloads::DownloadList,
    downloads_panel_open: bool,
    download_icon: iced::widget::svg::Handle,
    /// In-chrome Notification.requestPermission prompt.
    pending_notify: Option<crate::notify::IpcPerm>,
    /// In-chrome getUserMedia / huddle permission prompt.
    pending_media: Option<crate::media::IpcMedia>,
    /// In-chrome `alert` / `confirm` / `prompt` / leave-page.
    pending_js_dialog: Option<crate::js_dialog::Ipc>,
    js_dialog_queue: VecDeque<crate::js_dialog::Ipc>,
    /// A page's file chooser on screen (the kit panel), and those waiting.
    file_dialog: Option<(crate::file_dialog::Ipc, sola_kit::components::FilePicker)>,
    file_dialog_queue: VecDeque<crate::file_dialog::Ipc>,
    /// Where the last upload came from: the next panel starts there.
    upload_dir: Option<std::path::PathBuf>,
    js_dialog_prompt: String,
    pending_http_auth: Option<crate::http_auth::Ipc>,
    http_auth_user: String,
    http_auth_pass: String,
    /// Instant the copy-URL button last succeeded — drives the check flash.
    copy_url_flash: Option<Instant>,
    /// Page tab the docked inspector is attached to (`None` = closed).
    devtools: Option<TabId>,
    /// Fraction of the page column given to the page (inspector below).
    devtools_ratio: f32,
    devtools_dragging: bool,
    /// Logical window height for divider drag.
    window_h: f32,
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
        tabs: Vec<(String, SessionTab)>,
        active_index: usize,
        recently_closed: Vec<crate::session::ClosedTab>,
    ) -> Self {
        let page_menus = engine.page_menus_handle();
        // The last run's page favicons: each page sends its own again.
        favicons::clear();
        let mut app = Self {
            engine,
            windows: crate::page_windows::PageWindows::new(app_id, slot.clone()),
            shown_active: TabId(u64::MAX),
            focus_switch: false,
            slot,
            cmd_tx,
            tabs_handle,
            window_ws: HashMap::new(),
            active_handle,
            cached_tabs: Vec::new(),
            cached_active: TabId(u64::MAX),
            closed_tabs: HashSet::new(),
            retired_tabs: HashSet::new(),
            engine_tabs_seen: false,
            recently_closed,
            page_ids: HashMap::new(),
            quit_apps: crate::quit_apps::QuitApps::load(),
            page_icons: HashMap::new(),
            videos: videos::Videos::default(),
            webapps: webapps::WebApps::default(),
            url_field: String::new(),
            last_seen_url: String::new(),
            theme: sola_kit::theme::default_theme(),
            agent: crate::agent::AgentState::default(),
            context_menu: None,
            desk_menu: None,
            wallpaper: None,
            screen_size: None,
            pages_light: None,
            scheme_wait: false,
            replayed: false,
            nav_hold: None,
            page_menus,
            float: sola_kit::FloatState::new(app_id),
            window_id: None,
            app_id,
            url_bar_focused: false,
            url_bar_on_focus: None,
            url_bar_activating: false,
            visits: crate::visit_history::VisitHistory::load(),
            omnibox_sel: None,
            omnibox_armed: false,
            pending_nav: None,
            find_open: false,
            find_query: String::new(),
            find_count: 0,
            find_ordinal: 0,
            find_field_focused: false,
            session_fp: HashMap::new(),
            pending_session: Some((tabs, active_index)),
            #[cfg(feature = "vault")]
            vault: vault::Vault::new(),
            downloads: crate::downloads::DownloadList::load(),
            downloads_panel_open: false,
            download_icon: icon_handle("lucide/download"),
            pending_notify: None,
            pending_media: None,
            pending_js_dialog: None,
            js_dialog_queue: VecDeque::new(),
            file_dialog: None,
            file_dialog_queue: VecDeque::new(),
            upload_dir: None,
            js_dialog_prompt: String::new(),
            pending_http_auth: None,
            http_auth_user: String::new(),
            http_auth_pass: String::new(),
            copy_url_flash: None,
            devtools: None,
            devtools_ratio: 0.62,
            devtools_dragging: false,
            window_h: VIEW_H as f32,
        };
        let seed: Vec<(String, String)> = app
            .pending_session
            .as_ref()
            .map(|(tabs, _)| {
                let mut v = Vec::new();
                for (_, tab) in tabs {
                    for h in tab.history.iter().rev() {
                        v.push((h.url.clone(), h.title.clone()));
                    }
                    v.push((tab.url.clone(), tab.title.clone()));
                }
                v
            })
            .unwrap_or_default();
        let mut dirty = false;
        for (url, title) in &seed {
            dirty |= app.visits.ingest(url, title);
        }
        if dirty {
            app.visits.save();
        }
        app
    }

    pub(crate) fn set_url_bar_focused(&mut self, on: bool) {
        self.url_bar_focused = on;
        URL_BAR_FOCUSED.store(on, Ordering::Relaxed);
        if on {
            self.find_field_focused = false;
        } else {
            self.omnibox_sel = None;
            self.omnibox_armed = false;
            self.url_bar_on_focus = None;
        }
    }

    fn omnibox_hits(&self) -> Vec<crate::visit_history::Visit> {
        if !self.url_bar_focused {
            return Vec::new();
        }
        self.visits.search(&self.url_field)
    }

    fn note_visit(&mut self, url: &str, title: &str) {
        self.visits.record(url, title);
    }

    /// Enter on a highlighted history row: focus the tab already showing
    /// that page, else load it here. Typed text never jumps tabs.
    fn open_history_pick(&mut self, url: String) -> Task<Msg> {
        if url.is_empty() {
            return Task::none();
        }
        if let Some(id) = tab_showing_url(
            self.cached_tabs.iter().map(|t| (t.id, t.url.as_str())),
            &url,
        ) {
            let leaving = self.cached_active;
            if id != leaving {
                self.switch_active_tab(id);
                // New-tab (⌘T) + a picked row that is already open must
                // not leave the unused about:blank behind.
                self.discard_scratch_tab(leaving);
            }
            self.omnibox_sel = None;
            self.set_url_bar_focused(false);
            self.persist_session();
            return crate::integration::unfocus_chrome();
        }
        self.navigate_to(url)
    }

    /// Drop an unused `about:blank` we just left. Not pushed onto ⌘⇧T.
    fn discard_scratch_tab(&mut self, id: TabId) {
        if id == self.cached_active || self.cached_tabs.len() <= 1 {
            return;
        }
        let scratch = self
            .cached_tabs
            .iter()
            .find(|t| t.id == id)
            .is_some_and(tab_is_scratch_blank);
        if !scratch {
            return;
        }
        self.slot().forget_tab(id);
        self.slot().drop_paint_tabs.lock().unwrap().push(id.0);
        self.slot().need_park_prime.lock().unwrap().remove(&id.0);
        let _ = self.cmd_tx.send(Cmd::CloseTab(id));
        self.closed_tabs.insert(id);
        self.cached_tabs.retain(|t| t.id != id);
    }

    fn navigate_to(&mut self, url: String) -> Task<Msg> {
        if url.is_empty() {
            return Task::none();
        }
        let previous = self
            .active_tab_info()
            .map(|t| t.url.clone())
            .unwrap_or_default();
        self.pending_nav = Some(PendingNav {
            previous,
            requested: url.clone(),
        });
        self.url_field = url.clone();
        self.last_seen_url = url.clone();
        self.set_url_bar_focused(false);
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
        crate::integration::unfocus_chrome()
    }

    fn submit_omnibox(&mut self, search_results: bool) -> Task<Msg> {
        let typed = self.url_field.trim().to_string();
        if typed.is_empty() {
            return Task::none();
        }
        let hits = self.visits.search(&typed);
        let highlighted = chosen_history_url(
            hits.iter().map(|v| v.url.as_str()),
            self.omnibox_sel,
            self.omnibox_armed,
        );
        let url = crate::util::omnibox_submit_url(&typed, highlighted, search_results);
        let picked = highlighted.is_some() && !search_results;
        tracing::debug!(
            %typed,
            selected = ?self.omnibox_sel,
            armed = self.omnibox_armed,
            history = picked,
            %url,
            "omnibox submit"
        );
        self.omnibox_sel = None;
        self.omnibox_armed = false;
        if picked {
            self.open_history_pick(url)
        } else {
            self.navigate_to(url)
        }
    }

    /// Keep a submitted URL on the active tab while the engine is still
    /// reporting the page we left. A redirect (engine URL is neither) shows.
    fn hold_pending_navigation(&mut self) {
        let Some(pending) = self.pending_nav.clone() else {
            return;
        };
        let Some(tab) = self
            .cached_tabs
            .iter_mut()
            .find(|t| t.id == self.cached_active)
        else {
            self.pending_nav = None;
            return;
        };
        match held_navigation_url(&tab.url, &pending) {
            Some(hold) => {
                tab.url = hold;
                tab.is_loading = true;
            }
            None => self.pending_nav = None,
        }
    }

    /// The vault panel is up (it has the keys).
    fn vault_is_open(&self) -> bool {
        #[cfg(feature = "vault")]
        return self.vault.is_open();
        #[cfg(not(feature = "vault"))]
        false
    }

    fn set_downloads_panel_open(&mut self, open: bool) {
        self.downloads_panel_open = open;
        if open {
            self.downloads.mark_seen();
            #[cfg(feature = "vault")]
            self.vault.set_open(false);
        }
    }

    /// Open the restored pages and the `--app` web app (once its catalog
    /// entry is in). After `WindowReady`, and after the pages' light or
    /// dark is sent (or the bus has replayed without one, `Topic::Replayed`).
    fn open_restored(&mut self) {
        self.scheme_wait = false;
        if let Some((tabs, active_index)) = self.pending_session.take() {
            self.bootstrap_tabs(tabs, active_index);
        }
        if let Some(id) = self.webapps.launch.clone() {
            self.open_app(&id);
        }
    }

    /// Clear compositor env so WebKit child processes cannot open a Wayland
    /// window. Safe after iced has already connected (WindowReady).
    /// Open restored tabs in order and focus `active_index`.
    fn bootstrap_tabs(&mut self, tabs: Vec<(String, SessionTab)>, active_index: usize) {
        debug_assert!(!tabs.is_empty(), "bootstrap always has ≥1 tab");
        let session_tabs = tabs;
        let mut ids = Vec::with_capacity(session_tabs.len());
        for (profile, tab) in &session_tabs {
            let id = self.engine.alloc_tab_id();
            self.page_ids.insert(id, tab.id.clone());
            if let Some(app) = &tab.app {
                self.webapps.tabs.insert(id, app.clone());
            }
            let url = crate::util::normalize_url(&tab.url);
            let url = if url.is_empty() {
                BLANK_URL.to_string()
            } else {
                url
            };
            // Optimistic chrome snapshot so the window has a title at once.
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
                profile: profile.clone(),
                ..TabInfo::chrome(id, url.clone(), title.clone())
            });
            // One background frame may be imported to seed park cache.
            self.slot().need_park_prime.lock().unwrap().insert(id.0);
            self.open_in(profile, id, url, title);
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

    pub fn active_tab_info(&self) -> Option<&TabInfo> {
        self.cached_tabs.iter().find(|t| t.id == self.cached_active)
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        let task = self.update_msg(msg);
        let task = Task::batch([task, self.sync_windows()]);
        self.publish_videos();
        task
    }

    /// The page slot the chrome drives: the active page's window, else the
    /// template (before any window).
    pub fn slot(&self) -> Arc<FrameSlot<E>> {
        self.windows
            .slot_of(self.cached_active)
            .unwrap_or_else(|| self.slot.clone())
    }

    /// Keep a window per page: open one for each page without, close the
    /// windows of pages gone, tell the engine which pages are on screen,
    /// and show the active page when it changed from outside its window.
    fn sync_windows(&mut self) -> Task<Msg> {
        let mut tasks = Vec::new();
        let ids: Vec<TabId> = self.cached_tabs.iter().map(|t| t.id).collect();
        let bus_up = sola_kit::app::bus().lock().is_ok_and(|b| b.is_connected());
        let replaying = !self.replayed && bus_up;
        for id in &ids {
            if !self.windows.has(*id) && !self.webapps.holds(*id, replaying) {
                let page_id = self.page_id(*id);
                let background = *id != self.cached_active;
                // A web app's page opens as the app.
                let app_id = self.window_app_id(*id);
                tasks.push(
                    self.windows
                        .open(&app_id, *id, &page_id, background)
                        .map(Msg::PageWindowUp),
                );
            }
        }
        for id in self.windows.open_tabs() {
            if !ids.contains(&id) {
                tasks.push(self.windows.close(id).map(|_| Msg::Noop));
            }
        }
        self.sync_page_icons();
        for (id, shown) in self.windows.take_shown_changes() {
            let _ = self.cmd_tx.send(Cmd::ShowPage { id, shown });
        }
        self.sync_app_sites();
        if self.cached_active != self.shown_active {
            self.shown_active = self.cached_active;
            if !std::mem::take(&mut self.focus_switch) {
                self.windows.show(self.cached_active);
            }
        }
        Task::batch(tasks)
    }

    /// A page window's lifecycle: focus makes its page the active tab;
    /// its close button (or the compositor) closes the page.
    fn on_page_window(&mut self, window: iced::window::Id, event: iced::window::Event) -> Task<Msg> {
        use iced::window::Event as WE;
        match event {
            WE::Focused => {
                self.windows.focused = Some(window);
                let Some(tab) = self.windows.tab_of(window) else {
                    return Task::none();
                };
                if tab != self.cached_active {
                    self.focus_switch = true;
                    self.switch_active_tab(tab);
                    self.persist_session();
                }
                if self.url_bar_on_focus == Some(tab) && tab == self.cached_active {
                    self.url_bar_on_focus = None;
                    self.set_url_bar_focused(true);
                    return Task::batch([
                        crate::integration::focus_url_bar(),
                        crate::integration::select_url_bar(),
                    ]);
                }
                Task::none()
            }
            WE::Unfocused => {
                crate::input::forget_held();
                Task::none()
            }
            WE::CloseRequested => match self.windows.tab_of(window) {
                Some(tab) => self.update_msg(Msg::CloseTab(tab)),
                None => iced::window::close(window),
            },
            WE::Resized(size) => {
                self.windows.resized(window, size);
                Task::none()
            }
            WE::Closed => {
                if let Some(tab) = self.windows.closed(window) {
                    tracing::warn!(tab = tab.0, "page window closed outside the browser");
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn update_msg(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::WindowReady(id) => {
                self.window_id = id;
                // Iced is connected; open the restored pages once they can
                // be told light or dark.
                let bus_up = sola_kit::app::bus().lock().is_ok_and(|b| b.is_connected());
                if self.pages_light.is_none() && !self.replayed && bus_up {
                    self.scheme_wait = true;
                    return Task::none();
                }
                self.open_restored();
                return Task::none();
            }
            Msg::PageWindowUp(window) => return self.windows.up(window).discard(),
            Msg::TitleDrag => return sola_kit::drag(self.windows.window_of(self.cached_active)),
            Msg::TitleResize(dir) => {
                return sola_kit::drag_resize(self.windows.window_of(self.cached_active), dir);
            }
            Msg::TitleClose => {
                let id = self.cached_active;
                return self.update_msg(Msg::CloseTab(id));
            }
            Msg::PageWindow(window, event) => return self.on_page_window(window, event),
            Msg::Noop => {}
            #[cfg(feature = "vault")]
            Msg::Vault(m) => return self.on_vault(m),
            Msg::Wallpaper(event) => return self.on_wallpaper(event),
            Msg::WrapOpen => {
                self.context_menu = None;
                return self.open_wrap_sheet();
            }
            Msg::WrapName(name) => {
                if let Some(draft) = &mut self.webapps.wrap {
                    draft.name = name;
                }
            }
            Msg::WrapSubmit => return self.wrap_page(),
            Msg::WrapCancel => self.webapps.wrap = None,
            Msg::RemoveAppConfirm => return self.remove_app(),
            Msg::RemoveAppCancel => self.webapps.removing = None,
            Msg::WindowDrag(window) => return iced::window::drag(window),
            Msg::WindowResize(window, dir) => return iced::window::drag_resize(window, dir),
            Msg::WindowClose(window) => {
                if let Some(tab) = self.windows.tab_of(window) {
                    return self.update_msg(Msg::CloseTab(tab));
                }
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
                            error: String::new(),
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
            Msg::NotifyAllow => self.resolve_notify_permission("granted"),
            Msg::NotifyBlock => self.resolve_notify_permission("denied"),
            Msg::MediaAllow => self.resolve_media_permission("granted"),
            Msg::MediaBlock => self.resolve_media_permission("denied"),
            Msg::JsDialogOk => return self.resolve_js_dialog(true),
            Msg::FileDialog(m) => return self.on_file_dialog(m),
            Msg::JsDialogCancel => return self.resolve_js_dialog(false),
            Msg::JsDialogInput(s) => {
                self.js_dialog_prompt = s;
            }
            Msg::HttpAuthUser(s) => {
                self.http_auth_user = s;
            }
            Msg::HttpAuthPass(s) => {
                self.http_auth_pass = s;
            }
            Msg::HttpAuthOk => return self.resolve_http_auth(true),
            Msg::HttpAuthCancel => return self.resolve_http_auth(false),
            Msg::HttpAuthFocusPass => {
                return iced::widget::operation::focus(crate::http_auth::pass_input_id());
            }
            Msg::UrlBarActivate => {
                self.set_url_bar_focused(true);
                self.omnibox_sel = None;
                self.omnibox_armed = false;
                self.url_bar_activating = true;
                let _ = self.cmd_tx.send(Cmd::Focus(false));
                return Task::batch([
                    crate::integration::focus_url_bar(),
                    crate::integration::select_url_bar(),
                ]);
            }
            Msg::FindOpen => return self.open_find(),
            Msg::FindNext => return self.find_step(true),
            Msg::FindPrev => return self.find_step(false),
            Msg::FindInput(s) => {
                self.find_query = s;
                self.find_field_focused = true;
                self.send_find(true, false);
            }
            Msg::FindClose => {
                self.close_find();
            }
            Msg::FindResult { count, ordinal } => {
                self.find_count = count;
                self.find_ordinal = ordinal;
            }
            Msg::DownloadRemove(id) => {
                self.downloads.remove(&id);
            }
            Msg::NewFrame => {
                // Allow the next kick if the shader pump stops. While the
                // shader is request_redraw-pumping, frame_stream skips this.
                // Every window redraws after an update, so this serves each
                // page window's slot that queued a kick, not only one.
                self.slot.redraw_queued.store(false, Ordering::Release);
                self.slot.pumping.store(true, Ordering::Release);
                for slot in self.windows.slots().lock().unwrap().iter() {
                    if slot.redraw_queued.swap(false, Ordering::AcqRel) {
                        slot.pumping.store(true, Ordering::Release);
                    }
                }
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
                } else if crate::input::stored_modifiers().shift() {
                    self.set_active_loading(true);
                    let _ = self.cmd_tx.send(Cmd::Nav(NavCmd::ReloadIgnoreCache));
                } else {
                    self.set_active_loading(true);
                    let _ = self.cmd_tx.send(Cmd::Nav(NavCmd::Reload));
                }
            }
            Msg::NavHardReload => {
                self.set_active_loading(true);
                let _ = self.cmd_tx.send(Cmd::Nav(NavCmd::ReloadIgnoreCache));
            }
            Msg::CopyUrl => {
                let page_url = self.active_tab_info().map(|t| t.url.as_str()).unwrap_or("");
                let Some(url) =
                    crate::util::copyable_page_url(page_url, &self.last_seen_url, &self.url_field)
                else {
                    return Task::none();
                };
                self.copy_url_flash = Some(Instant::now());
                tracing::debug!(%url, "copy url → clipboard");
                return iced::clipboard::write(url);
            }
            Msg::NavStop => {
                if self.find_open {
                    self.close_find();
                    return crate::integration::unfocus_chrome();
                }
                if self.url_bar_focused {
                    self.set_url_bar_focused(false);
                    self.url_field = if self.last_seen_url == BLANK_URL {
                        String::new()
                    } else {
                        self.last_seen_url.clone()
                    };
                    return crate::integration::unfocus_chrome();
                }
                if self.context_menu.is_some() {
                    self.context_menu = None;
                    return Task::none();
                }
                #[cfg(feature = "vault")]
                if self.vault.is_open() || self.vault.passkey_pending() {
                    return self.on_vault(vault::VaultMsg::Escape);
                }
                if self.downloads_panel_open {
                    self.set_downloads_panel_open(false);
                    return Task::none();
                }
                self.set_active_loading(false);
                let _ = self.cmd_tx.send(Cmd::Nav(NavCmd::Stop));
            }
            Msg::UrlInput(s) => {
                self.url_field = s;
                self.set_url_bar_focused(true);
                self.omnibox_sel = None;
                self.omnibox_armed = false;
                return crate::integration::focus_url_bar();
            }
            Msg::UrlSubmit => {
                return self.submit_omnibox(false);
            }
            Msg::UrlSearchResults => {
                return self.submit_omnibox(true);
            }
            Msg::OmniboxSelectNext => {
                let n = self.omnibox_hits().len();
                if n == 0 {
                    return crate::integration::focus_url_bar();
                }
                self.omnibox_sel = Some(match self.omnibox_sel {
                    None => 0,
                    Some(i) => (i + 1).min(n - 1),
                });
                self.omnibox_armed = true;
                return crate::integration::focus_url_bar();
            }
            Msg::OmniboxSelectPrev => {
                self.omnibox_sel = match self.omnibox_sel {
                    None | Some(0) => None,
                    Some(i) => Some(i - 1),
                };
                self.omnibox_armed = self.omnibox_sel.is_some();
                return crate::integration::focus_url_bar();
            }
            Msg::OmniboxPick(i) => {
                let n = self.omnibox_hits().len();
                if i < n {
                    self.omnibox_sel = Some(i);
                    self.omnibox_armed = true;
                }
                return crate::integration::focus_url_bar();
            }
            Msg::CloseTab(id) => {
                // A window per page: closing the last page quits (the
                // session keeps it, as quitting does).
                if self.cached_tabs.len() <= 1 {
                    self.persist_session();
                    sola_kit::close_app(self.app_id);
                    return iced::exit();
                }
                // A web app's window is not a browser tab to reopen.
                if self.webapps.tabs.remove(&id).is_none() {
                    self.remember_closed(id);
                }
                self.webapps.favicons.remove(&id);
                self.forget_page_icon(id);
                self.forget_videos(id);
                // If closing the active tab, pick a new active tab
                // first so the engine never sees `active` pointing
                // at a closed tab.
                let was_active = self.cached_active == id;
                if was_active {
                    if let Some(new_active) = self.pick_new_active_after_close(id) {
                        // The shell shows and focuses the next page in the
                        // closed window's zone; focus moves the active tab.
                        self.focus_switch = true;
                        self.switch_active_tab(new_active);
                    }
                }
                self.slot().forget_tab(id);
                self.slot().drop_paint_tabs.lock().unwrap().push(id.0);
                self.slot().need_park_prime.lock().unwrap().remove(&id.0);
                let _ = self.cmd_tx.send(Cmd::CloseTab(id));
                // Drop from optimistic cache immediately so persist sees it
                // and Tick cannot paint the row again.
                self.closed_tabs.insert(id);
                self.cached_tabs.retain(|t| t.id != id);
                self.persist_session();
            }
            Msg::ReopenClosedTab => {
                self.reopen_closed_tab();
            }
            Msg::Call(inc) => return self.on_call(inc),
            Msg::Tick => {
                crate::chrome_wake::take_queued();
                if self
                    .copy_url_flash
                    .is_some_and(|t| t.elapsed() >= COPY_URL_FLASH)
                {
                    self.copy_url_flash = None;
                }
                let mut launched = Task::none();
                while let Some(h) = crate::instance::try_recv_handoff() {
                    match h {
                        crate::instance::Handoff::OpenUrl(url) => {
                            // Its window opens in front and takes focus.
                            tracing::info!(%url, "opening handed-off URL in this chrome");
                            self.open_outside_link(url, true);
                        }
                        crate::instance::Handoff::OpenPage(url) => {
                            tracing::info!(%url, "opening a page handed to this chrome");
                            self.open_tab(url, true);
                        }
                        crate::instance::Handoff::Launch => {
                            // A launch (the shell's launcher, a bare
                            // `sola-browser`) opens a window, like ⌘T.
                            tracing::info!("launch handoff — opening a new page window");
                            let opened = crate::integration::run_intent(
                                self,
                                crate::integration::BrowserIntent::NewBlankTab,
                            );
                            launched = Task::batch([launched, opened]);
                        }
                        crate::instance::Handoff::OpenApp(id) => self.launch_app(&id),
                    }
                }
                #[cfg(feature = "vault")]
                let vault = self.vault_tick();
                #[cfg(not(feature = "vault"))]
                let vault = Task::none();
                self.take_page_menu();
                // Merge engine snapshot with prior cache: WebKit often reports
                // empty title until the page finishes loading (esp. inactive
                // restored tabs). Keep the last known title so the strip does
                // not blank out after session restore.
                let live: Vec<TabInfo> = self
                    .tabs_handle
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|t| !self.retired_tabs.contains(&t.id))
                    .cloned()
                    .collect();
                if !live.is_empty() {
                    self.engine_tabs_seen = true;
                    let prev_ids: HashSet<TabId> = self.cached_tabs.iter().map(|t| t.id).collect();
                    let opener = self.cached_active;
                    self.cached_tabs =
                        merge_tab_snapshot(&self.cached_tabs, &live, &self.closed_tabs);
                    self.closed_tabs
                        .retain(|id| live.iter().any(|t| t.id == *id));
                    let new_popups =
                        place_engine_popup_tabs(&mut self.cached_tabs, &prev_ids, opener);
                    // A web app's popups (a huddle) are the app's.
                    self.adopt_popups(opener, &new_popups);
                    if !new_popups.is_empty() {
                        let engine_active = TabId(self.active_handle.load(Ordering::Relaxed));
                        let focus = new_popups
                            .iter()
                            .copied()
                            .find(|id| *id == engine_active)
                            .unwrap_or(*new_popups.last().unwrap());
                        self.switch_active_tab(focus);
                    }
                }
                // Chrome `paint_tab` is the strip/omnibox authority. The
                // worker `active_handle` can lag a pump tick behind and was
                // clobbering optimistic activate (new-tab had no highlight).
                let paint = self.slot().paint_tab.load(Ordering::Relaxed);
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
                self.hold_pending_navigation();
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
                    if let Some(info) = self.active_tab_info() {
                        if !is_transient_nav_url(&info.url) {
                            let url = info.url.clone();
                            let title = info.title.clone();
                            self.note_visit(&url, &title);
                        }
                    }
                }
                self.persist_session();
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
                #[cfg(feature = "vault")]
                let vault_pages = self.drain_vault_page_events();
                #[cfg(not(feature = "vault"))]
                let vault_pages = Task::none();
                // ⌘-click / target=_blank: helper found a URL; chrome owns the id.
                let bg: Vec<crate::engine::ChromeTabRequest> = self
                    .engine
                    .background_tabs_handle()
                    .lock()
                    .unwrap()
                    .drain(..)
                    .collect();
                self.drain_notify_ipc();
                let js_dlg = self.drain_js_dialogs();
                self.drain_file_dialogs();
                let http_auth = self.drain_http_auth();
                self.drain_find_results();
                self.drain_favicons();
                self.drain_videos();
                self.drain_devtools();
                for req in bg {
                    // The page that asked: the active one (it was clicked).
                    let opener = self.cached_active;
                    if crate::popup::is_devtools_url(&req.url) {
                        tracing::info!(url = %req.url, "ignoring DevTools as a strip tab");
                        continue;
                    }
                    self.open_link(opener, req.url, req.activate, &req.profile);
                }
                // Drain any page-selection / in-page copy the engine extracted.
                // The engine's own clipboard can't reach Wayland; iced's can.
                let clip = self.take_page_clipboard();
                let agent = self.drain_agent();
                return Task::batch([clip, vault, vault_pages, js_dlg, http_auth, agent, launched]);
            }
            Msg::Bus(message) => {
                self.windows.update(&message);
                use sola_bus::topics::Topic;
                match Topic::parse(&message) {
                    Some(Topic::OutputGeometry(g)) if g.width > 0 && g.height > 0 => {
                        self.screen_size = Some((g.width as u32, g.height as u32));
                    }
                    Some(Topic::Replayed) => {
                        self.replayed = true;
                        self.retell_videos();
                        self.open_held_links();
                        if self.scheme_wait {
                            tracing::info!("no Appearance saved: restored pages open without light or dark");
                            self.open_restored();
                        }
                    }
                    Some(Topic::Appearance(look)) if self.pages_light != Some(look.light) => {
                        self.pages_light = Some(look.light);
                        let _ = self.cmd_tx.send(Cmd::ColorScheme { light: look.light });
                        if self.scheme_wait {
                            // The scheme is queued ahead of the pages.
                            self.open_restored();
                        }
                    }
                    _ => {}
                }
                // The tab bar's stoplight: as the window's own close.
                if let Some(window) = self.windows.close_request(&message) {
                    let close = self.on_page_window(window, iced::window::Event::CloseRequested);
                    return close;
                }
                self.on_workspace_message(&message);
                return crate::integration::handle_bus(self, message, self.app_id);
            }
            Msg::CursorMoved(_x, y) => {
                if self.devtools_dragging {
                    let h = (self.window_h - CHROME_HEIGHT).max(120.0);
                    self.devtools_ratio = ((y - CHROME_HEIGHT) / h).clamp(0.25, 0.82);
                }
            }
            Msg::CursorReleased => {
                if self.devtools_dragging {
                    self.devtools_dragging = false;
                    DIVIDER_DRAGGING.store(false, Ordering::Relaxed);
                }
                return self.finish_nav_hold();
            }
            Msg::MenuDismiss => {
                self.context_menu = None;
                self.nav_hold = None;
            }
            Msg::WebViewFocused => {
                // Page took the click: drop iced chrome focus so keys go to
                // the shader → CEF, and tell the host it is the focused OSR
                // widget (caret blink / IME require SetFocus).
                self.set_url_bar_focused(false);
                self.find_field_focused = false;
                self.slot().input_devtools.store(false, Ordering::Relaxed);
                let _ = self.cmd_tx.send(Cmd::Focus(true));
                return crate::integration::unfocus_chrome();
            }
            Msg::DevToolsFocused => {
                self.set_url_bar_focused(false);
                self.find_field_focused = false;
                self.slot().input_devtools.store(true, Ordering::Relaxed);
                let _ = self.cmd_tx.send(Cmd::DevToolsFocus(true));
                return crate::integration::unfocus_chrome();
            }
            Msg::DevToolsSplitDrag => {
                self.devtools_dragging = true;
                DIVIDER_DRAGGING.store(true, Ordering::Relaxed);
            }
            Msg::WindowResized(h) => {
                self.window_h = h.max(1.0);
            }
            Msg::LeftPressed => {
                // A press landed somewhere. Resolve, against the real widget
                // tree, whether it focused the URL bar — `text_input` captures
                // the click so no wrapper can tell us directly.
                return crate::integration::url_bar_is_focused(Msg::UrlBarFocusSync);
            }
            Msg::UrlBarFocusSync(now) => {
                if self.url_bar_activating {
                    self.url_bar_activating = false;
                    if now {
                        return crate::integration::select_url_bar();
                    }
                    // The activating click's global press query often lands
                    // before the field is focused — keep edit mode.
                    return Task::none();
                }
                // Select-all only on the false→true edge, so a second click in
                // an already-focused field can place the caret normally.
                if !now && self.url_bar_focused && !self.visits.search(&self.url_field).is_empty() {
                    return crate::integration::focus_url_bar();
                }
                let gained = now && !self.url_bar_focused;
                self.set_url_bar_focused(now);
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
                // The vault panel owns the Edit actions while it is open (the
                // shell grabs ⌘V; it would otherwise paste into the page).
                #[cfg(feature = "vault")]
                if self.vault.is_open() {
                    return self.vault_edit_action(cmd);
                }
                // Undo / Redo: the focused field's own history (the
                // location bar, the find bar), else the page's.
                let history = match cmd {
                    EditCmd::Undo => Some(MenuEdit::Undo),
                    EditCmd::Redo => Some(MenuEdit::Redo),
                    _ => None,
                };
                if let Some(edit) = history {
                    return edit_focused(edit).map(move |done| Msg::EditHistory {
                        cmd,
                        url_bar_focused,
                        done,
                    });
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
                        // Taken above, by the field's own history.
                        EditCmd::Undo | EditCmd::Redo => Task::none(),
                    };
                }
                return self.edit_page(cmd);
            }
            Msg::EditHistory {
                cmd,
                url_bar_focused,
                done,
            } => {
                return match done {
                    Some(MenuEdited {
                        id: Some(id),
                        value: Some(value),
                        ..
                    }) => {
                        if id == crate::integration::url_input_id() {
                            self.update(Msg::UrlInput(value))
                        } else if id == crate::integration::find_input_id() {
                            self.update(Msg::FindInput(value))
                        } else {
                            Task::none()
                        }
                    }
                    // A field had focus, with nothing to undo or redo.
                    Some(_) => Task::none(),
                    // The location bar is up but not yet focused.
                    None if url_bar_focused => Task::none(),
                    None => self.edit_page(cmd),
                };
            }
            Msg::UrlPasted(text) => {
                let Some(s) = crate::util::usable_clipboard_text(text) else {
                    return Task::none();
                };
                // Omnibox paste replaces: ⌘L / click select-all, and iced
                // does not expose the caret to Edit-menu routing.
                self.url_field = s.clone();
                // Restore: smithay receive can drop the original offer.
                return iced::clipboard::write(s);
            }
            Msg::DeskRedeemed(tab, value) => self.desk_redeemed(tab, value),
            Msg::PageOffer(offer) => {
                use sola_kit::clipboard::Offer;
                return match offer {
                    Offer::Empty => iced::clipboard::read().map(Msg::PagePasted),
                    other => {
                        crate::page_paste::send(&self.cmd_tx, other);
                        Task::none()
                    }
                };
            }
            Msg::PagePasted(text) => {
                let Some(s) = crate::util::usable_clipboard_text(text) else {
                    return Task::none();
                };
                let _ = self.cmd_tx.send(Cmd::PasteText(s.clone()));
                return iced::clipboard::write(s);
            }
        }
        Task::none()
    }

    /// The stable id of a page (minted the first time it is asked for).
    pub(crate) fn page_id(&mut self, id: TabId) -> String {
        self.page_ids
            .entry(id)
            .or_insert_with(crate::profiles::new_uuid)
            .clone()
    }

    pub(crate) fn remember_closed(&mut self, id: TabId) {
        let Some(index) = self.cached_tabs.iter().position(|t| t.id == id) else {
            return;
        };
        let page_id = self.page_id(id);
        let tab = &self.cached_tabs[index];
        let entry = crate::session::ClosedTab {
            id: page_id,
            url: if tab.url.is_empty() {
                BLANK_URL.to_string()
            } else {
                tab.url.clone()
            },
            title: tab.title.clone(),
            index,
            history: tab
                .history
                .iter()
                .map(|e| crate::session::SessionHistory {
                    url: e.url.clone(),
                    title: e.title.clone(),
                })
                .collect(),
            history_index: tab.history_index,
        };
        crate::session::push_closed(&mut self.recently_closed, entry);
    }

    fn reopen_closed_tab(&mut self) {
        let Some(closed) = self.recently_closed.pop() else {
            tracing::debug!("reopen closed tab: stack empty");
            return;
        };
        let url = crate::util::normalize_url(&closed.url);
        let url = if url.is_empty() {
            BLANK_URL.to_string()
        } else {
            url
        };
        let id = self.engine.alloc_tab_id();
        let title = if url == BLANK_URL && closed.title.is_empty() {
            "New Tab".to_string()
        } else {
            closed.title.clone()
        };
        let session_tab = crate::session::SessionTab {
            id: if closed.id.is_empty() {
                crate::profiles::new_uuid()
            } else {
                closed.id.clone()
            },
            url: url.clone(),
            title: title.clone(),
            history: closed.history,
            history_index: closed.history_index,
            app: None,
        };
        self.page_ids.insert(id, session_tab.id.clone());
        let (history, history_index) = session::history_from_session(&session_tab);
        // ⌘⇧T reopens under the showing workspace's profile.
        let profile = crate::profiles::showing_profile();
        let info = TabInfo {
            is_loading: url != BLANK_URL && !url.is_empty(),
            history,
            history_index,
            profile: profile.clone(),
            ..TabInfo::chrome(id, url.clone(), title.clone())
        };
        let idx = closed.index.min(self.cached_tabs.len());
        self.cached_tabs.insert(idx, info);
        self.open_in(&profile, id, url, title);
        self.switch_active_tab(id);
        self.persist_session();
        tracing::info!(id = id.0, "reopened closed tab");
    }

    /// Open a new tab loading `url`, focusing it when `activate`. Called from
    /// app-menu intents (e.g., ⌘T for new tab) and bus-driven OpenUrl via
    /// `integration::run_intent`. Last in the page list.
    /// A new page under the showing workspace's profile.
    pub fn open_tab(&mut self, url: String, activate: bool) -> TabId {
        let profile = crate::profiles::showing_profile();
        self.mint_tab(url, activate, false, &profile)
    }

    /// ⌘-click or `target=_blank`: next to the current tab in the page
    /// list, under its opener's profile.
    pub fn open_tab_beside(&mut self, url: String, activate: bool, profile: &str) -> TabId {
        self.mint_tab(url, activate, true, profile)
    }

    fn mint_tab(&mut self, url: String, activate: bool, beside: bool, profile: &str) -> TabId {
        let url = crate::util::normalize_url(&url);
        let id = self.engine.alloc_tab_id();
        let title = if url == BLANK_URL {
            "New Tab".to_string()
        } else {
            String::new()
        };
        let info = TabInfo {
            is_loading: url != BLANK_URL && !url.is_empty(),
            profile: profile.to_string(),
            ..TabInfo::chrome(id, url.clone(), title.clone())
        };
        if beside {
            insert_beside(&mut self.cached_tabs, self.cached_active, info);
        } else {
            self.cached_tabs.push(info);
        }
        if !activate {
            // Background open (e.g. cmd-click): allow one park prime frame.
            self.slot().need_park_prime.lock().unwrap().insert(id.0);
        }
        self.open_in(profile, id, url, title);
        if activate {
            self.switch_active_tab(id);
        }
        self.persist_session();
        id
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
        // The location bar was the other page's: a page that becomes active
        // (⌘W back to the opener, a click on its window) starts unfocused,
        // or its history suggestions would open over it. ⌘T / ⌘L focus it
        // after switching.
        if id != self.cached_active {
            self.set_url_bar_focused(false);
        }
        self.cached_active = id;
        // Optimistic: worker frame filter reads this before SetActiveTab is
        // pumped. Without this, every buffer-rendered is drop_bg → black page.
        self.pending_nav = None;
        self.active_handle
            .store(id.0, std::sync::atomic::Ordering::Relaxed);
        // Same-size park → pending this frame. Miss → blank now (do not
        // keep sampling the previous tab until CEF answers). A page window's
        // slot only ever paints its own page.
        let slot = self.slot();
        if !slot.windowed {
            slot.present_tab(id);
        }
        let _ = self.cmd_tx.send(Cmd::SetActiveTab(id));
        if self.find_open && !self.find_query.trim().is_empty() {
            self.send_find(true, false);
        }
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
    pub fn theme(&self, window: iced::window::Id) -> iced::Theme {
        sola_kit::theme_for(self.window_chrome(window), &self.theme)
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

    /// A page window: the active page's window carries the full chrome,
    /// any other shows its page live under a read-only bar.
    pub fn view(&self, window: iced::window::Id) -> Element<'_, Msg> {
        let Some(tab) = self.windows.tab_of(window) else {
            return Space::new().into();
        };
        let Some(slot) = self.windows.slot_of(tab) else {
            return Space::new().into();
        };
        if let Some(app) = self.webapps.app_of(tab) {
            self.view_app(window, tab, slot, app)
        } else if tab == self.cached_active {
            self.view_active(window, slot)
        } else {
            self.view_passive(window, tab, slot)
        }
    }

    /// A page window that is not the active page: its location (read only)
    /// over the page. A click focuses the window, which makes it active.
    fn view_passive(
        &self,
        window: iced::window::Id,
        tab: TabId,
        slot: Arc<FrameSlot<E>>,
    ) -> Element<'_, Msg> {
        let info = self.cached_tabs.iter().find(|t| t.id == tab);
        let url = info
            .map(|t| t.url.as_str())
            .filter(|u| *u != BLANK_URL)
            .unwrap_or("");
        let muted = self.theme.extended_palette().background.strong.text;
        let bar = container(
            text(url.to_string())
                .size(13)
                .font(sola_kit::fonts::ui())
                .wrapping(iced::widget::text::Wrapping::None)
                .style(move |_t: &iced::Theme| iced::widget::text::Style { color: Some(muted) }),
        )
        .padding(Padding::from([0.0, sola_kit::components::style::SPACE_MD * 2.0]))
        .height(Length::Fixed(CHROME_HEIGHT))
        .width(Length::Fill)
        .align_y(Alignment::Center)
        .style(|_t: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(chrome_surface())),
            ..Default::default()
        });
        let webview = Shader::new(E::make_program(slot))
            .width(Length::Fill)
            .height(Length::Fill);
        let content: Element<'_, Msg> = column![bar, horizontal_divider(), webview]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        sola_kit::window_frame(
            self.window_chrome(window),
            info.map(|t| t.title.clone()).unwrap_or_default(),
            Msg::TitleDrag,
            Some(Msg::TitleClose),
            Msg::TitleResize,
            content,
        )
    }

    /// How to frame `window`: tiled, floating, or bare in Cinema.
    fn window_chrome(&self, window: iced::window::Id) -> sola_kit::Chrome {
        self.windows
            .river_id(window)
            .map_or(sola_kit::Chrome::Tiled, |id| self.float.chrome(id))
    }

    /// The window title: the page's title and address; a web app's name, or
    /// the page's title while the app has several windows (their rows must
    /// differ, under the app's name).
    pub fn title(&self, window: iced::window::Id) -> String {
        let tab = self.windows.tab_of(window);
        if let Some(app) = tab.and_then(|t| self.webapps.app_of(t)) {
            let page = tab
                .filter(|_| self.webapps.tabs_of(&app.app_id).len() > 1)
                .and_then(|tab| self.cached_tabs.iter().find(|t| t.id == tab))
                .map(|t| t.title.trim())
                .filter(|t| !t.is_empty());
            return page.unwrap_or(&app.label).to_string();
        }
        match tab.and_then(|tab| self.cached_tabs.iter().find(|t| t.id == tab)) {
            Some(t) => crate::util::browser_window_title(&t.title, &t.url),
            None => crate::util::browser_window_title("", ""),
        }
    }

    /// The active page's window, with all of the chrome.
    fn view_active(&self, window: iced::window::Id, slot: Arc<FrameSlot<E>>) -> Element<'_, Msg> {
        let webview = Shader::new(E::make_program(slot.clone()))
            .width(Length::Fill)
            .height(Length::Fill);
        let chrome_keys = !self.url_bar_focused
            && !self.find_field_focused
            && self.pending_js_dialog.is_none()
            && self.file_dialog.is_none()
            && self.wallpaper.is_none()
            && !self.downloads_panel_open
            && !self.vault_is_open();
        let dt_keys = slot.input_devtools.load(Ordering::Relaxed);
        let page_owns_keys = chrome_keys && !dt_keys;
        let webview = crate::cef::page_ime::page_ime(webview, slot.clone(), page_owns_keys);

        let page: Element<'_, Msg> = if self.find_open {
            stack![webview, self.view_find_bar()].into()
        } else {
            webview.into()
        };
        let page: Element<'_, Msg> = if self.devtools == Some(self.cached_active) {
            let inspector = Shader::new(E::make_devtools_program(slot.clone()))
                .width(Length::Fill)
                .height(Length::Fill);
            let inspector = crate::cef::page_ime::page_ime(
                inspector,
                slot.clone(),
                chrome_keys && dt_keys,
            );
            let canvas = self.theme.extended_palette().background.base.color;
            let border = self.theme.extended_palette().background.strong.color;
            sola_kit::components::split_with(
                sola_bus::topics::SplitDir::Horizontal,
                page,
                self.devtools_ratio,
                Msg::DevToolsSplitDrag,
                inspector,
                DividerColors::uniform(canvas, border),
            )
        } else {
            page
        };
        let lower = container(page).width(Length::Fill).height(Length::Fill);
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

        // Always stack so the location bar widget path is stable — wrapping
        // only when hits appear remounts the field and drops focus.
        let content: Element<'_, Msg> = stack![content, self.view_omnibox_hits()].into();

        let content: Element<'_, Msg> = if self.downloads_panel_open {
            stack![content, self.view_downloads_panel()].into()
        } else {
            content
        };

        #[cfg(feature = "vault")]
        let content: Element<'_, Msg> = if self.vault.is_open() {
            stack![content, self.view_vault_panel()].into()
        } else {
            content
        };

        let content: Element<'_, Msg> = if self.webapps.wrap.is_some() {
            stack![content, self.view_wrap_sheet()].into()
        } else if let Some(lightbox) = self.view_wallpaper() {
            stack![content, lightbox].into()
        } else if let Some(modal) = self.page_modal() {
            stack![content, modal].into()
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

        sola_kit::window_frame(
            self.window_chrome(window),
            self.title(window),
            Msg::TitleDrag,
            Some(Msg::TitleClose),
            Msg::TitleResize,
            content,
        )
    }

    /// The page's own dialog, if one is up (sign-in, JS dialog, camera /
    /// microphone, notifications): over the active page's window, a
    /// browser page's or a web app's.
    fn page_modal(&self) -> Option<Element<'_, Msg>> {
        if let Some((_, picker)) = &self.file_dialog {
            Some(picker.overlay().map(Msg::FileDialog))
        } else if self.pending_http_auth.is_some() {
            Some(self.view_http_auth())
        } else if self.pending_js_dialog.is_some() {
            Some(self.view_js_dialog())
        } else if self.pending_media.is_some() {
            Some(self.view_media_permission())
        } else if self.pending_notify.is_some() {
            Some(self.view_notify_permission())
        } else {
            None
        }
    }

    fn menu_items(&self, target: &CtxTarget) -> Vec<MenuItem<Msg>> {
        match target {
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

    fn drain_devtools(&mut self) {
        let evs: Vec<crate::engine::DevToolsEvent> = self
            .engine
            .devtools_handle()
            .lock()
            .unwrap()
            .drain(..)
            .collect();
        for ev in evs {
            match ev {
                crate::engine::DevToolsEvent::Opened { tab_id, paint_id } => {
                    self.devtools = Some(tab_id);
                    self.slot().devtools_tab.store(paint_id.0, Ordering::Relaxed);
                    self.slot().devtools_blank.store(true, Ordering::Relaxed);
                    self.slot().input_devtools.store(true, Ordering::Relaxed);
                    if let Some(frame) = self.slot().parked_frames.lock().unwrap().get(&paint_id.0) {
                        *self.slot().devtools_pending.lock().unwrap() =
                            Some(crate::engine::PendingFrame {
                                tab_id: paint_id,
                                frame: frame.clone(),
                            });
                        self.slot().devtools_blank.store(false, Ordering::Relaxed);
                    }
                    tracing::info!(?tab_id, ?paint_id, "DevTools panel open");
                }
                crate::engine::DevToolsEvent::Closed { tab_id } => {
                    if self.devtools == Some(tab_id) || self.devtools.is_some() {
                        self.devtools = None;
                        self.slot().devtools_tab.store(u64::MAX, Ordering::Relaxed);
                        self.slot().devtools_pending.lock().unwrap().take();
                        self.slot().input_devtools.store(false, Ordering::Relaxed);
                        tracing::info!(?tab_id, "DevTools panel closed");
                    }
                }
            }
        }
    }

    /// The active page window's top bar: nav, location bar, vault, downloads.
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
        let page_url = info.map(|t| t.url.as_str()).unwrap_or("");
        let copy_enabled =
            crate::util::copyable_page_url(page_url, &self.last_seen_url, &self.url_field)
                .is_some();
        let copy_flashing = self
            .copy_url_flash
            .is_some_and(|t| t.elapsed() < COPY_URL_FLASH);
        let copy_url = self.nav_icon_btn(
            if copy_flashing {
                nav_icon_copy_done()
            } else {
                nav_icon_copy()
            },
            16,
            copy_enabled,
            NAV_BTN_W,
            Msg::CopyUrl,
            muted,
        );

        #[cfg(feature = "vault")]
        let vault_btn = self.vault_key_button(muted);
        #[cfg(not(feature = "vault"))]
        let vault_btn = Space::new().width(Length::Fixed(0.0));

        let downloads_btn = self.downloads_tool_btn(muted);

        let bar = row![
            back,
            forward,
            reload_or_stop,
            copy_url,
            self.view_omnibox(),
            vault_btn,
            downloads_btn,
        ]
        .spacing(SPACE_SM)
        .padding([SPACE_SM, SPACE_MD])
        .align_y(Alignment::Center)
        .height(Length::Fixed(CHROME_HEIGHT));

        container(bar)
            .width(Length::Fill)
            .style(|_t: &iced::Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(chrome_surface())),
                ..Default::default()
            })
            .into()
    }

    fn view_omnibox(&self) -> Element<'_, Msg> {
        let field: Element<'_, Msg> = if self.url_bar_focused {
            text_input("Search or enter URL", &self.url_field)
                .id(crate::integration::url_input_id())
                .on_input(Msg::UrlInput)
                .size(13)
                .style(sola_kit::components::text_input::style)
                .width(Length::Fill)
                .into()
        } else {
            let shown = crate::util::idle_display_url(&self.url_field);
            let idle = if shown.is_empty() {
                text("Search or enter URL")
                    .size(13)
                    .font(sola_kit::fonts::ui())
                    .style(|theme: &iced::Theme| {
                        let t = theme.extended_palette().secondary.base.text;
                        iced::widget::text::Style {
                            color: Some(iced::Color { a: 0.55, ..t }),
                        }
                    })
            } else {
                text(shown)
                    .size(13)
                    .font(sola_kit::fonts::ui())
                    .wrapping(iced::widget::text::Wrapping::None)
                    .style(|theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(theme.extended_palette().background.base.text),
                    })
            };
            mouse_area(
                container(idle)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(sola_kit::components::text_input::DEFAULT_PADDING)
                    .align_y(Alignment::Center)
                    .clip(true),
            )
            .on_press(Msg::UrlBarActivate)
            .into()
        };
        let field = container(field).width(Length::Fill).clip(true);
        match self.active_load_frac() {
            Some(frac) => stack![field, omnibox_progress_overlay(frac)].into(),
            None => field.into(),
        }
    }

    fn view_omnibox_hits(&self) -> Element<'_, Msg> {
        use sola_kit::components::style::{HAIRLINE_A, RADIUS_MD, SPACE_XS, mix_white};
        let hits = self.omnibox_hits();
        if hits.is_empty() {
            return Space::new()
                .width(Length::Shrink)
                .height(Length::Shrink)
                .into();
        }
        let mut list = column![].spacing(SPACE_XS).width(Length::Fill);
        for (i, hit) in hits.iter().enumerate() {
            let selected = self.omnibox_sel == Some(i);
            let title = if hit.title.trim().is_empty() {
                crate::util::compact_history_url(&hit.url)
            } else {
                crate::util::truncate(hit.title.trim(), 56)
            };
            let url_line = crate::util::compact_history_url(&hit.url);
            let body = column![
                text(title)
                    .size(13)
                    .font(sola_kit::fonts::ui_medium())
                    .wrapping(iced::widget::text::Wrapping::None),
                text(url_line).size(11).style(|theme: &iced::Theme| {
                    let t = theme.extended_palette().background.base.text;
                    iced::widget::text::Style {
                        color: Some(iced::Color { a: 0.62, ..t }),
                    }
                }),
            ]
            .spacing(1)
            .width(Length::Fill);
            let row = container(body)
                .padding(Padding::from([8, 14]))
                .width(Length::Fill)
                .style(move |theme: &iced::Theme| {
                    let p = theme.extended_palette();
                    let bg = if selected {
                        p.background.strong.color
                    } else {
                        iced::Color::TRANSPARENT
                    };
                    iced::widget::container::Style {
                        background: Some(iced::Background::Color(bg)),
                        border: iced::Border {
                            color: iced::Color::TRANSPARENT,
                            width: 0.0,
                            radius: RADIUS_MD.into(),
                        },
                        ..Default::default()
                    }
                });
            list = list.push(mouse_area(row).on_press(Msg::OmniboxPick(i)));
        }
        let tray = container(list)
            .width(Length::Fill)
            .padding(Padding::from([6, 8]))
            .style(move |_t: &iced::Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(chrome_surface())),
                border: iced::Border {
                    color: mix_white(chrome_surface(), HAIRLINE_A),
                    width: 1.0,
                    radius: iced::border::Radius {
                        top_left: 0.0,
                        top_right: 0.0,
                        bottom_right: RADIUS_MD,
                        bottom_left: RADIUS_MD,
                    },
                },
                shadow: iced::Shadow {
                    color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.32),
                    offset: iced::Vector::new(0.0, 8.0),
                    blur_radius: 20.0,
                },
                ..Default::default()
            });
        let (left, right) = omnibox_tray_insets();
        container(tray)
            .width(Length::Fill)
            .padding(Padding {
                top: CHROME_HEIGHT,
                right,
                bottom: 0.0,
                left,
            })
            .align_x(Alignment::Start)
            .align_y(Alignment::Start)
            .into()
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
                    tool_btn_open
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
            self.open_desk_menu(ctx);
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

    fn drain_notify_ipc(&mut self) {
        let evs: Vec<crate::notify::Ipc> = self
            .engine
            .notifications_handle()
            .lock()
            .unwrap()
            .drain(..)
            .collect();
        for ev in evs {
            match ev {
                crate::notify::Ipc::Show(show) => {
                    let perm = crate::notify::permission_for(
                        &self.profile_of(TabId(show.tab_id)),
                        &show.origin,
                    );
                    if perm != "granted" {
                        tracing::info!(
                            origin = %show.origin,
                            %perm,
                            "notify: drop show (origin not granted)"
                        );
                        continue;
                    }
                    tracing::info!(
                        origin = %show.origin,
                        title = %show.title,
                        "notify: emit AppNotification"
                    );
                    if let Ok(mut bus) = sola_kit::app::bus().lock() {
                        let _ = bus.emit(sola_bus::topics::Topic::AppNotification(
                            self.notification_for(&show),
                        ));
                    }
                }
                crate::notify::Ipc::Perm(perm) => {
                    tracing::info!(
                        origin = %perm.origin,
                        prompt_id = perm.req_id,
                        "notification permission request"
                    );
                    let known = crate::notify::permission_for(
                        &self.profile_of(TabId(perm.tab_id)),
                        &perm.origin,
                    );
                    if known != "default" {
                        // The asking page's helper (pages of several
                        // profiles live at once).
                        let _ = self.cmd_tx.send(Cmd::Target {
                            tab: TabId(perm.tab_id),
                            cmd: Box::new(Cmd::EvaluateJs(crate::notify::resolve_script(
                                perm.req_id,
                                &known,
                            ))),
                        });
                        continue;
                    }
                    if self.pending_notify.is_none() {
                        self.pending_notify = Some(perm);
                    }
                }
                crate::notify::Ipc::Media(m) => self.on_media_ipc(m),
            }
        }
    }

    fn drain_js_dialogs(&mut self) -> Task<Msg> {
        let evs: Vec<crate::js_dialog::Event> = self
            .engine
            .js_dialogs_handle()
            .lock()
            .unwrap()
            .drain(..)
            .collect();
        let mut focus = Task::none();
        for ev in evs {
            match ev {
                crate::js_dialog::Event::Open(dlg) => {
                    tracing::info!(
                        id = dlg.id,
                        origin = %dlg.origin,
                        kind = ?dlg.kind,
                        "js dialog"
                    );
                    if self.pending_js_dialog.is_none() {
                        focus = self.show_js_dialog(dlg);
                    } else {
                        self.js_dialog_queue.push_back(dlg);
                    }
                }
                crate::js_dialog::Event::Reset { ids } => {
                    let drop = |d: &crate::js_dialog::Ipc| ids.contains(&d.id);
                    self.js_dialog_queue.retain(|d| !drop(d));
                    if self.pending_js_dialog.as_ref().is_some_and(drop) {
                        self.pending_js_dialog = None;
                        self.js_dialog_prompt.clear();
                        if let Some(next) = self.js_dialog_queue.pop_front() {
                            focus = self.show_js_dialog(next);
                        } else {
                            crate::js_dialog::set_open(false);
                        }
                    }
                }
            }
        }
        focus
    }

    fn drain_file_dialogs(&mut self) {
        let asks: Vec<crate::file_dialog::Ipc> = self
            .engine
            .file_dialogs_handle()
            .lock()
            .unwrap()
            .drain(..)
            .collect();
        for ask in asks {
            tracing::info!(id = ask.id, mode = ?ask.mode, "file dialog");
            if self.file_dialog.is_none() {
                self.show_file_dialog(ask);
            } else {
                self.file_dialog_queue.push_back(ask);
            }
        }
    }

    /// The page that asked comes forward, the panel over it.
    fn show_file_dialog(&mut self, ask: crate::file_dialog::Ipc) {
        if ask.tab_id != 0
            && self.cached_active.0 != ask.tab_id
            && self.cached_tabs.iter().any(|t| t.id.0 == ask.tab_id)
        {
            self.switch_active_tab(TabId(ask.tab_id));
        }
        let picker = crate::file_dialog::picker(&ask, self.upload_dir.as_deref());
        crate::file_dialog::set_open(true);
        self.file_dialog = Some((ask, picker));
    }

    fn on_file_dialog(&mut self, m: sola_kit::components::file_picker::Message) -> Task<Msg> {
        use sola_kit::components::file_picker::Outcome;
        let Some((ask, picker)) = self.file_dialog.as_mut() else {
            return Task::none();
        };
        let outcome = picker.update(m);
        let follow = picker.scroll_task().map(Msg::FileDialog);
        let paths: Option<Vec<std::path::PathBuf>> = match outcome {
            None => return follow,
            Some(Outcome::Cancelled) => None,
            Some(Outcome::Picked(p)) => Some(vec![p]),
            Some(Outcome::PickedMany(ps)) => Some(ps),
        };
        let id = ask.id;
        if let Some(first) = paths.as_ref().and_then(|ps| ps.first()) {
            let dir = if first.is_dir() { Some(first.as_path()) } else { first.parent() };
            self.upload_dir = dir.map(std::path::Path::to_path_buf);
        }
        let _ = self.cmd_tx.send(Cmd::FileDialog {
            id,
            paths: paths.map(|ps| {
                ps.into_iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect()
            }),
        });
        self.file_dialog = None;
        match self.file_dialog_queue.pop_front() {
            Some(next) => self.show_file_dialog(next),
            None => crate::file_dialog::set_open(false),
        }
        Task::none()
    }

    fn show_js_dialog(&mut self, dlg: crate::js_dialog::Ipc) -> Task<Msg> {
        crate::js_dialog::set_kind(Some(dlg.kind));
        self.js_dialog_prompt = dlg.default_prompt.clone();
        if dlg.tab_id != 0 && self.cached_active.0 != dlg.tab_id {
            if self.cached_tabs.iter().any(|t| t.id.0 == dlg.tab_id) {
                self.switch_active_tab(TabId(dlg.tab_id));
            }
        }
        let prompt = dlg.kind == crate::js_dialog::Kind::Prompt;
        self.pending_js_dialog = Some(dlg);
        if prompt {
            Task::batch([
                iced::widget::operation::focus(crate::js_dialog::prompt_input_id()),
                iced::advanced::widget::operate(
                    iced::advanced::widget::operation::text_input::select_all::<Msg>(
                        crate::js_dialog::prompt_input_id(),
                    ),
                ),
            ])
        } else {
            crate::integration::unfocus_chrome()
        }
    }

    fn resolve_js_dialog(&mut self, success: bool) -> Task<Msg> {
        let Some(dlg) = self.pending_js_dialog.take() else {
            crate::js_dialog::set_open(false);
            return Task::none();
        };
        let input = if dlg.kind == crate::js_dialog::Kind::Prompt {
            std::mem::take(&mut self.js_dialog_prompt)
        } else {
            String::new()
        };
        let _ = self.cmd_tx.send(Cmd::JsDialog {
            id: dlg.id,
            success,
            input,
        });
        if let Some(next) = self.js_dialog_queue.pop_front() {
            self.show_js_dialog(next)
        } else {
            crate::js_dialog::set_open(false);
            Task::none()
        }
    }

    fn drain_find_results(&mut self) {
        let evs: Vec<crate::engine::FindResult> = self
            .engine
            .find_results_handle()
            .lock()
            .unwrap()
            .drain(..)
            .collect();
        if let Some(last) = evs.last().copied() {
            self.find_count = last.count;
            self.find_ordinal = last.ordinal;
        }
    }

    fn begin_find(&mut self) {
        // ⌘F from the omnibox moves focus here; the omnibox must not keep
        // claiming Enter (the key route follows URL_BAR_FOCUSED).
        self.set_url_bar_focused(false);
        self.find_open = true;
        self.find_field_focused = true;
        FIND_OPEN.store(true, Ordering::Relaxed);
    }

    fn focus_find(&self) -> Task<Msg> {
        Task::batch([
            iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::focus::<
                Msg,
            >(crate::integration::find_input_id())),
            iced::advanced::widget::operate(
                iced::advanced::widget::operation::text_input::select_all::<Msg>(
                    crate::integration::find_input_id(),
                ),
            ),
        ])
    }

    fn open_find(&mut self) -> Task<Msg> {
        self.begin_find();
        if !self.find_query.is_empty() {
            self.send_find(true, false);
        }
        self.focus_find()
    }

    fn close_find(&mut self) {
        self.find_open = false;
        self.find_field_focused = false;
        self.find_count = 0;
        self.find_ordinal = 0;
        FIND_OPEN.store(false, Ordering::Relaxed);
        let _ = self.cmd_tx.send(Cmd::StopFind { clear: true });
    }

    fn find_step(&mut self, forward: bool) -> Task<Msg> {
        if !self.find_open {
            self.begin_find();
            if !self.find_query.is_empty() {
                self.send_find(forward, false);
            }
            return self.focus_find();
        }
        if self.find_query.is_empty() {
            return self.focus_find();
        }
        self.send_find(forward, true);
        Task::none()
    }

    fn send_find(&mut self, forward: bool, next: bool) {
        let text = self.find_query.trim();
        if text.is_empty() {
            let _ = self.cmd_tx.send(Cmd::StopFind { clear: true });
            self.find_count = 0;
            self.find_ordinal = 0;
            return;
        }
        let _ = self.cmd_tx.send(Cmd::Find {
            text: text.to_string(),
            forward,
            next,
        });
    }

    fn view_find_bar(&self) -> Element<'_, Msg> {
        use sola_kit::components::style::{
            chrome_surface, HAIRLINE_STRONG_A, RADIUS_MD, SPACE_MD, SPACE_SM, hairline_on,
        };

        let field = text_input("Find in page", &self.find_query)
            .id(crate::integration::find_input_id())
            .on_input(Msg::FindInput)
            .on_submit(Msg::FindNext)
            .size(13)
            .style(sola_kit::components::text_input::style)
            .width(Length::Fixed(180.0));
        let count = if self.find_query.trim().is_empty() {
            String::new()
        } else if self.find_count <= 0 {
            "No results".into()
        } else {
            format!("{} of {}", self.find_ordinal.max(1), self.find_count)
        };
        let count_el = text(count).size(11).style(|theme: &iced::Theme| {
            let t = theme.extended_palette().secondary.base.text;
            iced::widget::text::Style {
                color: Some(iced::Color { a: 0.8, ..t }),
            }
        });
        let prev =
            kit_toolbar::toolbar_icon_tip(nav_icon_find_prev(), "Previous", Some(Msg::FindPrev));
        let next = kit_toolbar::toolbar_icon_tip(nav_icon_find_next(), "Next", Some(Msg::FindNext));
        let close = kit_toolbar::toolbar_icon_tip(nav_icon_stop(), "Close", Some(Msg::FindClose));
        let row = row![field, count_el, prev, next, close]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center);
        let panel = container(row)
            .padding([SPACE_SM, SPACE_MD])
            .style(move |_t: &iced::Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(chrome_surface())),
                border: hairline_on(chrome_surface(), HAIRLINE_STRONG_A, RADIUS_MD),
                shadow: iced::Shadow {
                    color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.32),
                    offset: iced::Vector::new(0.0, 4.0),
                    blur_radius: 16.0,
                },
                ..Default::default()
            });
        row![Space::new().width(Length::Fill), panel]
            .padding(Padding {
                top: SPACE_MD,
                right: SPACE_MD,
                bottom: 0.0,
                left: 0.0,
            })
            .width(Length::Fill)
            .into()
    }

    fn drain_http_auth(&mut self) -> Task<Msg> {
        let evs: Vec<crate::http_auth::Event> = self
            .engine
            .http_auth_handle()
            .lock()
            .unwrap()
            .drain(..)
            .collect();
        let mut focus = Task::none();
        for ev in evs {
            match ev {
                crate::http_auth::Event::Open(dlg) => {
                    tracing::info!(id = dlg.id, host = %dlg.host, "http-auth prompt");
                    if self.pending_http_auth.is_none() {
                        focus = self.show_http_auth(dlg);
                    }
                }
                crate::http_auth::Event::Reset { ids } => {
                    if self
                        .pending_http_auth
                        .as_ref()
                        .is_some_and(|d| ids.contains(&d.id))
                    {
                        self.pending_http_auth = None;
                        self.http_auth_user.clear();
                        self.http_auth_pass.clear();
                        crate::http_auth::set_open(false);
                    }
                }
            }
        }
        focus
    }

    fn show_http_auth(&mut self, dlg: crate::http_auth::Ipc) -> Task<Msg> {
        crate::http_auth::set_open(true);
        self.http_auth_user = dlg.username.clone();
        self.http_auth_pass.clear();
        if dlg.tab_id != 0 && self.cached_active.0 != dlg.tab_id {
            if self.cached_tabs.iter().any(|t| t.id.0 == dlg.tab_id) {
                self.switch_active_tab(TabId(dlg.tab_id));
            }
        }
        self.pending_http_auth = Some(dlg);
        if self.http_auth_user.is_empty() {
            iced::widget::operation::focus(crate::http_auth::user_input_id())
        } else {
            Task::batch([
                iced::widget::operation::focus(crate::http_auth::pass_input_id()),
                iced::advanced::widget::operate(
                    iced::advanced::widget::operation::text_input::select_all::<Msg>(
                        crate::http_auth::pass_input_id(),
                    ),
                ),
            ])
        }
    }

    fn resolve_http_auth(&mut self, success: bool) -> Task<Msg> {
        let Some(dlg) = self.pending_http_auth.take() else {
            crate::http_auth::set_open(false);
            return Task::none();
        };
        let username = std::mem::take(&mut self.http_auth_user);
        let password = std::mem::take(&mut self.http_auth_pass);
        let _ = self.cmd_tx.send(Cmd::HttpAuth {
            id: dlg.id,
            success,
            username,
            password,
        });
        crate::http_auth::set_open(false);
        Task::none()
    }

    fn view_http_auth(&self) -> Element<'_, Msg> {
        let Some(dlg) = self.pending_http_auth.as_ref() else {
            return Space::new()
                .width(Length::Shrink)
                .height(Length::Shrink)
                .into();
        };
        crate::http_auth::overlay(
            dlg,
            &self.http_auth_user,
            &self.http_auth_pass,
            Msg::HttpAuthOk,
            Msg::HttpAuthCancel,
            Msg::HttpAuthUser,
            Msg::HttpAuthPass,
            Msg::HttpAuthFocusPass,
        )
    }

    fn view_js_dialog(&self) -> Element<'_, Msg> {
        let Some(dlg) = self.pending_js_dialog.as_ref() else {
            return Space::new()
                .width(Length::Shrink)
                .height(Length::Shrink)
                .into();
        };
        crate::js_dialog::overlay(
            dlg,
            &self.js_dialog_prompt,
            Msg::JsDialogOk,
            Msg::JsDialogCancel,
            Msg::JsDialogInput,
        )
    }

    fn on_media_ipc(&mut self, m: crate::media::IpcMedia) {
        let profile = self.profile_of(TabId(m.tab_id));
        let known = crate::media::permission_for(&profile, &m.origin);
        if known != "default" {
            crate::media::send_resolve(&self.cmd_tx, &m, known == "granted");
            return;
        }
        if let Some(pending) = self.pending_media.as_mut() {
            if crate::notify::canon_origin(&pending.origin)
                == crate::notify::canon_origin(&m.origin)
            {
                crate::media::merge(pending, &m);
                return;
            }
            crate::media::send_resolve(&self.cmd_tx, &m, false);
            return;
        }
        tracing::info!(
            origin = %m.origin,
            audio = m.audio,
            video = m.video,
            "media permission request"
        );
        self.pending_media = Some(m);
    }

    fn resolve_media_permission(&mut self, result: &str) {
        let Some(perm) = self.pending_media.take() else {
            return;
        };
        let profile = self.profile_of(TabId(perm.tab_id));
        if let Err(e) = crate::media::set_permission(&profile, &perm.origin, result) {
            tracing::warn!(error = %e, "media: persist permission failed");
        }
        crate::media::send_resolve(&self.cmd_tx, &perm, result == "granted");
    }

    fn view_media_permission(&self) -> Element<'_, Msg> {
        use sola_kit::components::style::{SPACE_MD, SPACE_SM};

        let Some(perm) = self.pending_media.as_ref() else {
            return Space::new()
                .width(Length::Shrink)
                .height(Length::Shrink)
                .into();
        };
        let (title_s, hint_s) = crate::media::copy(perm);
        let title = text(title_s).size(15).font(sola_kit::fonts::ui_medium());
        let hint = text(hint_s).size(12).style(|theme: &iced::Theme| {
            let t = theme.extended_palette().background.base.text;
            iced::widget::text::Style {
                color: Some(iced::Color { a: 0.72, ..t }),
            }
        });
        let actions = row![
            kit_button::labeled("Allow", kit_button::primary).on_press(Msg::MediaAllow),
            kit_button::labeled("Block", kit_button::ghost).on_press(Msg::MediaBlock),
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center);
        let body = column![title, hint, actions]
            .spacing(SPACE_SM)
            .width(Length::Fixed(300.0));
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
        .on_press(Msg::MediaBlock);
        let centered = container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center);
        stack![backdrop, centered].into()
    }

    fn resolve_notify_permission(&mut self, result: &str) {
        let Some(perm) = self.pending_notify.take() else {
            return;
        };
        let profile = self.profile_of(TabId(perm.tab_id));
        if let Err(e) = crate::notify::set_permission(&profile, &perm.origin, result) {
            tracing::warn!(error = %e, "notify: persist permission failed");
        }
        let granted = result == "granted";
        let _ = self.cmd_tx.send(Cmd::NotifyPermission {
            prompt_id: perm.req_id,
            granted,
        });
        let _ = self.cmd_tx.send(Cmd::Target {
            tab: TabId(perm.tab_id),
            cmd: Box::new(Cmd::EvaluateJs(crate::notify::resolve_script(
                perm.req_id,
                result,
            ))),
        });
    }

    fn view_notify_permission(&self) -> Element<'_, Msg> {
        use sola_kit::components::style::{SPACE_MD, SPACE_SM};

        let Some(perm) = self.pending_notify.as_ref() else {
            return Space::new()
                .width(Length::Shrink)
                .height(Length::Shrink)
                .into();
        };
        let host = crate::notify::host_of(&perm.origin);
        let title = text("Notifications")
            .size(15)
            .font(sola_kit::fonts::ui_medium());
        let hint = text(format!("{host} wants to show notifications."))
            .size(12)
            .style(|theme: &iced::Theme| {
                let t = theme.extended_palette().background.base.text;
                iced::widget::text::Style {
                    color: Some(iced::Color { a: 0.72, ..t }),
                }
            });
        let actions = row![
            kit_button::labeled("Allow", kit_button::primary).on_press(Msg::NotifyAllow),
            kit_button::labeled("Block", kit_button::ghost).on_press(Msg::NotifyBlock),
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center);
        let body = column![title, hint, actions]
            .spacing(SPACE_SM)
            .width(Length::Fixed(300.0));
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
        .on_press(Msg::NotifyBlock);
        let centered = container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center);
        stack![backdrop, centered].into()
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
                    DownloadStatus::Failed => {
                        if e.error.is_empty() {
                            "Failed".into()
                        } else {
                            format!("Failed · {}", e.error)
                        }
                    }
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
                scroll::scrollable(list)
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
        let active = self.active_handle.clone();
        let mut subs = vec![
            crate::run::frame_subscription_slots::<E>(frames, self.windows.slots(), active),
            iced::window::events().map(|(id, e)| Msg::PageWindow(id, e)),
            sola_kit::app::bus_subscription().map(Msg::Bus),
            sola_kit::call_subscription().map(Msg::Call),
            chrome_drain_subscription(),
            if self.file_dialog.is_some() {
                sola_kit::components::FilePicker::keys().map(Msg::FileDialog)
            } else {
                Subscription::none()
            },
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
                        // typing. Only a divider drag needs samples.
                        if DIVIDER_DRAGGING.load(Ordering::Relaxed) {
                            Some(Msg::CursorMoved(position.x, position.y))
                        } else {
                            None
                        }
                    }
                    // A left press anywhere: resolve whether it focused the URL bar
                    // (for click-to-select-all). Received regardless of which widget
                    // captures it, unlike a wrapping `mouse_area`.
                    Event::Window(iced::window::Event::Resized(size)) => {
                        Some(Msg::WindowResized(size.height))
                    }
                    Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                        Some(Msg::LeftPressed)
                    }
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                        Some(Msg::CursorReleased)
                    }
                    // Escape stops loading (browser-standard). Only when iced did
                    // not already capture the key (e.g. a focused text field that
                    // wants Escape for its own cancel).
                    Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                        if crate::input::is_reopen_closed_shortcut(&key, modifiers) =>
                    {
                        Some(Msg::ReopenClosedTab)
                    }
                    Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                        if crate::input::is_hard_reload_shortcut(&key, modifiers) =>
                    {
                        Some(Msg::NavHardReload)
                    }
                    Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                        if crate::input::chrome_nav_shortcut(&key, modifiers) == Some('r') =>
                    {
                        Some(Msg::NavReloadOrStop)
                    }
                    Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                        if crate::input::chrome_nav_shortcut(&key, modifiers) == Some('g') =>
                    {
                        Some(Msg::FindNext)
                    }
                    Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                        if crate::input::chrome_nav_shortcut(&key, modifiers) == Some('f') =>
                    {
                        Some(Msg::FindOpen)
                    }
                    Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                        if crate::input::is_find_prev_shortcut(&key, modifiers) =>
                    {
                        Some(Msg::FindPrev)
                    }
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(keyboard::key::Named::Escape),
                        ..
                    }) if crate::wallpaper::is_open() => Some(Msg::Wallpaper(wallpaper::Event::Cancel)),
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(keyboard::key::Named::Enter),
                        ..
                    }) if crate::wallpaper::is_open() => Some(Msg::Wallpaper(wallpaper::Event::Use)),
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(keyboard::key::Named::Escape),
                        ..
                    }) if crate::http_auth::is_open() => Some(Msg::HttpAuthCancel),
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(keyboard::key::Named::Escape),
                        ..
                    }) if crate::js_dialog::is_open() => Some(if crate::js_dialog::is_alert() {
                        Msg::JsDialogOk
                    } else {
                        Msg::JsDialogCancel
                    }),
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(keyboard::key::Named::Enter),
                        ..
                    }) if crate::js_dialog::is_open() && !crate::js_dialog::is_prompt() => {
                        Some(Msg::JsDialogOk)
                    }
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        modified_key,
                        modifiers,
                        ..
                    }) if URL_BAR_FOCUSED.load(Ordering::Relaxed)
                        && !crate::js_dialog::is_open()
                        && !crate::http_auth::is_open() =>
                    {
                        // `modified_key` (not `key`): iced's unmodified `key` for
                        // numpad digits is Home/End/arrows, which would steal
                        // history-tray nav while NumLock is typing a number.
                        omnibox_key(&modified_key, &modifiers)
                    }
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(keyboard::key::Named::Escape),
                        ..
                    }) if !crate::file_dialog::is_open()
                        && (FIND_OPEN.load(Ordering::Relaxed)
                            || status == event::Status::Ignored) =>
                    {
                        Some(Msg::NavStop)
                    }
                    #[cfg(feature = "vault")]
                    Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                        if vault::keys_open() =>
                    {
                        vault::panel_key(&key, modifiers, status).map(Msg::Vault)
                    }
                    _ => None,
                }
            }),
        ];
        if self.needs_chrome_poll_timer() {
            subs.push(iced::time::every(Duration::from_millis(250)).map(|_| Msg::Tick));
        }
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

impl<E: Engine> App<E> {
    /// 250 ms `Tick` only while a short-lived UI needs a clock (not idle).
    fn needs_chrome_poll_timer(&self) -> bool {
        if self.copy_url_flash.is_some() {
            return true;
        }
        #[cfg(feature = "vault")]
        if self.vault.needs_clock() {
            return true;
        }
        false
    }
}

/// Divider drag: `listen_with` is a fn pointer and cannot close over App.
static DIVIDER_DRAGGING: AtomicBool = AtomicBool::new(false);
static FIND_OPEN: AtomicBool = AtomicBool::new(false);
static URL_BAR_FOCUSED: AtomicBool = AtomicBool::new(false);

fn omnibox_key(key: &keyboard::Key, modifiers: &keyboard::Modifiers) -> Option<Msg> {
    use keyboard::key::Named;
    if modifiers.logo() {
        return None;
    }
    match key {
        keyboard::Key::Named(Named::ArrowDown) => Some(Msg::OmniboxSelectNext),
        keyboard::Key::Named(Named::ArrowUp) => Some(Msg::OmniboxSelectPrev),
        keyboard::Key::Named(Named::Enter) if modifiers.shift() => Some(Msg::UrlSearchResults),
        keyboard::Key::Named(Named::Enter) => Some(Msg::UrlSubmit),
        keyboard::Key::Character(c)
            if modifiers.control() && !modifiers.alt() && c.eq_ignore_ascii_case("n") =>
        {
            Some(Msg::OmniboxSelectNext)
        }
        keyboard::Key::Character(c)
            if modifiers.control() && !modifiers.alt() && c.eq_ignore_ascii_case("p") =>
        {
            Some(Msg::OmniboxSelectPrev)
        }
        _ => None,
    }
}

fn chrome_drain_subscription() -> Subscription<Msg> {
    Subscription::run(chrome_drain_stream)
}

fn chrome_drain_stream() -> impl iced::futures::Stream<Item = Msg> {
    use iced::futures::StreamExt;
    let (tx, rx) = iced::futures::channel::mpsc::unbounded();
    crate::chrome_wake::install_tx(tx);
    rx.map(|()| Msg::Tick)
}

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

fn last_cursor_point() -> iced::Point {
    iced::Point::new(
        f32::from_bits(CURSOR_X_BITS.load(Ordering::Relaxed)),
        f32::from_bits(CURSOR_Y_BITS.load(Ordering::Relaxed)),
    )
}

/// A toolbar button whose panel is open, or whose work is under way: a
/// subtle accent wash.
fn tool_btn_open(
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

fn nav_icon_copy() -> iced::widget::svg::Handle {
    static H: OnceLock<iced::widget::svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/copy")).clone()
}

fn nav_icon_copy_done() -> iced::widget::svg::Handle {
    static H: OnceLock<iced::widget::svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/copy-check")).clone()
}

fn nav_icon_find_prev() -> iced::widget::svg::Handle {
    static H: OnceLock<iced::widget::svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/chevron-up")).clone()
}

fn nav_icon_find_next() -> iced::widget::svg::Handle {
    static H: OnceLock<iced::widget::svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/chevron-down")).clone()
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

/// Fresh ⌘T tab: blank URL and no real session history.
fn tab_is_scratch_blank(t: &TabInfo) -> bool {
    is_transient_nav_url(&t.url) && t.history.iter().all(|e| is_transient_nav_url(&e.url))
}

/// Open tab already on exactly `url` (a picked history row focuses it
/// rather than loading the page twice). `None` means load it here.
fn tab_showing_url<'a, I>(tabs: I, url: &str) -> Option<TabId>
where
    I: IntoIterator<Item = (TabId, &'a str)>,
{
    let want = crate::util::normalize_url(url);
    tabs.into_iter()
        .find(|(_, tab_url)| crate::util::normalize_url(tab_url) == want)
        .map(|(id, _)| id)
}

/// Visit-history URL for Enter. `None` unless the user highlighted a row
/// after the current text (`armed`). The first search hit is not implied.
fn chosen_history_url<'a, I>(hits: I, selected: Option<usize>, armed: bool) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    if !armed {
        return None;
    }
    let hits: Vec<&str> = hits.into_iter().collect();
    selected
        .and_then(|index| hits.get(index).copied())
        .filter(|url| !url.is_empty())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingNav {
    previous: String,
    requested: String,
}

/// URL to keep showing while `pending` is in flight.
/// `None` means the engine has arrived or gone elsewhere — show its URL.
fn held_navigation_url(engine_url: &str, pending: &PendingNav) -> Option<String> {
    if engine_url == pending.requested {
        return None;
    }
    if is_transient_nav_url(engine_url) || engine_url == pending.previous {
        Some(pending.requested.clone())
    } else {
        None
    }
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

/// Engine-created browsers (`window.open` / NEW_POPUP) appear in the live
/// snapshot with ids chrome did not mint. Move each beside `after` and
/// return those ids.
fn place_engine_popup_tabs(
    tabs: &mut Vec<TabInfo>,
    prev_ids: &HashSet<TabId>,
    after: TabId,
) -> Vec<TabId> {
    let new: Vec<TabId> = tabs
        .iter()
        .map(|t| t.id)
        .filter(|id| !prev_ids.contains(id))
        .collect();
    let mut after = after;
    for id in &new {
        if let Some(pos) = tabs.iter().position(|t| t.id == *id) {
            let info = tabs.remove(pos);
            insert_beside(tabs, after, info);
            after = *id;
        }
    }
    new
}

/// Put `info` just after page `after` (at the end when it is gone).
fn insert_beside(tabs: &mut Vec<TabInfo>, after: TabId, info: TabInfo) {
    match tabs.iter().position(|t| t.id == after) {
        Some(pos) => tabs.insert(pos + 1, info),
        None => tabs.push(info),
    }
}

/// Chrome owns which tabs exist and their order. Engine owns field updates
/// (url / title / loading). Engine-only ids (popups) are appended unless
/// chrome already closed them.
fn merge_tab_snapshot(prev: &[TabInfo], live: &[TabInfo], closed: &HashSet<TabId>) -> Vec<TabInfo> {
    let mut out: Vec<TabInfo> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for p in prev {
        if closed.contains(&p.id) || !seen.insert(p.id) {
            continue;
        }
        out.push(match live.iter().find(|t| t.id == p.id) {
            Some(t) => merge_tab_fields(p, t),
            None => p.clone(),
        });
    }
    for t in live {
        if closed.contains(&t.id) || !seen.insert(t.id) {
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
        profile: if live.profile.is_empty() {
            prior.profile.clone()
        } else {
            live.profile.clone()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: u64, url: &str, title: &str) -> TabInfo {
        TabInfo::chrome(TabId(id), url, title)
    }

    #[test]
    fn picked_row_focuses_only_an_exact_open_page() {
        let tabs = [
            (TabId(1), "about:blank"),
            (TabId(2), "https://github.com/sola/app"),
        ];
        assert_eq!(
            tab_showing_url(tabs, "https://github.com/sola/app"),
            Some(TabId(2))
        );
        assert_eq!(tab_showing_url(tabs, "https://github.com/other"), None);
    }

    #[test]
    fn scratch_blank_is_unused_new_tab() {
        assert!(tab_is_scratch_blank(&tab(1, BLANK_URL, "New Tab")));
        assert!(!tab_is_scratch_blank(&tab(1, "https://example.com/", "Ex")));
        let mut used = tab(1, BLANK_URL, "New Tab");
        used.history = vec![crate::engine::HistoryEntry {
            index: 0,
            url: "https://example.com/".into(),
            title: "Ex".into(),
        }];
        assert!(!tab_is_scratch_blank(&used));
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
    fn engine_popup_sits_beside_the_opener() {
        let mut tabs = vec![
            tab(1, "https://a.example/", "A"),
            tab(2, "https://b.example/", "B"),
            tab(9, "https://popup.example/", "Popup"),
        ];
        let prev = HashSet::from([TabId(1), TabId(2)]);
        let new = place_engine_popup_tabs(&mut tabs, &prev, TabId(1));
        assert_eq!(new, vec![TabId(9)]);
        assert_eq!(
            tabs.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![TabId(1), TabId(9), TabId(2)]
        );
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

    #[test]
    fn enter_ignores_the_first_history_hit_until_a_row_is_chosen() {
        let hits = [
            "https://exitgroup.dev/login/magic",
            "https://exitgroup.dev/home",
        ];
        let typed = "https://exitgroup.dev/admin";
        assert_eq!(chosen_history_url(hits, None, false), None);
        assert_eq!(chosen_history_url(hits, Some(0), false), None);
        assert_eq!(
            crate::util::omnibox_submit_url(typed, chosen_history_url(hits, Some(0), false), false),
            typed
        );
        assert_eq!(
            chosen_history_url(hits, Some(0), true),
            Some("https://exitgroup.dev/login/magic")
        );
        assert_eq!(
            crate::util::omnibox_submit_url(typed, chosen_history_url(hits, Some(1), true), false),
            "https://exitgroup.dev/home"
        );
    }

    #[test]
    fn submitted_url_stays_until_the_engine_leaves_the_previous_page() {
        let pending = PendingNav {
            previous: "https://exitgroup.dev/login/magic".into(),
            requested: "https://exitgroup.dev/admin".into(),
        };
        assert_eq!(
            held_navigation_url("https://exitgroup.dev/login/magic", &pending).as_deref(),
            Some("https://exitgroup.dev/admin")
        );
        assert_eq!(
            held_navigation_url("about:blank", &pending).as_deref(),
            Some("https://exitgroup.dev/admin")
        );
        assert_eq!(
            held_navigation_url("https://exitgroup.dev/admin", &pending),
            None
        );
        assert_eq!(
            held_navigation_url("https://exitgroup.dev/login/magic?next=1", &pending),
            None
        );
    }
}
