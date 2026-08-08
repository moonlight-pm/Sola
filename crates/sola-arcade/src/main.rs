//! sola-arcade — Steam library gallery + windowed-gamescope launcher.
//!
//! Discovers installed titles from Steam manifests (no Settings catalog).
//! Launch: `gamescope -W/-H -- steam -applaunch <id>` (never host `-f`).
mod launch;
mod steam;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{column, container, image, row, scrollable, stack, text, Space};
use iced::widget::operation;
use iced::widget::scrollable::{AbsoluteOffset, Viewport};
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
use sola_kit::components::style::{
    RADIUS_LG, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS, hairline,
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
use steam::{SteamGame, scan_installed_games};

const APP_ID: &str = "sola-arcade";
/// Wayland app_id gamescope reports (after river pid inference when empty).
const GAMESCOPE_HOST_APP_ID: &str = "gamescope";
/// Full-width banner row height. Steam `library_hero` is 1920×620; we show a
/// wide strip (Cover) so the hero fills the row without portrait cropping.
const ROW_H: f32 = 168.0;

fn gallery_scroll_id() -> ScrollId {
    ScrollId::new("arcade-gallery")
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
    host_width: u32,
    host_height: u32,
    status: Option<String>,
    status_tone: StatusTone,
    gamescope_ok: bool,
    steam_ok: bool,
    active: Option<ActiveSession>,
    /// Gallery list scroll (absolute Y). Restored after launch so Play→Stop
    /// row rebuild does not jump the list to the top.
    scroll_y: f32,
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
        Self {
            games: scan_installed_games(),
            filter: String::new(),
            host_width: DEFAULT_HOST_WIDTH,
            host_height: DEFAULT_HOST_HEIGHT,
            status: None,
            status_tone: StatusTone::Info,
            gamescope_ok,
            steam_ok,
            active: None,
            scroll_y: 0.0,
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
        }
    }
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    Filter(String),
    Refresh,
    Launch(u32),
    StopGame,
    OpenStore(u32),
    Uninstall(u32),
    Tick,
    GalleryScrolled(AbsoluteOffset),
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
        (app, sola_kit::window_ready_task(Msg::WindowReady))
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
        } else if self.games.is_empty() {
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
        if q.is_empty() {
            return self.games.iter().collect();
        }
        self.games
            .iter()
            .filter(|g| {
                g.name.to_ascii_lowercase().contains(&q) || g.app_id.to_string().contains(&q)
            })
            .collect()
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
        // Row UI swaps Play→Stop; re-apply scroll so the list does not jump.
        let y = self.scroll_y;
        operation::scroll_to(
            gallery_scroll_id(),
            AbsoluteOffset {
                x: None,
                y: Some(y),
            },
        )
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
        let y = self.scroll_y;
        operation::scroll_to(
            gallery_scroll_id(),
            AbsoluteOffset {
                x: None,
                y: Some(y),
            },
        )
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
            Msg::Filter(s) => self.filter = s,
            Msg::Refresh => {
                self.games = scan_installed_games();
                self.gamescope_ok = sola_core::applications::command_exists("gamescope");
                self.steam_ok = sola_core::applications::command_exists("steam");
                if self.active.is_none() {
                    self.set_boot_status();
                }
            }
            Msg::Launch(id) => return self.launch_game(id),
            Msg::StopGame => return self.stop_game(),
            Msg::GalleryScrolled(off) => {
                self.scroll_y = off.y;
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
        // Header is only a chunky search field.
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

        // Status strip: prepare/info + problems (no top “playing” banner).
        let status: Element<'_, Msg> = match &self.status {
            Some(s)
                if matches!(
                    self.status_tone,
                    StatusTone::Danger | StatusTone::Warn | StatusTone::Info
                ) =>
            {
                let style = match self.status_tone {
                    StatusTone::Warn => kit_text::warning,
                    StatusTone::Danger => kit_text::danger,
                    _ => kit_text::muted,
                };
                kit_text::caption(s.as_str()).style(style).into()
            }
            _ => Space::new().height(0).into(),
        };

        let gallery: Element<'_, Msg> = {
            let filtered = self.filtered();
            if filtered.is_empty() {
                container(
                    kit_text::body(if self.games.is_empty() {
                        "No games found. Install titles in Steam, then Refresh (Meta+R)."
                    } else {
                        "No matches for this search."
                    })
                    .style(kit_text::muted),
                )
                .padding(SPACE_XL)
                .center_x(Length::Fill)
                .width(Length::Fill)
                .into()
            } else {
                let active_id = self.active.as_ref().map(|a| a.steam_app_id);
                let mut list = column![].spacing(SPACE_MD).width(Length::Fill);
                for g in filtered {
                    list = list.push(game_row(g, active_id));
                }
                scrollable(
                    container(list)
                        .width(Length::Fill)
                        .padding(Padding {
                            top: SPACE_SM,
                            right: SPACE_XS,
                            bottom: SPACE_XL,
                            left: 0.0,
                        }),
                )
                .id(gallery_scroll_id())
                .on_scroll(|vp: Viewport| Msg::GalleryScrolled(vp.absolute_offset()))
                .height(Length::Fill)
                .width(Length::Fill)
                .into()
            }
        };

        let content = column![search, status, gallery]
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
fn game_row(g: &SteamGame, active_id: Option<u32>) -> Element<'_, Msg> {
    let banner: Element<'_, Msg> = match g.banner_path() {
        Some(path) => image(ImageHandle::from_path(path))
            .width(Length::Fill)
            .height(Length::Fixed(ROW_H))
            .content_fit(iced::ContentFit::Cover)
            .into(),
        None => container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(ROW_H))
            .style(row_placeholder_style)
            .into(),
    };

    let title = text(g.name.as_str())
        .font(fonts::display())
        .size(26)
        .style(|theme: &Theme| {
            let p = theme.extended_palette();
            iced::widget::text::Style {
                color: Some(p.background.base.text),
            }
        });

    let primary = if active_id == Some(g.app_id) {
        kit_btn::labeled("Stop", kit_btn::danger).on_press(Msg::StopGame)
    } else if active_id.is_some() {
        // Another title is active — no on_press.
        kit_btn::labeled("Play", kit_btn::secondary)
    } else {
        kit_btn::labeled("Play", kit_btn::primary).on_press(Msg::Launch(g.app_id))
    };

    // Ghost labels fail on light heroes. Impeccable/Operate: put secondary
    // actions on a controlled dark chip so contrast is stable over any art.
    let actions = container(
        row![
            primary,
            kit_btn::labeled_sm("Store", kit_btn::secondary).on_press(Msg::OpenStore(g.app_id)),
            kit_btn::labeled_sm("Uninstall", kit_btn::secondary)
                .on_press(Msg::Uninstall(g.app_id)),
        ]
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

    let layers = stack![
        container(banner)
            .width(Length::Fill)
            .height(Length::Fixed(ROW_H))
            .clip(true),
        // Fade banner so large type stays readable.
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(ROW_H))
            .style(row_scrim_style),
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

#[allow(dead_code)]
fn _cover_path_typecheck(p: PathBuf) -> ImageHandle {
    ImageHandle::from_path(p)
}
