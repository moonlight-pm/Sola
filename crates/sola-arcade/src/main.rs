//! sola-arcade — Steam library gallery + windowed-gamescope launcher.
//!
//! Discovers installed titles from Steam manifests (no Settings catalog).
//! Launch: `gamescope -W/-H -- steam -applaunch <id>` (never host `-f`).
mod launch;
mod steam;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{
    button, column, container, image as iced_image, row, scrollable, stack, text, tooltip, Space,
};
use iced::widget::operation;
use iced::widget::scrollable::{AbsoluteOffset, Viewport};
use iced::widget::tooltip::Position as TooltipPosition;
use iced::widget::Id as ScrollId;
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Subscription, Task, Theme};
use iced::widget::image::Handle as ImageHandle;

use sola_bus::Message;
use sola_bus::topics::{
    AppMenuPayload, Application, LaunchAppPayload, MenuDefinition, Topic, TopicKind,
};
use sola_core::KeyCode;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus, bus_subscription, is_self_quit, startup,
    window_settings_transparent,
};
use sola_kit::components::icon;
use sola_kit::components::style::{
    RADIUS_LG, RADIUS_MD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS, hairline,
};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::{self as kit_input, text_input};
use sola_kit::components::button as kit_btn;
use sola_kit::components::card as kit_card;
use sola_kit::fonts;
use sola_kit::theme::default_theme;

use launch::{
    DEFAULT_HOST_HEIGHT, DEFAULT_HOST_WIDTH, game_session_app_id, launch_command, session_alive,
    stop_nest_local,
};
use steam::{
    SortMode, SteamGame, load_library_cache, save_library_cache, scan_library_games, sort_games,
};

const APP_ID: &str = "sola-arcade";
/// Wayland app_id gamescope reports (after river pid inference when empty).
const GAMESCOPE_HOST_APP_ID: &str = "gamescope";
/// Full-width banner row height. Steam `library_hero` is 1920×620; we show a
/// wide strip (Cover) so the hero fills the row without portrait cropping.
const ROW_H: f32 = 168.0;
/// Vertical gap between gallery rows (`column` spacing).
const ROW_GAP: f32 = SPACE_MD;
/// Decode height for cached banners (2× row for HiDPI; width from aspect).
const BANNER_DECODE_H: u32 = 336;
/// Extra rows above/below the viewport to decode early (smooth scroll).
const BANNER_OVERSCAN_ROWS: f32 = 2.0;
/// Fallback viewport height until the first scrollable measure arrives.
const DEFAULT_VIEWPORT_H: f32 = 900.0;

fn gallery_scroll_id() -> ScrollId {
    ScrollId::new("arcade-gallery")
}

/// Fixed status slot height so the gallery scrollable is always the same
/// child index/type in the column (avoids iced tree rematch resetting scroll).
const STATUS_SLOT_H: f32 = 22.0;

fn restore_gallery_scroll(y: f32) -> Task<Msg> {
    // Apply twice: once immediately, once after the next view pass. Launch
    // rebuilds row content (Play→Stop); a single op can race the rematch.
    Task::batch([
        operation::scroll_to(
            gallery_scroll_id(),
            AbsoluteOffset {
                x: None,
                y: Some(y),
            },
        ),
        Task::done(Msg::RestoreScroll),
    ])
}

fn main() -> iced::Result {
    // Game-runner path used by sola-session (`LaunchApp` whitespace-splits):
    //   sola-arcade --run <steam_app_id> [width] [height]
    // gamescope child (desktop Steam, no BPM):
    //   sola-arcade --nested-steam <steam_app_id>
    let mut argv = std::env::args().skip(1).peekable();
    if argv.peek().map(|s| s.as_str()) == Some("--nested-steam") {
        argv.next();
        let app_id: u32 = argv
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| {
                eprintln!("sola-arcade: --nested-steam requires <steam_app_id>");
                std::process::exit(2);
            });
        launch::run_nested_steam_blocking(app_id);
    }
    if let Some((app_id, w, h)) = parse_run_args(argv) {
        launch::run_game_blocking(app_id, w, h);
    }

    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu(
            "Arcade",
            [
                ("refresh", "Refresh Library", KeyCode::R.meta()),
                ("stop-game", "Stop Game", KeyCode::S.meta_shift()),
                ("quit", "Quit Arcade", KeyCode::Q.meta()),
            ],
        )
        .install();

    let app = iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings_transparent(APP_ID));
    app.run()
}

/// Parse `--run <appid> [width] [height]`. Returns `None` for UI mode.
fn parse_run_args<I, S>(mut args: I) -> Option<(u32, u32, u32)>
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    let first = args.next()?;
    if first.as_ref() != "--run" {
        return None;
    }
    let app_id: u32 = args.next()?.as_ref().parse().ok()?;
    let width = args
        .next()
        .and_then(|s| s.as_ref().parse().ok())
        .unwrap_or(DEFAULT_HOST_WIDTH);
    let height = args
        .next()
        .and_then(|s| s.as_ref().parse().ok())
        .unwrap_or(DEFAULT_HOST_HEIGHT);
    Some((app_id, width, height))
}

/// Active play/load session tracked by the Arcade UI.
#[derive(Debug, Clone)]
struct ActiveSession {
    steam_app_id: u32,
    name: String,
    session_id: String,
    /// True after we observe a live process / LaunchResult ok.
    running: bool,
    started: Instant,
}

struct App {
    games: Vec<SteamGame>,
    filter: String,
    sort: SortMode,
    /// When true (default), only fully-installed / ready-to-play titles —
    /// Steam-style “Ready to Play” filter.
    ready_to_play_only: bool,
    host_width: u32,
    host_height: u32,
    status: Option<String>,
    status_tone: StatusTone,
    gamescope_ok: bool,
    steam_ok: bool,
    active: Option<ActiveSession>,
    /// True while a background Steam library scan is running.
    library_scanning: bool,
    /// False until the first successful cache load or completed scan.
    /// Used to show “initial scan” copy instead of a blank window.
    has_library_data: bool,
    /// Gallery list scroll (absolute Y). Restored after launch so Play→Stop
    /// row rebuild does not jump the list to the top.
    scroll_y: f32,
    /// Measured scrollable viewport height (content clip). Used for lazy
    /// banner decode of only on-screen rows.
    viewport_h: f32,
    /// Decoded banner handles (app_id → RGBA). Filled lazily as rows enter
    /// the viewport (plus a small overscan), never all-at-once on boot.
    banners: HashMap<u32, ImageHandle>,
    /// Banner decode currently in flight (avoid duplicate jobs).
    banner_inflight: HashSet<u32>,
    /// Tried decode and got nothing (missing file / bad image) — do not retry.
    banner_missing: HashSet<u32>,
    theme: Theme,
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusTone {
    Info,
    Warn,
    Danger,
    Ok,
}

impl Default for App {
    fn default() -> Self {
        let gamescope_ok = sola_core::applications::command_exists("gamescope");
        let steam_ok = sola_core::applications::command_exists("steam");
        // Instant open: never block the UI thread on a full Steam walk.
        // Cache (if any) paints immediately; a background scan always runs.
        let cached = load_library_cache();
        let has_library_data = cached.is_some();
        let games = cached.unwrap_or_default();
        Self {
            games,
            filter: String::new(),
            sort: SortMode::Alphabetical,
            ready_to_play_only: true,
            host_width: DEFAULT_HOST_WIDTH,
            host_height: DEFAULT_HOST_HEIGHT,
            status: None,
            status_tone: StatusTone::Info,
            gamescope_ok,
            steam_ok,
            active: None,
            library_scanning: true,
            has_library_data,
            scroll_y: 0.0,
            viewport_h: DEFAULT_VIEWPORT_H,
            banners: HashMap::new(),
            banner_inflight: HashSet::new(),
            banner_missing: HashSet::new(),
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
        }
    }
}

/// One lazy banner decode batch: which app_ids were requested, which loaded.
#[derive(Debug, Clone)]
struct BannerBatch {
    attempted: Vec<u32>,
    loaded: HashMap<u32, ImageHandle>,
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    Filter(String),
    SetSort(SortMode),
    ToggleReadyToPlayOnly,
    Refresh,
    Launch(u32),
    Install(u32),
    StopGame,
    OpenStore(u32),
    Uninstall(u32),
    Tick,
    GalleryScrolled(Viewport),
    /// Re-apply [`App::scroll_y`] after a view rebuild (launch/stop).
    RestoreScroll,
    /// Lazy banner decode finished for a viewport slice.
    BannersReady(BannerBatch),
    /// Background Steam library scan finished (or failed empty).
    LibraryScanned(Vec<SteamGame>),
    WindowReady(Option<iced::window::Id>),
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
}

impl App {
    fn boot() -> (Self, Task<Msg>) {
        let mut app = Self::default();
        // Reattach UI lock if a nest survived a previous Arcade process.
        app.recover_active_from_procs();
        app.set_boot_status();
        // Decode only the first viewport page of whatever we already have
        // (cache). Full Steam walk always runs in the background.
        let banners = app.ensure_visible_banners();
        (
            app,
            Task::batch([
                sola_kit::window_ready_task(Msg::WindowReady),
                banners,
                scan_library_task(),
            ]),
        )
    }

    /// Kick off decode for banners whose rows intersect the current viewport
    /// (plus overscan). No-op when nothing new is needed.
    fn ensure_visible_banners(&mut self) -> Task<Msg> {
        let candidates: Vec<(u32, Option<PathBuf>)> = {
            let filtered = self.filtered();
            let n = filtered.len();
            let (start, end) = visible_row_range(n, self.scroll_y, self.viewport_h);
            filtered
                .into_iter()
                .take(end)
                .skip(start)
                .map(|g| {
                    // Resolve banner path on demand (scan defers FS walks).
                    let path = g
                        .banner
                        .clone()
                        .or_else(|| steam::banner_art_path(g.app_id));
                    (g.app_id, path)
                })
                .collect()
        };
        // Remember resolved paths on the game records so we don't re-walk.
        for (id, path) in &candidates {
            if let Some(p) = path {
                if let Some(g) = self.games.iter_mut().find(|g| g.app_id == *id) {
                    if g.banner.is_none() {
                        g.banner = Some(p.clone());
                    }
                }
            }
        }
        let mut jobs: Vec<(u32, PathBuf)> = Vec::new();
        for (id, path) in candidates {
            if self.banners.contains_key(&id)
                || self.banner_inflight.contains(&id)
                || self.banner_missing.contains(&id)
            {
                continue;
            }
            let Some(path) = path else {
                self.banner_missing.insert(id);
                continue;
            };
            self.banner_inflight.insert(id);
            jobs.push((id, path));
        }
        decode_banners_task(jobs)
    }

    fn apply_scanned_library(&mut self, games: Vec<SteamGame>) -> Task<Msg> {
        self.library_scanning = false;
        self.has_library_data = true;
        self.games = games;
        self.apply_sort();
        save_library_cache(&self.games);
        // Keep decoded banners for app_ids that still exist; drop orphans.
        let live: HashSet<u32> = self.games.iter().map(|g| g.app_id).collect();
        self.banners.retain(|id, _| live.contains(id));
        self.banner_inflight.retain(|id| live.contains(id));
        self.banner_missing.retain(|id| live.contains(id));
        if self.active.is_none() {
            self.set_boot_status();
        } else if self
            .status
            .as_deref()
            .is_some_and(|s| s.contains("Scanning") || s.contains("Refreshing"))
        {
            self.status = None;
        }
        self.ensure_visible_banners()
    }

    /// If `sola-arcade --run <id>` / gamescope applaunch is already live,
    /// restore Stop-on-row state without re-launching.
    fn recover_active_from_procs(&mut self) {
        for g in &self.games {
            if session_alive(g.app_id) {
                let name = g.name.clone();
                let steam_app_id = g.app_id;
                self.active = Some(ActiveSession {
                    steam_app_id,
                    name: name.clone(),
                    session_id: game_session_app_id(steam_app_id),
                    running: true,
                    started: Instant::now(),
                });
                // Re-publish host label so menubar/switcher show the game name.
                publish_gamescope_host_label(steam_app_id, &name);
                return;
            }
        }
    }

    /// Publish (or clear) the gamescope host label used by shell chrome.
    fn publish_host_label_for_active(&self) {
        if let Some(a) = &self.active {
            publish_gamescope_host_label(a.steam_app_id, &a.name);
        }
    }

    fn clear_host_label(&self) {
        retract_gamescope_host_label();
    }

    fn title(&self) -> String {
        "Arcade".into()
    }

    fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.is_floating_any(), &self.theme)
    }

    fn set_boot_status(&mut self) {
        if self.active.is_some() {
            return;
        }
        // First cold start: no cache yet — tell the user while the background
        // scan runs so the window never looks frozen/empty without reason.
        if self.library_scanning && !self.has_library_data {
            self.status = Some(
                "Scanning Steam library for the first time… this can take a little while."
                    .into(),
            );
            self.status_tone = StatusTone::Info;
            return;
        }
        // Quiet by default — only surface problems in the status strip.
        if !self.steam_ok {
            self.status = Some("steam not found on PATH — install Steam to launch titles.".into());
            self.status_tone = StatusTone::Danger;
        } else if !self.gamescope_ok {
            self.status = Some(
                "gamescope not on PATH — launches fall back to bare steam -applaunch (no host nest)."
                    .into(),
            );
            self.status_tone = StatusTone::Warn;
        } else if self.has_library_data && !self.games.iter().any(|g| g.installed) {
            self.status =
                Some("No installed Steam games found (checked default library paths).".into());
            self.status_tone = StatusTone::Warn;
        } else {
            self.status = None;
            self.status_tone = StatusTone::Ok;
        }
    }

    fn filtered(&self) -> Vec<&SteamGame> {
        let q = self.filter.trim().to_ascii_lowercase();
        self.games
            .iter()
            .filter(|g| {
                if self.ready_to_play_only && !g.installed {
                    return false;
                }
                if q.is_empty() {
                    return true;
                }
                g.name.to_ascii_lowercase().contains(&q) || g.app_id.to_string().contains(&q)
            })
            .collect()
    }

    fn apply_sort(&mut self) {
        sort_games(&mut self.games, self.sort);
    }

    fn launch_game(&mut self, steam_app_id: u32) -> Task<Msg> {
        if let Some(active) = &self.active {
            self.status = Some(format!(
                "Already {} “{}” — Stop before starting another.",
                if active.running { "playing" } else { "loading" },
                active.name
            ));
            self.status_tone = StatusTone::Warn;
            return Task::none();
        }

        let Some(game) = self.games.iter().find(|g| g.app_id == steam_app_id) else {
            self.status = Some(format!("Unknown app id {steam_app_id}"));
            self.status_tone = StatusTone::Danger;
            return Task::none();
        };
        if !game.installed {
            self.status = Some(format!(
                "“{}” is not installed — use Install first.",
                game.name
            ));
            self.status_tone = StatusTone::Warn;
            return Task::none();
        }
        let name = game.name.clone();
        let plan = launch_command(steam_app_id, self.host_width, self.host_height);
        let session_id = game_session_app_id(steam_app_id);

        if let Ok(mut b) = bus().lock() {
            let _ = b.emit(Topic::LaunchApp(LaunchAppPayload {
                app_id: session_id.clone(),
                command: plan.command.clone(),
            }));
        }

        // Lock UI immediately — row shows Stop; other Plays disabled.
        // `running` becomes true on LaunchResult ok (or process probe).
        self.active = Some(ActiveSession {
            steam_app_id,
            name: name.clone(),
            session_id: session_id.clone(),
            running: false,
            started: Instant::now(),
        });
        // Shell menubar/switcher key off gamescope's host app_id — publish
        // the game title as Application label + app-menu name.
        publish_gamescope_host_label(steam_app_id, &name);
        // Steam cold-start under the nest may show prepare UI (shader cache,
        // updates) inside gamescope before the game process starts — that is
        // intentional and automatic (no separate Steam session required).
        self.status = Some(format!(
            "Starting “{name}”… Steam may prepare shaders/updates in the nest first."
        ));
        self.status_tone = StatusTone::Info;
        tracing::info!(
            %session_id,
            steam_app_id,
            command = %plan.command,
            gamescope = plan.gamescope,
            "arcade launch"
        );
        // Row UI swaps Play→Stop; keep gallery scroll position.
        restore_gallery_scroll(self.scroll_y)
    }

    fn stop_game(&mut self) -> Task<Msg> {
        let Some(active) = self.active.take() else {
            return Task::none();
        };
        if let Ok(mut b) = bus().lock() {
            let _ = b.emit(Topic::CloseApp(active.session_id.clone()));
        }
        stop_nest_local(active.steam_app_id);
        retract_gamescope_host_label();
        self.status = None;
        self.set_boot_status();
        restore_gallery_scroll(self.scroll_y)
    }

    fn on_bus_topic(&mut self, topic: Topic) {
        match topic {
            Topic::LaunchResult(r)
                if self.active.as_ref().is_some_and(|a| a.session_id == r.app_id) =>
            {
                if r.ok {
                    if let Some(a) = self.active.as_mut() {
                        a.running = true;
                    }
                    self.status = None;
                    self.publish_host_label_for_active();
                } else {
                    let err = r.error.unwrap_or_else(|| "spawn failed".into());
                    self.status = Some(format!("Launch failed: {err}"));
                    self.status_tone = StatusTone::Danger;
                    self.active = None;
                    self.clear_host_label();
                }
            }
            Topic::UserAppExited(e)
                if self
                    .active
                    .as_ref()
                    .is_some_and(|a| a.session_id == e.app_id) =>
            {
                // `--run` can exit early on bare applaunch while the game
                // still lives under Steam/gamescope — only clear when dead.
                let steam_id = self.active.as_ref().map(|a| a.steam_app_id);
                if steam_id.is_some_and(session_alive) {
                    if let Some(a) = self.active.as_mut() {
                        a.running = true;
                    }
                    self.publish_host_label_for_active();
                    return;
                }
                self.active = None;
                self.clear_host_label();
                self.status = None;
                self.set_boot_status();
            }
            _ => {}
        }
    }

    /// Drop active session if the nest/process is gone (poll).
    fn reconcile_active(&mut self) {
        let Some(active) = &self.active else {
            return;
        };
        if session_alive(active.steam_app_id) {
            if let Some(a) = self.active.as_mut() {
                a.running = true;
            }
            return;
        }
        // Grace for cold spawn + Steam prepare (shader cache / updates can
        // take minutes on first launch of a Proton title). After that, no
        // process ⇒ clear Stop state.
        let grace = Duration::from_secs(180);
        if !active.running && active.started.elapsed() < grace {
            return;
        }
        self.active = None;
        self.clear_host_label();
        self.status = None;
        self.set_boot_status();
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(message) => {
                self.float.update(&message);
                apply_theme_update(&message, &mut self.theme);
                if is_self_quit(&message, APP_ID) {
                    return iced::exit();
                }
                if let Some(topic) = Topic::parse(&message) {
                    if let Topic::MenuAction(a) = &topic {
                        if a.app_id == APP_ID {
                            match a.action_id.as_str() {
                                "refresh" => return self.update(Msg::Refresh),
                                "stop-game" => return self.update(Msg::StopGame),
                                "quit" => {
                                    sola_kit::close_app(APP_ID);
                                }
                                _ => {}
                            }
                        }
                    }
                    self.on_bus_topic(topic);
                }
            }
            Msg::Filter(s) => {
                self.filter = s;
                return self.ensure_visible_banners();
            }
            Msg::SetSort(mode) => {
                self.sort = mode;
                self.apply_sort();
                return self.ensure_visible_banners();
            }
            Msg::ToggleReadyToPlayOnly => {
                self.ready_to_play_only = !self.ready_to_play_only;
                return self.ensure_visible_banners();
            }
            Msg::Refresh => {
                if self.library_scanning {
                    self.status = Some("Library scan already in progress…".into());
                    self.status_tone = StatusTone::Info;
                    return Task::none();
                }
                self.library_scanning = true;
                self.gamescope_ok = sola_core::applications::command_exists("gamescope");
                self.steam_ok = sola_core::applications::command_exists("steam");
                if self.active.is_none() {
                    self.status = Some(if self.has_library_data {
                        "Refreshing Steam library…".into()
                    } else {
                        "Scanning Steam library for the first time… this can take a little while."
                            .into()
                    });
                    self.status_tone = StatusTone::Info;
                }
                return scan_library_task();
            }
            Msg::LibraryScanned(games) => {
                return self.apply_scanned_library(games);
            }
            Msg::Launch(id) => return self.launch_game(id),
            Msg::Install(id) => {
                // Hand off to Steam's install UI (never download ourselves).
                let uri = format!("steam://install/{id}");
                let _ = std::process::Command::new("steam").arg(&uri).spawn();
                let name = self
                    .games
                    .iter()
                    .find(|g| g.app_id == id)
                    .map(|g| g.name.as_str())
                    .unwrap_or("game");
                self.status = Some(format!("Opened Steam install for “{name}”."));
                self.status_tone = StatusTone::Info;
            }
            Msg::StopGame => return self.stop_game(),
            Msg::GalleryScrolled(vp) => {
                self.scroll_y = vp.absolute_offset().y;
                let h = vp.bounds().height;
                if h > 1.0 {
                    self.viewport_h = h;
                }
                return self.ensure_visible_banners();
            }
            Msg::RestoreScroll => {
                return operation::scroll_to(
                    gallery_scroll_id(),
                    AbsoluteOffset {
                        x: None,
                        y: Some(self.scroll_y),
                    },
                );
            }
            Msg::BannersReady(batch) => {
                for id in &batch.attempted {
                    self.banner_inflight.remove(id);
                    if !batch.loaded.contains_key(id) {
                        self.banner_missing.insert(*id);
                    }
                }
                self.banners.extend(batch.loaded);
                // After a batch lands, pull the next overscan slice if needed.
                return self.ensure_visible_banners();
            }
            Msg::OpenStore(id) => {
                let url = format!("https://store.steampowered.com/app/{id}");
                sola_core::open_url_logged(&url);
            }
            Msg::Uninstall(id) => {
                // Hand off to Steam's uninstall UI (never delete files ourselves).
                let uri = format!("steam://uninstall/{id}");
                let _ = std::process::Command::new("steam")
                    .arg(&uri)
                    .spawn();
                self.status = Some(format!("Opened Steam uninstall for app {id}."));
                self.status_tone = StatusTone::Info;
            }
            Msg::Tick => {
                self.reconcile_active();
            }
            Msg::WindowReady(id) => self.window_id = id,
            Msg::TitleDrag => return sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => return sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => sola_kit::close_app(APP_ID),
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Msg> {
        // Header: search field with icon tool buttons on the same row.
        let search = text_input("Search games…", &self.filter)
            .on_input(Msg::Filter)
            .size(18)
            .padding(Padding {
                top: 16.0,
                right: 20.0,
                bottom: 16.0,
                left: 20.0,
            })
            .style(kit_input::style)
            .width(Length::Fill);

        let sort_alpha = icon_tool_btn(
            "lucide/arrow-down-a-z",
            "Sort A–Z",
            self.sort == SortMode::Alphabetical,
            Msg::SetSort(SortMode::Alphabetical),
        );
        let sort_recent = icon_tool_btn(
            "lucide/history",
            "Sort by recent activity",
            self.sort == SortMode::Recency,
            Msg::SetSort(SortMode::Recency),
        );
        let ready_only = icon_tool_btn(
            "lucide/circle-check",
            if self.ready_to_play_only {
                "Ready to play only (on) — click to show uninstalled"
            } else {
                "Ready to play only (off) — showing uninstalled too"
            },
            self.ready_to_play_only,
            Msg::ToggleReadyToPlayOnly,
        );

        let header = row![search, sort_alpha, sort_recent, ready_only]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center)
            .width(Length::Fill);

        // Status strip: always the same widget type + fixed height so the
        // gallery scrollable stays the same tree slot (Space↔caption rematch
        // was resetting scroll state on launch).
        let status: Element<'_, Msg> = {
            let line = self.status.as_deref().unwrap_or("");
            // Non-breaking space when empty keeps caption layout stable.
            let shown = if line.is_empty() { "\u{00a0}" } else { line };
            let caption = match self.status_tone {
                StatusTone::Danger if !line.is_empty() => {
                    kit_text::caption(shown).style(kit_text::danger)
                }
                StatusTone::Warn if !line.is_empty() => {
                    kit_text::caption(shown).style(kit_text::warning)
                }
                _ => kit_text::caption(shown).style(kit_text::muted),
            };
            container(caption)
                .width(Length::Fill)
                .height(Length::Fixed(STATUS_SLOT_H))
                .into()
        };

        // Always a scrollable with a stable Id — never swap for a plain
        // container (that destroyed scroll state on empty/filter edges too).
        let gallery: Element<'_, Msg> = {
            let filtered = self.filtered();
            let installed_any = self.games.iter().any(|g| g.installed);
            let body: Element<'_, Msg> = if filtered.is_empty() {
                let empty_msg = if self.library_scanning && !self.has_library_data {
                    "Scanning your Steam library for the first time…\nTitles will appear here when the scan finishes."
                } else if self.games.is_empty() {
                    "No games found. Install titles in Steam, then Refresh (Meta+R)."
                } else if !installed_any && self.ready_to_play_only {
                    "No ready-to-play games. Turn off “Ready to play only” or install titles in Steam."
                } else {
                    "No matches for this search."
                };
                container(kit_text::body(empty_msg).style(kit_text::muted))
                    .padding(SPACE_XL)
                    .center_x(Length::Fill)
                    .width(Length::Fill)
                    .into()
            } else {
                let active_id = self.active.as_ref().map(|a| a.steam_app_id);
                let mut list = column![].spacing(ROW_GAP).width(Length::Fill);
                for g in filtered {
                    list = list.push(game_row(g, active_id, self.banners.get(&g.app_id)));
                }
                container(list)
                    .width(Length::Fill)
                    .padding(Padding {
                        top: SPACE_SM,
                        right: SPACE_XS,
                        bottom: SPACE_XL,
                        left: 0.0,
                    })
                    .into()
            };
            scrollable(body)
                .id(gallery_scroll_id())
                .on_scroll(Msg::GalleryScrolled)
                .height(Length::Fill)
                .width(Length::Fill)
                .into()
        };

        let content = column![header, status, gallery]
            .spacing(SPACE_LG)
            .padding(Padding {
                top: SPACE_XL,
                right: SPACE_XL,
                bottom: SPACE_MD,
                left: SPACE_XL,
            })
            .width(Length::Fill)
            .height(Length::Fill);

        sola_kit::wrap_if_floating(
            self.float.is_floating_any(),
            "Arcade",
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            content.into(),
        )
    }

    fn subscription(&self) -> Subscription<Msg> {
        let bus = bus_subscription().map(Msg::Bus);
        if self.active.is_some() {
            // Poll nest lifetime so Stop/Play state stays honest.
            let tick = iced::time::every(Duration::from_secs(1)).map(|_| Msg::Tick);
            return Subscription::batch([bus, tick]);
        }
        bus
    }
}

/// Compact icon tool button with hover tooltip (sort / filter chrome).
fn icon_tool_btn<'a>(
    icon_name: &'static str,
    tip: &'static str,
    active: bool,
    on_press: Msg,
) -> Element<'a, Msg> {
    let style = if active {
        kit_btn::primary
    } else {
        kit_btn::secondary
    };
    let btn = button(icon::icon(icon_name, 18))
        .padding(Padding {
            top: 12.0,
            right: 12.0,
            bottom: 12.0,
            left: 12.0,
        })
        .style(style)
        .on_press(on_press);

    let tip_body = container(text(tip).size(12).font(fonts::ui()))
        .padding(Padding {
            top: SPACE_SM,
            right: SPACE_MD,
            bottom: SPACE_SM,
            left: SPACE_MD,
        })
        .style(|theme: &Theme| {
            let p = theme.extended_palette();
            container::Style {
                background: Some(Background::Color(p.background.strong.color)),
                text_color: Some(p.background.base.text),
                border: Border {
                    color: p.background.stronger.color,
                    width: 1.0,
                    radius: RADIUS_MD.into(),
                },
                ..Default::default()
            }
        });

    tooltip(btn, tip_body, TooltipPosition::Bottom)
        .gap(6)
        .into()
}

/// Tell shell chrome the gamescope host should display `name` (menubar + switcher).
///
/// The nest surface reports `app_id=gamescope` (sometimes empty until river
/// infers from pid). Shell looks up `Application` label and `SetAppMenu`
/// first-menu label for that app_id.
fn publish_gamescope_host_label(steam_app_id: u32, name: &str) {
    let icon = steam::banner_art_path(steam_app_id)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "lucide/gamepad-2".into());
    let app = Application {
        app_id: GAMESCOPE_HOST_APP_ID.into(),
        label: name.to_string(),
        // Not a launcher entry — empty command; shell still uses label/icon
        // for switcher/menubar while the nest is up. Retracted on stop.
        command: String::new(),
        icon,
    };
    let menu = AppMenuPayload {
        app_id: GAMESCOPE_HOST_APP_ID.into(),
        menus: vec![MenuDefinition {
            label: name.to_string(),
            items: vec![],
        }],
    };
    if let Ok(mut b) = bus().lock() {
        let _ = b.emit(Topic::Application(app));
        let _ = b.emit(Topic::SetAppMenu(menu));
    }
    tracing::info!(%name, steam_app_id, "published gamescope host label");
}

fn retract_gamescope_host_label() {
    let app = Application {
        app_id: GAMESCOPE_HOST_APP_ID.into(),
        label: String::new(),
        command: String::new(),
        icon: String::new(),
    };
    let menu = AppMenuPayload {
        app_id: GAMESCOPE_HOST_APP_ID.into(),
        menus: vec![],
    };
    if let Ok(mut b) = bus().lock() {
        let _ = b.retract(Topic::Application(app));
        let _ = b.retract(Topic::SetAppMenu(menu));
    }
    tracing::info!("retracted gamescope host label");
}

/// One full-width row: faded banner background, large title, actions on the right.
///
/// `active_id`: currently launching/playing Steam app. That row shows **Stop**;
/// every other row has Play disabled.
///
/// Uninstalled rows use **Install** instead of Play/Uninstall and a heavier
/// banner fade so they read as “not on disk”.
///
/// `banner`: pre-decoded handle when ready; until then a neutral placeholder
/// (avoids iced's sequential path-decode of full 1920×620 heroes).
fn game_row<'a>(
    g: &'a SteamGame,
    active_id: Option<u32>,
    banner: Option<&'a ImageHandle>,
) -> Element<'a, Msg> {
    let installed = g.installed;
    let banner: Element<'_, Msg> = if let Some(handle) = banner {
        iced_image(handle.clone())
            .width(Length::Fill)
            .height(Length::Fixed(ROW_H))
            .content_fit(iced::ContentFit::Cover)
            .opacity(if installed { 1.0 } else { 0.40 })
            .into()
    } else {
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(ROW_H))
            .style(row_placeholder_style)
            .into()
    };

    let title = text(g.name.as_str())
        .font(fonts::display())
        .size(26)
        .style(move |theme: &Theme| {
            let p = theme.extended_palette();
            let mut c = p.background.base.text;
            if !installed {
                c.a *= 0.72;
            }
            iced::widget::text::Style { color: Some(c) }
        });

    let primary: Element<'_, Msg> = if !installed {
        kit_btn::labeled("Install", kit_btn::primary)
            .on_press(Msg::Install(g.app_id))
            .into()
    } else if active_id == Some(g.app_id) {
        kit_btn::labeled("Stop", kit_btn::danger)
            .on_press(Msg::StopGame)
            .into()
    } else if active_id.is_some() {
        // Another title is active — no on_press.
        kit_btn::labeled("Play", kit_btn::secondary).into()
    } else {
        kit_btn::labeled("Play", kit_btn::primary)
            .on_press(Msg::Launch(g.app_id))
            .into()
    };

    // Ghost labels fail on light heroes. Impeccable/Operate: put secondary
    // actions on a controlled dark chip so contrast is stable over any art.
    let secondary_actions: Element<'_, Msg> = if installed {
        row![
            kit_btn::labeled_sm("Store", kit_btn::secondary).on_press(Msg::OpenStore(g.app_id)),
            kit_btn::labeled_sm("Uninstall", kit_btn::secondary)
                .on_press(Msg::Uninstall(g.app_id)),
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center)
        .into()
    } else {
        kit_btn::labeled_sm("Store", kit_btn::secondary)
            .on_press(Msg::OpenStore(g.app_id))
            .into()
    };

    let actions = container(
        row![primary, secondary_actions]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: SPACE_SM,
        right: SPACE_SM,
        bottom: SPACE_SM,
        left: SPACE_SM,
    })
    .style(action_chip_style);

    let foreground = container(
        row![
            container(title)
                .width(Length::Fill)
                .center_y(Length::Fixed(ROW_H)),
            container(actions).center_y(Length::Fixed(ROW_H)),
        ]
        .spacing(SPACE_LG)
        .padding(Padding {
            top: 0.0,
            right: SPACE_LG,
            bottom: 0.0,
            left: SPACE_XL,
        })
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .height(Length::Fixed(ROW_H)),
    )
    .width(Length::Fill)
    .height(Length::Fixed(ROW_H));

    let scrim = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(ROW_H))
        .style(if installed {
            row_scrim_style
        } else {
            row_scrim_uninstalled_style
        });

    let layers = stack![
        container(banner)
            .width(Length::Fill)
            .height(Length::Fixed(ROW_H))
            .clip(true),
        // Fade banner so large type stays readable (heavier when uninstalled).
        scrim,
        foreground,
    ];

    kit_card::plain(layers)
        .width(Length::Fill)
        .height(Length::Fixed(ROW_H))
        .padding(0)
        .clip(true)
        .into()
}

fn row_scrim_style(theme: &Theme) -> container::Style {
    let _ = theme;
    // Soft wash only — dark heroes still need to read; title uses display type.
    // Actions sit on their own dark chip (see `action_chip_style`), not this scrim.
    container::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.32))),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_LG.into(),
        },
        ..Default::default()
    }
}

/// Uninstalled rows: banner already at 40% opacity; add a stronger wash so
/// the row reads as “not on disk” (half or more of the art is obscured).
fn row_scrim_uninstalled_style(theme: &Theme) -> container::Style {
    let _ = theme;
    container::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.55))),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_LG.into(),
        },
        ..Default::default()
    }
}

/// Always-dark frosted surface under row actions — readable on white heroes.
fn action_chip_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let fill = p.background.base.color;
    container::Style {
        background: Some(Background::Color(Color {
            r: fill.r * 0.35,
            g: fill.g * 0.35,
            b: fill.b * 0.38,
            a: 0.82,
        })),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.10),
            width: 1.0,
            radius: RADIUS_LG.into(),
        },
        ..Default::default()
    }
}

fn row_placeholder_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.strong.color)),
        ..Default::default()
    }
}

#[allow(dead_code)]
fn _hairline_ref(theme: &Theme) {
    let _ = hairline(theme.extended_palette(), RADIUS_LG);
}

/// Inclusive-exclusive index range of rows intersecting the viewport.
fn visible_row_range(n: usize, scroll_y: f32, viewport_h: f32) -> (usize, usize) {
    if n == 0 {
        return (0, 0);
    }
    let stride = ROW_H + ROW_GAP;
    let overscan = BANNER_OVERSCAN_ROWS * stride;
    let view_h = if viewport_h > 1.0 {
        viewport_h
    } else {
        DEFAULT_VIEWPORT_H
    };
    let top = (scroll_y - overscan).max(0.0);
    let bottom = scroll_y + view_h + overscan;
    // List content: padding-top SPACE_SM, then rows with ROW_GAP between.
    let start = ((top - SPACE_SM) / stride).floor().max(0.0) as usize;
    let end = (((bottom - SPACE_SM) / stride).ceil() as usize).saturating_add(1);
    let start = start.min(n);
    let end = end.min(n).max(start);
    (start, end)
}

/// Full Steam library walk off the UI thread (every boot + manual Refresh).
fn scan_library_task() -> Task<Msg> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(scan_library_games)
                .await
                .unwrap_or_else(|e| {
                    tracing::error!(?e, "library scan task join failed");
                    Vec::new()
                })
        },
        Msg::LibraryScanned,
    )
}

/// Decode a small job list off the UI thread (viewport slice only).
fn decode_banners_task(jobs: Vec<(u32, PathBuf)>) -> Task<Msg> {
    if jobs.is_empty() {
        return Task::none();
    }
    let attempted: Vec<u32> = jobs.iter().map(|(id, _)| *id).collect();
    Task::perform(
        async move {
            let loaded = tokio::task::spawn_blocking(move || decode_banners_parallel(jobs))
                .await
                .unwrap_or_default();
            BannerBatch { attempted, loaded }
        },
        Msg::BannersReady,
    )
}

fn decode_banners_parallel(jobs: Vec<(u32, PathBuf)>) -> HashMap<u32, ImageHandle> {
    use std::sync::Mutex;
    let out = Mutex::new(HashMap::with_capacity(jobs.len()));
    std::thread::scope(|scope| {
        for (app_id, path) in jobs {
            let out = &out;
            scope.spawn(move || {
                if let Some(handle) = decode_banner_handle(&path) {
                    out.lock().expect("banner map").insert(app_id, handle);
                }
            });
        }
    });
    out.into_inner().unwrap_or_default()
}

/// Load + downscale a Steam hero so GPU upload is cheap and sync for iced.
fn decode_banner_handle(path: &std::path::Path) -> Option<ImageHandle> {
    let img = image::open(path).ok()?.into_rgba8();
    let (sw, sh) = img.dimensions();
    if sw == 0 || sh == 0 {
        return None;
    }
    let th = BANNER_DECODE_H;
    let tw = ((sw as f64 / sh as f64) * th as f64).round() as u32;
    let tw = tw.clamp(64, 1920);
    let rgba = if sh > th || sw > tw {
        image::imageops::resize(&img, tw, th, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let (w, h) = rgba.dimensions();
    Some(ImageHandle::from_rgba(w, h, rgba.into_raw()))
}
