use std::collections::HashSet;
use std::sync::Arc;

use iced::widget::{canvas, container, row, text};
use iced::{Element, Length, Subscription, Task, Theme};
use iced::keyboard;

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

/// Compute the pane area (the canvas the terminal grid occupies) from the
/// window size and the sidebar width.
///
/// The sidebar takes `sidebar_w` logical pixels on the left. The rest is the
/// terminal pane — full height. Clamps to zero so the value is always safe to
/// pass to [`term_view::cols_rows_for`], which does its own PAD subtraction.
///
/// This is a pure function so it can be tested headlessly without an iced
/// runtime (Task 2.6).
pub(crate) fn pane_size(window: iced::Size, sidebar_w: f32) -> iced::Size {
    let w = (window.width - sidebar_w).max(0.0);
    iced::Size::new(w, window.height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use term_view::CellMetrics;

    #[test]
    fn pane_size_subtracts_sidebar() {
        let window = iced::Size::new(800.0, 480.0);
        let pane = pane_size(window, 200.0);
        assert_eq!(pane.width, 600.0);
        assert_eq!(pane.height, 480.0);
    }

    #[test]
    fn pane_size_clamps_to_zero() {
        // sidebar wider than window → width clamps to 0, not negative.
        let window = iced::Size::new(100.0, 480.0);
        let pane = pane_size(window, 200.0);
        assert_eq!(pane.width, 0.0);
    }

    #[test]
    fn pane_to_grid_end_to_end() {
        // 800×480 window, 200px sidebar, CellMetrics default (9×18 cells, PAD=6).
        // pane width  = 600 px
        // usable_w    = 600 - 12 = 588 → floor(588/9)  = 65 cols
        // usable_h    = 480 - 12 = 468 → floor(468/18) = 26 rows
        let window = iced::Size::new(800.0, 480.0);
        let pane = pane_size(window, 200.0);
        let (cols, rows) = term_view::cols_rows_for(pane, CellMetrics::default());
        assert_eq!(cols, 65);
        assert_eq!(rows, 26);
    }
}

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
    /// Cached canvas geometry for the active tab's grid. Cleared on PtyOutput
    /// so the next `view` re-renders from the live Term (Task 2.5).
    term_cache: canvas::Cache,
    /// Colour table for the renderer. Hardcoded dark theme for now; Task 4.4
    /// will drive it from `self.theme` (the bus theme).
    palette: term_view::Palette,
    /// Last known window size (logical pixels). Initialised to a sane default;
    /// updated on every `Msg::Resized`. Used by Task 2.6 resize plumbing.
    window_size: iced::Size,
    /// Cell metrics for the active font. Default 9×18 until font negotiation
    /// lands (Task 4.x).
    metrics: term_view::CellMetrics,
    /// Current terminal grid dimensions (cols × rows). Derived from
    /// `window_size`, sidebar width and `metrics`. Initialised to
    /// DEFAULT_COLS×DEFAULT_ROWS; updated on every resize event.
    grid: (u16, u16),
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
            term_cache: canvas::Cache::default(),
            palette: term_view::Palette::default(),
            window_size: iced::Size::new(800.0, 480.0),
            metrics: term_view::CellMetrics::default(),
            grid: (DEFAULT_COLS, DEFAULT_ROWS),
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
            // Task 2.6: drive Msg::Resized whenever the window changes size.
            // iced 0.14 API: `resize_events() -> Subscription<(Id, Size)>`.
            iced::window::resize_events().map(|(_id, size)| Msg::Resized(size)),
        ])
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => self.on_bus(&m),
            Msg::NewTab => self.new_tab(),
            Msg::CloseTab(id) => self.close_tab(&id),
            // Shell exited (reader hit EOF) — tear the tab down like a close.
            Msg::PtyExit(id) => self.close_tab(&id),
            // New grid content: invalidate the cached geometry so the next
            // `view` re-renders from the live Term. (Clearing on any tab's
            // output is the simplest correct choice — only the active tab's
            // grid is on screen, so a stray clear just costs one redraw.)
            Msg::PtyOutput(_id) => {
                self.term_cache.clear();
                Task::none()
            }
            Msg::Input(event) => self.on_input(event),
            Msg::Resized(size) => self.on_resized(size),
            // All remaining arms are Phase 2+ stubs.
            _ => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        // The pane: the live terminal canvas for the active tab, or a
        // placeholder when no tab is attached yet.
        let pane: Element<'_, Msg> = match self
            .active
            .as_deref()
            .and_then(|id| self.tabs.runtime(id))
        {
            Some(rt) => {
                let view = term_view::TermView {
                    term: rt.emulator.term(),
                    cache: &self.term_cache,
                    palette: &self.palette,
                    metrics: term_view::CellMetrics::default(),
                };
                canvas(view)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            }
            None => container(text("terminal pane (placeholder)"))
                .padding(8)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        };

        row![
            sidebar::view(&self.sidebar, &self.tabs, self.active.as_deref(), &self.config),
            pane,
        ]
        .into()
    }

    /// Route a raw iced event. Only keyboard presses are handled here: they
    /// encode to PTY bytes and write to the active tab's backend. Mouse and
    /// window events fall through (mouse → selection is Task 4.1).
    fn on_input(&mut self, event: iced::Event) -> Task<Msg> {
        let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            text,
            ..
        }) = event
        else {
            return Task::none();
        };

        let Some(active) = self.active.clone() else {
            return Task::none();
        };
        let Some(rt) = self.tabs.runtime(&active) else {
            return Task::none();
        };

        // Read the term mode once (drops the lock immediately) — encode_key is
        // mode-aware (DECCKM picks ESC O vs ESC [ for arrows).
        let mode = { *rt.emulator.term().lock().mode() };

        let mods = input::Mods::from(modifiers);

        // Exactly one source of bytes, in priority order:
        //   1. encode_key — named keys + Ctrl-letter on Character keys.
        //   2. encode_char — Character keys (incl. Ctrl+symbol that encode_key
        //      deliberately returns None for).
        //   3. the platform `text` field — IME / printable that neither caught.
        let bytes = input::encode_key(&key, mods, mode).or_else(|| {
            if let keyboard::Key::Character(s) = &key {
                s.chars().next().and_then(|c| input::encode_char(c, mods))
            } else {
                None
            }
        });
        let bytes = bytes.or_else(|| {
            text.as_ref()
                .filter(|t| !t.is_empty())
                .map(|t| t.as_bytes().to_vec())
        });

        if let Some(bytes) = bytes {
            rt.backend.write(&bytes);
        }
        Task::none()
    }

    /// Handle a window resize event (Task 2.6).
    ///
    /// Recomputes the terminal grid from the new window size + current sidebar
    /// width, then drives the new dimensions into every live tab's emulator
    /// and PTY backend. All tabs are resized, not just the active one, so
    /// switching tabs never reveals a mis-sized grid.
    fn on_resized(&mut self, size: iced::Size) -> Task<Msg> {
        self.window_size = size;

        let sidebar_w = if self.config.sidebar_collapsed {
            36.0_f32
        } else {
            self.config.sidebar_width as f32
        };
        let pane = pane_size(size, sidebar_w);
        let (cols, rows) = term_view::cols_rows_for(pane, self.metrics);

        if (cols, rows) == self.grid {
            return Task::none(); // no change — avoid churn
        }
        self.grid = (cols, rows);

        for rt in self.tabs.runtimes_mut() {
            rt.emulator.resize(cols, rows);
            rt.backend.resize(cols, rows);
            // `PtyManager::resize` does TIOCSWINSZ + tmux::resize_window but
            // does NOT send SIGWINCH. Signal the child process group so TUIs
            // (vim, htop, …) redraw at the new size.
            rt.backend.sigwinch();
        }

        // Geometry changed — invalidate the cached canvas so the next frame
        // repaints at the correct grid size.
        self.term_cache.clear();
        Task::none()
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
    /// Grid size is taken from `self.grid` (updated by Task 2.6 resize
    /// plumbing). Scrollback authority (OPEN QUESTION #2): the
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

        // Use the current grid size so tabs attached after a resize come up at
        // the right dimensions (Task 2.6). Falls back to DEFAULT_COLS/ROWS on
        // first attach before the first Msg::Resized fires.
        let (cols, rows) = self.grid;

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
