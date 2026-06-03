use std::collections::HashSet;
use std::sync::Arc;

use iced::widget::{container, row, text};
use iced::{Element, Length, Subscription, Task, Theme};

use sola_bus::topics::{AppMenuPayload, MenuActionPayload, TerminalConfig, TerminalSession, Topic, TopicKind};
use sola_bus::Message;
use sola_kit::app::{BusSetup, apply_theme_update, bus, bus_subscription, is_self_quit, startup, window_settings};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

mod emulator;
mod input;
mod menu;
mod pty;
mod session;
mod sidebar;
mod state;
mod term_view;
mod tmux;

pub const APP_ID: &'static str = "sola-terminal";

fn main() -> iced::Result {
    startup(APP_ID);

    // Bring tmux server up before replaying tabs.
    tmux::cleanup_stale_socket();
    tmux::kill_orphaned_clients();
    tmux::ensure_server_running();
    tmux::reload_config();

    // Connect to bus, subscribe, and install global slot.
    // Terminal has a multi-menu payload so we skip .app_menu() here
    // and publish manually after install (see below).
    BusSetup::new(APP_ID)
        .subscribe(&[
            TopicKind::Theme,
            TopicKind::MenuAction,
            TopicKind::CloseApp,
            TopicKind::TerminalConfig,
            TopicKind::TerminalSession,
        ])
        .install();

    // Publish the full multi-menu payload directly (BusSetup::app_menu
    // only handles a single-menu definition; terminal needs 4 menus).
    if let Ok(mut client) = bus().lock() {
        if let Err(e) = client.emit(Topic::SetAppMenu(menu::terminal_menu(0))) {
            tracing::warn!("initial app-menu publish failed: {e:?}");
        }
    }

    let mut app = iced::application(App::new, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::mono())
        .window(window_settings(APP_ID));
    for bytes in fonts::load_all() {
        app = app.font(bytes);
    }
    app.run()
}

struct App {
    tabs: state::Tabs,
    active: Option<String>,
    config: TerminalConfig,
    /// Snapshot of live tmux sessions at startup, used to retract any
    /// persisted TerminalSession whose tmux peer is gone. None means
    /// the tmux query failed — we admit everything to be safe.
    live_tmux_at_startup: Option<HashSet<String>>,
    theme: Theme,
    sidebar: sidebar::SidebarState,
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    PtyOutput(String),
    PtyExit(String),
    SelectTab(String),
    CloseTab(String),
    NewTab,
    ToggleCollapse,
    SidebarDragStart,
    SidebarDragMove(f32),
    SidebarDragEnd,
    ReorderStart(usize),
    ReorderMove(f32),
    ReorderEnd,
    Input(iced::Event),
    Resized(iced::Size),
    Tick,
}

impl App {
    fn new() -> (Self, Task<Msg>) {
        let live_tmux_at_startup = tmux::list_sessions().map(|v| v.into_iter().collect());
        let app = Self {
            tabs: state::Tabs::default(),
            active: None,
            config: TerminalConfig::default(),
            live_tmux_at_startup,
            theme: default_theme(),
            sidebar: sidebar::SidebarState::default(),
        };
        (app, Task::none())
    }

    fn title(&self) -> String {
        "Terminal".into()
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            emulator::output_subscription().map(|s| Msg::PtyOutput(s)),
            iced::event::listen().map(Msg::Input),
        ])
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => self.on_bus(&m),
            // All other arms are Phase 2+ stubs.
            _ => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        row![
            sidebar::view(&self.sidebar, &self.tabs, self.active.as_deref(), &self.config),
            container(text("terminal pane (placeholder)"))
                .padding(8)
                .width(Length::Fill)
                .height(Length::Fill),
        ]
        .into()
    }

    fn on_bus(&mut self, m: &Message) -> Task<Msg> {
        // 1. Live theme reload.
        if apply_theme_update(m, &mut self.theme) {
            return Task::none();
        }

        // 2. Quit request (CloseApp or our own MenuAction("quit")).
        if is_self_quit(m, APP_ID) {
            return iced::exit();
        }

        // 3. Dispatch by topic.
        match Topic::parse(m) {
            Some(Topic::TerminalConfig(cfg)) => {
                self.config = cfg;
            }
            Some(Topic::TerminalSession(s)) => {
                // Retraction: a sticky topic delivered with sticky=false
                // signals removal of that slot from the bus state store.
                if !m.sticky {
                    self.tabs.remove(&s.id);
                    self.republish_menu();
                    return Task::none();
                }
                // Startup reconciliation: retract if tmux session gone.
                match session::reconcile_admit(&self.live_tmux_at_startup, &s.tmux_session) {
                    session::Admit::Retract => {
                        tracing::info!(
                            id = %s.id,
                            tmux_session = %s.tmux_session,
                            "retracting stale TerminalSession (tmux gone)"
                        );
                        if let Ok(mut client) = bus().lock() {
                            let _ = client.retract(Topic::TerminalSession(s));
                        }
                        return Task::none();
                    }
                    session::Admit::Yes => {
                        let was_empty = self.tabs.is_empty();
                        self.tabs.upsert_meta(state::TabMeta {
                            id: s.id.clone(),
                            tmux_session: s.tmux_session.clone(),
                            cwd: s.cwd.clone(),
                            ordinal: s.ordinal,
                        });
                        if self.active.is_none() {
                            self.active = Some(s.id.clone());
                        }
                        self.republish_menu();
                        if was_empty {
                            return self.attach_tab(&s.id);
                        }
                    }
                }
            }
            Some(Topic::MenuAction(ref p)) if p.app_id == APP_ID => {
                return self.on_menu_action(&p.action_id);
            }
            _ => {}
        }
        Task::none()
    }

    fn republish_menu(&self) {
        if let Ok(mut client) = bus().lock() {
            let _ = client.emit(Topic::SetAppMenu(menu::terminal_menu(self.tabs.len())));
        }
    }

    /// Stub: Phase 2 will open/attach a PTY for this tab.
    fn attach_tab(&mut self, _id: &str) -> Task<Msg> {
        Task::none()
    }

    /// Stub: Phase 3 will handle copy/paste/new-tab/close-tab actions.
    fn on_menu_action(&mut self, _action: &str) -> Task<Msg> {
        Task::none()
    }
}
