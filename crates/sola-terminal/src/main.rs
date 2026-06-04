use std::collections::HashSet;
use std::sync::Arc;

use iced::widget::{container, row, text};
use iced::{Element, Length, Subscription, Task, Theme};

use sola_bus::topics::{TerminalConfig, Topic, TopicKind};
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

const APP_ID: &str = "sola-terminal";

/// Default grid until the renderer reports a real pane size (Task 2.6).
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

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
            emulator::output_subscription().map(Msg::PtyOutput),
            emulator::exit_subscription().map(Msg::PtyExit),
            iced::event::listen().map(Msg::Input),
        ])
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => self.on_bus(&m),
            Msg::NewTab => self.new_tab(),
            Msg::CloseTab(id) => self.close_tab(&id),
            // Shell exited (reader hit EOF) — tear the tab down like a close.
            Msg::PtyExit(id) => self.close_tab(&id),
            // Task 2.5: the renderer reads `renderable_content` here. For now a
            // redraw is implicit in iced, so just acknowledge.
            Msg::PtyOutput(_id) => Task::none(),
            // All remaining arms are Phase 2+ stubs.
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
            if let Err(e) = client.emit(Topic::SetAppMenu(menu::terminal_menu(self.tabs.len()))) {
                tracing::warn!("republish_menu failed: {e:?}");
            }
        }
    }

    /// Open (or reattach) the PTY for `id`, seed tmux scrollback into the grid,
    /// and start the reader thread.
    ///
    /// Default grid is 80×24 until the renderer reports a real pane size (Task
    /// 2.6 wires resize). Scrollback authority (OPEN QUESTION #2): the
    /// alacritty `Grid` is the live viewport + local history; tmux is the
    /// persistence layer. The captured scrollback is a ONE-SHOT seed fed before
    /// the reader thread starts, so reattach shows history without racing live
    /// output. It is not re-synced afterward — the grid is authoritative once
    /// live.
    fn attach_tab(&mut self, id: &str) -> Task<Msg> {
        let Some(meta) = self.tabs.get(id).cloned() else {
            tracing::warn!(id = %id, "attach_tab: no TabMeta");
            return Task::none();
        };

        let (cols, rows) = (DEFAULT_COLS, DEFAULT_ROWS);

        let listener = emulator::Listener::new(
            id.to_string(),
            pty::pty_write_sender(),
            emulator::notify_sender(),
        );
        let em = emulator::Emulator::new(cols, rows, listener);
        let term = em.term();

        // Seed tmux scrollback into the grid BEFORE the reader thread starts,
        // so history shows on reattach without racing live output. Drive a
        // one-shot Processor over the shared term handle.
        match tmux::capture_scrollback(&meta.tmux_session) {
            Ok(text) if !text.trim().is_empty() => {
                // tmux capture-pane emits LF-only lines; normalise to CRLF so
                // the parser starts each line at column 0.
                let seed = text.replace('\n', "\r\n");
                let mut processor: alacritty_terminal::vte::ansi::Processor<
                    alacritty_terminal::vte::ansi::StdSyncHandler,
                > = alacritty_terminal::vte::ansi::Processor::new();
                let mut t = term.lock();
                processor.advance(&mut *t, seed.as_bytes());
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(id = %id, "scrollback capture failed: {e}"),
        }

        let backend = match pty::PtyBackend::spawn_or_attach(
            id,
            &meta.tmux_session,
            cols,
            rows,
            meta.cwd.as_deref(),
            term,
            emulator::notify_sender(),
            emulator::exit_sender(),
        ) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(id = %id, "spawn_or_attach failed: {e}");
                return Task::none();
            }
        };

        self.tabs.insert_runtime(
            id.to_string(),
            state::TabRuntime { emulator: em, backend },
        );
        Task::none()
    }

    /// Mint a new tab: fresh uuid, tmux session named after it, cwd inherited
    /// from the active tab, ordinal = max+1. Persist on the bus and attach.
    fn new_tab(&mut self) -> Task<Msg> {
        let id = uuid::Uuid::new_v4().to_string();
        let tmux_session = tmux::session_name(&id);

        let active_cwd = self.active.as_deref().and_then(|a| self.tabs.cwd_of(a));
        let cwd = state::inherit_cwd(active_cwd.as_deref());

        let ordinals: Vec<u32> = self
            .tabs
            .ordered_meta()
            .iter()
            .map(|m| m.ordinal)
            .collect();
        let ordinal = state::next_ordinal(&ordinals);

        self.tabs.upsert_meta(state::TabMeta {
            id: id.clone(),
            tmux_session: tmux_session.clone(),
            cwd: cwd.clone(),
            ordinal,
        });
        self.active = Some(id.clone());

        // Persist on the bus (sticky TerminalSession slot keyed by id).
        if let Ok(mut client) = bus().lock() {
            let _ = client.emit(Topic::TerminalSession(sola_bus::topics::TerminalSession {
                id: id.clone(),
                tmux_session,
                cwd,
                ordinal,
            }));
        }

        self.republish_menu();
        self.attach_tab(&id)
    }

    /// Close a tab: tear down its PTY backend (kills the tmux session),
    /// retract the persisted slot, drop the runtime, and pick a new active
    /// tab. Reached from the close button, a menu action, and PtyExit.
    fn close_tab(&mut self, id: &str) -> Task<Msg> {
        // Explicit close kills tmux (plain drop would preserve it).
        if let Some(rt) = self.tabs.runtime(id) {
            rt.backend.close();
        }

        // Choose the next active tab from the order BEFORE removal.
        if self.active.as_deref() == Some(id) {
            let order = self.tabs.ids_in_order();
            self.active = state::next_active_after_close(&order, id);
        }

        // Retract the persisted slot (sticky=false removes it from the store).
        if let Some(meta) = self.tabs.get(id).cloned() {
            if let Ok(mut client) = bus().lock() {
                let _ = client.retract(Topic::TerminalSession(
                    sola_bus::topics::TerminalSession {
                        id: meta.id,
                        tmux_session: meta.tmux_session,
                        cwd: meta.cwd,
                        ordinal: meta.ordinal,
                    },
                ));
            }
        }

        self.tabs.remove(id);
        self.republish_menu();
        Task::none()
    }

    /// Stub: Phase 3 will handle copy/paste/new-tab/close-tab actions.
    fn on_menu_action(&mut self, _action: &str) -> Task<Msg> {
        Task::none()
    }
}
