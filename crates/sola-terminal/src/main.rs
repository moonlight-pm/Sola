use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Minimum gap between mouse-mode wheel flushes to a single pane. Caps the
/// SGR report rate a TUI sees so a high-rate touchpad doesn't enqueue a
/// full-repaint per event. Intermediate notches are accumulated and drained
/// (up to [`WHEEL_MAX_REPORTS_PER_FLUSH`] per tick) so distance is mostly kept.
const WHEEL_MIN_INTERVAL: Duration = Duration::from_millis(12);
/// Max SGR wheel reports written per flush tick (after accumulation).
const WHEEL_MAX_REPORTS_PER_FLUSH: i32 = 2;

use iced::widget::{canvas, container, mouse_area};
use iced::{Element, Event, Length, Subscription, Task, Theme, window};
use iced::{event, keyboard, mouse};

use sola_bus::Message;
use sola_bus::topics::{MenuActionPayload, PaneLayout, SplitDir, Topic, TopicKind};
use sola_kit::app::{
    BusSetup, apply_theme_update, bus, bus_subscription, is_self_quit, startup,
    window_settings_transparent,
};
use sola_kit::fonts;
use sola_kit::tabs::TabWindows;
use sola_kit::theme::{Atoms, atoms_from_bus_theme, default_theme};

use sola_terminal::{emulator, extkeys, input, links, perf, pty, state, term_view, tmux};

mod instance;
mod menu;

const APP_ID: &str = "sola-terminal";

/// Default grid until a pane reports a real size.
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

/// A tab window's size until iced reports the real one.
const DEFAULT_WINDOW: iced::Size = iced::Size::new(800.0, 480.0);

/// Milliseconds to wait after Enter before querying tmux for the pane's cwd.
const CWD_REFRESH_MS: u64 = 150;

/// Slightly longer initial delay used when attaching a pane for the first time.
const CWD_INITIAL_DELAY_MS: u64 = 300;

/// After subscribing, how long to wait for saved tabs before opening a
/// first one.
const FIRST_TAB_WAIT: Duration = Duration::from_millis(700);

/// Minimum pane extent (logical px) along a split's main axis. Clamps the
/// divider drag so neither side collapses below it.
const MIN_PANE_PX: f32 = 80.0;

/// cwd basename → tab label, falling back to "shell".
pub(crate) fn tab_label(cwd: &Option<String>) -> String {
    match cwd.as_deref() {
        Some("/") => "/".into(),
        Some(p) if !p.is_empty() => p
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("shell")
            .to_string(),
        _ => "shell".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use term_view::CellMetrics;

    #[test]
    fn basename_label() {
        assert_eq!(
            tab_label(&Some("/home/joshua/Workspace".into())),
            "Workspace"
        );
        assert_eq!(tab_label(&Some("/".into())), "/");
        assert_eq!(tab_label(&None), "shell");
    }

    #[test]
    fn window_to_grid_end_to_end() {
        let (cols, rows) = term_view::cols_rows_for(DEFAULT_WINDOW, CellMetrics::default());
        assert!(cols > 0 && rows > 0);
    }
}

fn main() -> iced::Result {
    startup(APP_ID);

    // One Terminal: a second launch asks the running one for a new tab.
    if !instance::claim() {
        BusSetup::new(APP_ID).install();
        if let Ok(mut client) = bus().lock() {
            if let Err(e) = client.emit(Topic::MenuAction(MenuActionPayload {
                app_id: APP_ID.into(),
                action_id: "new_tab".into(),
            })) {
                tracing::warn!("asking the running terminal for a new tab failed: {e:?}");
            }
        }
        return Ok(());
    }

    // Bring tmux server up before replaying tabs.
    tmux::cleanup_stale_socket();
    tmux::kill_orphaned_clients();
    tmux::ensure_server_running();
    tmux::reload_config();

    // Subscribe to chrome topics only here. `TerminalSession` stickies are
    // expanded in `Msg::SubscribeSessions` *after* the iced bus pump is
    // live — sticky replay at BusSetup time races the poller handoff and
    // was dropping every tab on restart (tmux sessions still alive, UI
    // empty).
    BusSetup::new(APP_ID)
        .subscribe(&[TopicKind::Theme, TopicKind::MenuAction, TopicKind::CloseApp])
        .install();

    // Publish the full multi-menu payload directly (BusSetup::app_menu only
    // handles a single-menu definition; terminal needs several menus).
    if let Ok(mut client) = bus().lock() {
        if let Err(e) = client.emit(Topic::SetAppMenu(menu::terminal_menu())) {
            tracing::warn!("initial app-menu publish failed: {e:?}");
        }
    }

    // A window per tab: each is a tab in the shell's tab bar.
    iced::daemon(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::mono())
        .run()
}

struct App {
    tabs: state::Tabs,
    /// Each open tab's window, both ways.
    window_of: HashMap<String, window::Id>,
    tab_of: HashMap<window::Id, String>,
    /// Keys and menu actions go to the tab of the last window focused.
    focused: Option<window::Id>,
    sizes: HashMap<window::Id, iced::Size>,
    /// The windows, announced to the shell's tab bar as tabs.
    tab_windows: TabWindows,
    /// Snapshot of live tmux sessions at startup, used to prune persisted
    /// panes whose tmux peer is gone. None means the query failed — admit all.
    live_tmux_at_startup: Option<HashSet<String>>,
    /// Sticky `TerminalSession` replay is only for our own last run.
    /// After this instant, an unknown id is not ours to restore.
    startup_replay_until: Instant,
    theme: Theme,
    palette: term_view::Palette,
    metrics: term_view::CellMetrics,
    /// Runtime-only title cache: PaneId → most-recent OSC 0/2 title.
    titles: HashMap<String, String>,
    /// Block-cursor blink phase.
    cursor_on: bool,
    /// SplitId currently being dragged via its divider, if any.
    dragging_split: Option<String>,
    /// Last applied grid per PaneId — lets the resize fan-out skip panes whose
    /// dimensions didn't change (avoids TIOCSWINSZ churn during a divider drag).
    pane_grids: HashMap<String, (u16, u16)>,
    /// Latest keyboard modifiers from `ModifiersChanged`.
    keyboard_mods: keyboard::Modifiers,
    /// Modifier keys held, tracked from KeyPressed/KeyReleased of the modifier
    /// keys themselves (and their physical codes). On this Wayland stack the
    /// Enter press's modifier mask and ModifiersChanged often lack SHIFT even
    /// while Shift is held (keydebug: Shift+Enter → plain CR; Alt+Enter works).
    /// Tracking the Shift key down/up is the reliable signal.
    keys_held_mods: keyboard::Modifiers,
    /// Which windows float (CSD titlebar while floating).
    float: sola_kit::FloatState,
    /// Per-pane mouse-mode wheel accumulator. High-rate touchpad events are
    /// folded here and flushed at [`WHEEL_MIN_INTERVAL`] so TUIs don't full-
    /// repaint on every iced wheel sample.
    wheel_burst: HashMap<String, WheelBurst>,
}

/// Accumulated mouse-mode wheel reports awaiting a paced flush.
#[derive(Debug, Default)]
struct WheelBurst {
    /// Signed pending report count: positive = wheel-up, negative = wheel-down.
    pending: i32,
    /// Last encoded report (carries col/row/SGR vs X10). Direction is adjusted
    /// at flush time from `pending`'s sign.
    sample: Vec<u8>,
    /// When we last wrote wheel bytes for this pane.
    last_flush: Option<Instant>,
    /// A `FlushWheel` task is already scheduled for this pane.
    scheduled: bool,
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    /// New output on a pane's PTY (carries the PaneId).
    PtyOutput(String),
    /// A pane's shell exited (EOF) — carries the PaneId.
    PtyExit(String),
    Noop,
    /// Press on a pane split divider (carries the SplitId).
    SplitDividerPress(String),
    /// Pointer entered a pane's area (focus-follows-mouse) — carries PaneId.
    PaneFocused(String),
    /// Global cursor sample in a window (x, y). Drives the split-divider drag.
    CursorMoved(window::Id, f32, f32),
    /// Global left-button release. Ends the split-divider drag.
    CursorReleased,
    Input(window::Id, iced::Event),
    /// A tab window opened, resized, gained focus, or was asked to close.
    Window(window::Id, window::Event),
    SelectionChanged,
    /// Local scrollback moved on this pane (PaneId).
    Scrolled(String),
    /// Plain left-click on a URL; open it in sola-browser.
    OpenUrl(String),
    /// Mouse-wheel bytes destined for a pane's PTY (PaneId, encoded report),
    /// emitted when a mouse-tracking app owns the wheel. Accumulated and
    /// rate-limited before enqueue on the write queue.
    WheelToPty(String, Vec<u8>),
    /// Drain a pane's wheel accumulator (scheduled after a throttle gap).
    FlushWheel(String),
    Pasted(Option<String>),
    /// OSC 0/2 title for a pane (PaneId, title).
    Title(String, String),
    /// Result of an async tmux cwd query (PaneId, path).
    CwdResult(String, Option<String>),
    BlinkTick,
    /// Expand bus subscription to TerminalSession so sticky tab replay lands
    /// after the iced bus pump is attached.
    SubscribeSessions,
    /// No saved tab came back: open a first one.
    EnsureTab,
    TitleDrag(window::Id),
    TitleResize(window::Id, window::Direction),
    TitleClose(window::Id),
}

/// Full topic set once the iced bus pump can receive sticky replay.
const SESSION_TOPICS: &[TopicKind] = &[
    TopicKind::Theme,
    TopicKind::MenuAction,
    TopicKind::CloseApp,
    TopicKind::Windows,
    TopicKind::WindowFloating,
    TopicKind::TabWindow,
    TopicKind::TerminalSession,
];

impl App {
    fn boot() -> (Self, Task<Msg>) {
        let listed = tmux::list_sessions();
        match &listed {
            Some(s) if s.is_empty() => tracing::info!(
                "no live sola-* tmux sessions at boot (tmux was lost); rebuilding saved tabs"
            ),
            Some(s) => tracing::info!(count = s.len(), "live sola-* tmux sessions at boot"),
            None => tracing::warn!("tmux list-sessions failed at boot; admitting all stickies"),
        }
        let live_tmux_at_startup = state::boot_snapshot(listed);
        let app = Self {
            tabs: state::Tabs::default(),
            window_of: HashMap::new(),
            tab_of: HashMap::new(),
            focused: None,
            sizes: HashMap::new(),
            tab_windows: TabWindows::new(APP_ID),
            live_tmux_at_startup,
            startup_replay_until: Instant::now(),
            theme: default_theme(),
            palette: term_view::Palette::from_kit_theme(&Atoms::default()),
            metrics: term_view::CellMetrics::for_font(15.0, fonts::mono_metrics()),
            titles: HashMap::new(),
            cursor_on: true,
            dragging_split: None,
            pane_grids: HashMap::new(),
            keyboard_mods: keyboard::Modifiers::empty(),
            keys_held_mods: keyboard::Modifiers::empty(),
            float: sola_kit::FloatState::new(APP_ID),
            wheel_burst: HashMap::new(),
        };
        // After the first update the bus subscription is live; expand to
        // session topics so sticky replay is not dropped.
        (app, Task::done(Msg::SubscribeSessions))
    }

    /// A tab window's title: its active pane's OSC title, else the pane's
    /// folder.
    fn title(&self, window: window::Id) -> String {
        let Some(tab) = self.tab_of.get(&window).and_then(|t| self.tabs.get_tab(t)) else {
            return "Terminal".into();
        };
        self.titles
            .get(&tab.active_pane)
            .filter(|t| !t.is_empty())
            .cloned()
            .unwrap_or_else(|| tab_label(&self.tabs.pane_cwd(&tab.active_pane)))
    }

    fn theme(&self, window: window::Id) -> Theme {
        sola_kit::theme_for(self.is_floating(window), &self.theme)
    }

    fn is_floating(&self, window: window::Id) -> bool {
        self.tab_windows
            .river_id(window)
            .is_some_and(|id| self.float.is_window_floating(id))
    }

    fn subscription(&self) -> Subscription<Msg> {
        // IMPORTANT: keep this recipe **stable**. Toggling optional arms
        // rebuilds the whole batch and restarts `bus_subscription`, which
        // can drop sticky TerminalSession replay mid-handoff.
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            emulator::output_subscription().map(Msg::PtyOutput),
            emulator::exit_subscription().map(Msg::PtyExit),
            emulator::title_subscription().map(|(id, t)| Msg::Title(id, t)),
            // Keyboard must be seen even when a widget (e.g. canvas) has
            // captured the event — `event::listen()` only yields Ignored
            // events, which can drop keys. Mouse still uses the ignored-only
            // path below for the drag gestures.
            event::listen_with(|ev, status, id| match &ev {
                Event::Keyboard(_) => Some(Msg::Input(id, ev)),
                _ if matches!(status, iced::event::Status::Ignored) => Some(Msg::Input(id, ev)),
                _ => None,
            }),
            window::events().map(|(id, e)| Msg::Window(id, e)),
            iced::time::every(Duration::from_millis(530)).map(|_| Msg::BlinkTick),
            // One global cursor + release listener for the split-divider
            // drag; update gates on whether a drag is live.
            event::listen_with(|ev, _, id| match ev {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Msg::CursorMoved(id, position.x, position.y))
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Msg::CursorReleased)
                }
                _ => None,
            }),
        ])
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => self.on_bus(&m),
            Msg::SubscribeSessions => {
                // Expand kinds for reconnect too (kit only remembers the
                // last set for bus restart recovery).
                sola_kit::app::set_bus_kinds(SESSION_TOPICS);
                // Stickies replay synchronously on subscribe. Anything
                // arriving after this window is not our last run's.
                self.startup_replay_until = Instant::now() + Duration::from_secs(2);
                if let Ok(mut client) = bus().lock() {
                    if let Err(e) = client.subscribe(SESSION_TOPICS) {
                        tracing::warn!("SubscribeSessions failed: {e}");
                    } else {
                        tracing::info!("subscribed TerminalSession (sticky tab replay)");
                    }
                }
                Task::perform(tokio::time::sleep(FIRST_TAB_WAIT), |_| Msg::EnsureTab)
            }
            Msg::EnsureTab => {
                if self.tab_of.is_empty() {
                    tracing::info!("no saved tab came back; opening one");
                    return self.new_tab();
                }
                Task::none()
            }
            Msg::Window(id, event) => self.on_window(id, event),
            Msg::TitleDrag(id) => sola_kit::drag(Some(id)),
            Msg::TitleResize(id, dir) => sola_kit::drag_resize(Some(id), dir),
            Msg::TitleClose(id) => match self.tab_of.get(&id).cloned() {
                Some(tab) => self.close_tab(&tab),
                None => Task::none(),
            },
            Msg::Noop => Task::none(),
            Msg::PtyExit(pane_id) => {
                let session = self
                    .tabs
                    .pane_meta(&pane_id)
                    .map(|m| m.tmux_session.clone());
                let live = tmux::list_sessions().unwrap_or_default();
                if let Some(ref session) = session {
                    if live.iter().any(|s| s == session) {
                        tracing::info!(
                            pane = %pane_id,
                            %session,
                            "tmux client exited; session lives, reattaching"
                        );
                        let _ = self.tabs.take_pane_runtime(&pane_id);
                        return self.attach_pane(&pane_id, false);
                    }
                }
                self.close_pane_by_id(&pane_id)
            }
            Msg::PtyOutput(pane_id) => {
                perf::pty_output();
                self.tabs.clear_pane_cache(&pane_id);
                // Parked scrollback diagnostics — debug-gated, so quiet by default.
                // Enable with `RUST_LOG=sola_terminal=debug`; grep `SCROLLBACK`.
                if tracing::enabled!(tracing::Level::DEBUG) {
                    if let Some(rt) = self.tabs.pane_runtime(&pane_id) {
                        let (h, o) = rt.emulator.scrollback_stats();
                        tracing::debug!(
                            "SCROLLBACK ptyout pane={} hist={} off={}",
                            &pane_id[..8.min(pane_id.len())],
                            h,
                            o
                        );
                    }
                }
                Task::none()
            }
            Msg::BlinkTick => {
                // Cursor is an uncached overlay in TermView — flipping the
                // phase does not need a grid-cache clear (and clearing it
                // every 530ms was a free hitch under scroll load).
                self.cursor_on = !self.cursor_on;
                Task::none()
            }
            Msg::Input(id, event) => self.on_input(id, event),
            Msg::SelectionChanged => {
                self.tabs.clear_all_caches();
                Task::none()
            }
            Msg::Scrolled(pane) => {
                self.tabs.clear_pane_cache(&pane);
                Task::none()
            }
            Msg::OpenUrl(uri) => {
                links::open_url(&uri);
                Task::none()
            }
            Msg::WheelToPty(pane, bytes) => self.on_wheel_to_pty(pane, bytes),
            Msg::FlushWheel(pane) => self.flush_wheel(&pane),
            Msg::Pasted(text) => self.on_pasted(text),
            Msg::SplitDividerPress(split_id) => {
                self.dragging_split = Some(split_id);
                Task::none()
            }
            Msg::PaneFocused(pane) => {
                let mut changed = false;
                if let Some(tab_id) = self.tabs.tab_of_pane(&pane) {
                    if let Some(tab) = self.tabs.get_tab_mut(&tab_id) {
                        if tab.active_pane != pane {
                            tab.active_pane = pane;
                            changed = true;
                        }
                    }
                }
                if changed {
                    self.tabs.clear_all_caches();
                }
                Task::none()
            }
            Msg::CursorMoved(id, x, y) => {
                // Pane split divider.
                if let Some(split_id) = self.dragging_split.clone() {
                    if let Some(tab_id) = self.tab_of.get(&id).cloned() {
                        let content = self.content_rect(id);
                        let rects = self
                            .tabs
                            .get_tab(&tab_id)
                            .map(|t| state::split_rects(&t.layout, content))
                            .unwrap_or_default();
                        if let Some((_id, rect, dir)) =
                            rects.into_iter().find(|(id, _, _)| id == &split_id)
                        {
                            let ratio = state::ratio_for_drag(rect, dir, x, y, MIN_PANE_PX);
                            if let Some(tab) = self.tabs.get_tab_mut(&tab_id) {
                                state::set_ratio(&mut tab.layout, &split_id, ratio);
                            }
                            self.resize_all_panes();
                        }
                    }
                }
                Task::none()
            }
            Msg::CursorReleased => {
                if self.dragging_split.take().is_some() {
                    if let Some(tab_id) = self.focused_tab() {
                        self.persist_tab(&tab_id);
                    }
                }
                Task::none()
            }
            Msg::Title(pane_id, title) => {
                self.titles.insert(pane_id, title);
                Task::none()
            }
            Msg::CwdResult(pane_id, path) => self.on_cwd_result(pane_id, path),
        }
    }

    /// Window lifecycle: sizes, focus, and the close button / close request.
    fn on_window(&mut self, id: window::Id, event: window::Event) -> Task<Msg> {
        match event {
            window::Event::Opened { size, .. } | window::Event::Resized(size) => {
                self.sizes.insert(id, size);
                self.resize_all_panes();
                self.tabs.clear_all_caches();
                if self.focused.is_none() {
                    self.focused = Some(id);
                }
                Task::none()
            }
            window::Event::Focused => {
                self.focused = Some(id);
                Task::none()
            }
            window::Event::CloseRequested => match self.tab_of.get(&id).cloned() {
                Some(tab) => self.close_tab(&tab),
                None => window::close(id),
            },
            window::Event::Closed => {
                // Closed under us (not through close_tab): forget the window;
                // the tab's tmux sessions live on and come back next start.
                if let Some(tab) = self.tab_of.remove(&id) {
                    tracing::warn!(%tab, "tab window closed outside the app");
                    self.window_of.remove(&tab);
                    self.tab_windows.close(id);
                }
                self.sizes.remove(&id);
                if self.focused == Some(id) {
                    self.focused = self.tab_of.keys().next().copied();
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// The focused window's tab.
    fn focused_tab(&self) -> Option<String> {
        self.focused.and_then(|w| self.tab_of.get(&w).cloned())
    }

    fn view(&self, window: window::Id) -> Element<'_, Msg> {
        let pane: Element<'_, Msg> =
            match self.tab_of.get(&window).and_then(|id| self.tabs.get_tab(id)) {
                Some(tab) => self.render_node(&tab.layout, &tab.active_pane),
                None => container(sola_kit::components::text::caption(""))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
            };

        let bg = self.palette.bg;
        let framed: Element<'_, Msg> = container(pane)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(bg.into()),
                ..container::Style::default()
            })
            .into();

        sola_kit::wrap_if_floating(
            self.is_floating(window),
            self.title(window),
            Msg::TitleDrag(window),
            Msg::TitleClose(window),
            move |dir| Msg::TitleResize(window, dir),
            framed,
        )
    }

    /// Recursively fold a pane tree into kit splits; each leaf is a `TermView`
    /// canvas wrapped with focus-follows-mouse + an active-pane border.
    fn render_node<'a>(&'a self, node: &state::PaneNode, active_pane: &str) -> Element<'a, Msg> {
        match node {
            state::PaneNode::Leaf(pane_id) => self.render_leaf(pane_id, active_pane),
            state::PaneNode::Split {
                id,
                dir,
                ratio,
                a,
                b,
            } => {
                let a_el = self.render_node(a, active_pane);
                let b_el = self.render_node(b, active_pane);
                // Match both panes' cell bg so only the 1px hairline shows
                // (no canvas-grey gutter between black terminal surfaces).
                let line = self.theme.extended_palette().background.stronger.color;
                let colors = sola_kit::components::DividerColors::uniform(self.palette.bg, line);
                sola_kit::components::split_with(
                    *dir,
                    a_el,
                    *ratio,
                    Msg::SplitDividerPress(id.clone()),
                    b_el,
                    colors,
                )
            }
        }
    }

    fn render_leaf<'a>(&'a self, pane_id: &str, active_pane: &str) -> Element<'a, Msg> {
        let inner: Element<'a, Msg> = match self.tabs.pane_runtime(pane_id) {
            Some(rt) => {
                let view = term_view::TermView {
                    term: rt.emulator.term(),
                    cursor_snap: rt.emulator.cursor_snap(),
                    selection_track: rt.emulator.selection_track(),
                    cache: &rt.cache,
                    palette: &self.palette,
                    metrics: self.metrics,
                    cursor_on: self.cursor_on,
                    active: pane_id == active_pane,
                    on_select: Msg::SelectionChanged,
                    on_scroll: Msg::Scrolled(pane_id.to_string()),
                    on_open_url: Box::new(|uri| Msg::OpenUrl(uri)),
                    on_wheel_pty: Box::new({
                        let pid = pane_id.to_string();
                        move |bytes| Msg::WheelToPty(pid.clone(), bytes)
                    }),
                };
                canvas(view).width(Length::Fill).height(Length::Fill).into()
            }
            None => container(
                sola_kit::components::text::caption("…").style(sola_kit::components::text::muted),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        };

        // Pointer-enter focuses this pane (sloppy focus). Focus is shown by
        // the cursor style in TermView (blinking block vs hollow), not a
        // cyan border — macOS Terminal does the same and a 1px accent ring
        // next to the split hairline reads as noisy chrome.
        mouse_area(inner)
            .on_enter(Msg::PaneFocused(pane_id.to_string()))
            .into()
    }

    /// Accumulate a mouse-mode wheel report and flush on the throttle cadence.
    fn on_wheel_to_pty(&mut self, pane: String, bytes: Vec<u8>) -> Task<Msg> {
        perf::wheel_event();
        let dir = wheel_report_dir(&bytes);
        if dir == 0 {
            // Not a recognisable wheel report — pass through immediately.
            if let Some(rt) = self.tabs.pane_runtime(&pane) {
                rt.backend.write(&bytes);
            }
            return Task::none();
        }

        let burst = self.wheel_burst.entry(pane.clone()).or_default();
        // Same-direction notches stack; reversing direction discards the
        // opposite backlog so a flick-then-reverse doesn't play both ways.
        if burst.pending != 0 && burst.pending.signum() != dir {
            burst.pending = 0;
        }
        burst.pending += dir;
        burst.sample = bytes;

        let now = Instant::now();
        let ready = match burst.last_flush {
            None => true,
            Some(t) => now.duration_since(t) >= WHEEL_MIN_INTERVAL,
        };
        if ready {
            return self.flush_wheel(&pane);
        }
        if burst.scheduled {
            return Task::none();
        }
        burst.scheduled = true;
        let wait = burst
            .last_flush
            .map(|t| {
                WHEEL_MIN_INTERVAL
                    .checked_sub(now.duration_since(t))
                    .unwrap_or(Duration::ZERO)
            })
            .unwrap_or(Duration::ZERO);
        let pane_for_task = pane;
        Task::perform(
            async move {
                tokio::time::sleep(wait).await;
                pane_for_task
            },
            Msg::FlushWheel,
        )
    }

    /// Write up to [`WHEEL_MAX_REPORTS_PER_FLUSH`] accumulated wheel reports
    /// for `pane`, and re-schedule if more remain.
    fn flush_wheel(&mut self, pane: &str) -> Task<Msg> {
        let Some(burst) = self.wheel_burst.get_mut(pane) else {
            return Task::none();
        };
        burst.scheduled = false;
        if burst.pending == 0 || burst.sample.is_empty() {
            return Task::none();
        }

        let n = burst
            .pending
            .clamp(-WHEEL_MAX_REPORTS_PER_FLUSH, WHEEL_MAX_REPORTS_PER_FLUSH);
        burst.pending -= n;
        let sample = burst.sample.clone();
        burst.last_flush = Some(Instant::now());
        let more = burst.pending != 0;
        let pending_left = burst.pending;

        let bytes = set_wheel_report_dir(&sample, n > 0);
        if let Some(rt) = self.tabs.pane_runtime(pane) {
            for _ in 0..n.unsigned_abs() {
                rt.backend.write(&bytes);
            }
        }
        perf::wheel_flush(n.unsigned_abs(), pending_left);

        if more {
            if let Some(burst) = self.wheel_burst.get_mut(pane) {
                if !burst.scheduled {
                    burst.scheduled = true;
                    let pane_for_task = pane.to_string();
                    return Task::perform(
                        async move {
                            tokio::time::sleep(WHEEL_MIN_INTERVAL).await;
                            pane_for_task
                        },
                        Msg::FlushWheel,
                    );
                }
            }
        }
        Task::none()
    }

    /// Route a raw iced keyboard event to the active pane of `window`'s tab.
    fn on_input(&mut self, window: window::Id, event: iced::Event) -> Task<Msg> {
        // Keep a live modifier snapshot — see `keyboard_mods` on `App`.
        if let iced::Event::Keyboard(keyboard::Event::ModifiersChanged(mods)) = event {
            self.keyboard_mods = mods;
            return Task::none();
        }

        // Track modifier *keys* independently of the modifier mask. Probe
        // evidence: Shift+Enter arrives as plain CR (mask has no SHIFT) while
        // Alt+Enter correctly gets ESC+CR. If we see KeyPressed/Released for
        // Shift itself, we can still arm Shift+Enter.
        if let iced::Event::Keyboard(keyboard::Event::KeyReleased {
            key, physical_key, ..
        }) = &event
        {
            self.apply_modifier_key(key, physical_key, false);
            return Task::none();
        }

        let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modified_key,
            physical_key,
            modifiers,
            location,
            text,
            repeat,
            ..
        }) = event
        else {
            return Task::none();
        };

        self.apply_modifier_key(&key, &physical_key, true);

        // Bare modifier presses are not written to the PTY.
        if modifier_key_bit(&key, &physical_key).is_some() {
            return Task::none();
        }

        // Union Shift/Ctrl/Alt from snapshot + keys-held. Do not write
        // the union back into `keyboard_mods` — that latches Super after
        // a ⌘-chord when River eats the Super release.
        let event_mods = modifiers;
        let tracked_mods = self.keyboard_mods;
        let keys_held = self.keys_held_mods;
        let modifiers = input::merge_modifiers(event_mods, tracked_mods, keys_held);

        // ⌘/Super shortcuts are handled by the shell (menu → MenuAction). A
        // Super-modified key must never encode to the PTY, so ⌘W (unbound) and
        // friends stay inert and don't leak bytes to the shell.
        if modifiers.logo() {
            return Task::none();
        }

        let Some(pane) = self
            .tab_of
            .get(&window)
            .and_then(|t| self.tabs.get_tab(t))
            .map(|t| t.active_pane.clone())
        else {
            return Task::none();
        };
        let Some(rt) = self.tabs.pane_runtime(&pane) else {
            return Task::none();
        };

        let mut mode = { *rt.emulator.term().lock().mode() };

        // tmux *may* negotiate modifyOtherKeys (CSI > 4 ; Pv m) for other
        // modified keys; when it does, fold that into the kitty disambiguate
        // path. Shift/Ctrl+Enter no longer depend on this — `encode_enter`
        // always emits a distinct sequence — because modern tmux never sends
        // XTMODKEYS to the outer client. Keyed by PaneId.
        let modify_other_keys = extkeys::level(&pane) >= 1;
        if modify_other_keys {
            mode |= alacritty_terminal::term::TermMode::DISAMBIGUATE_ESC_CODES;
        }

        let mods = input::Mods::from(modifiers);
        // Prefer the base `key` for Enter identity: some winit paths leave
        // `modified_key` as Unidentified under Shift while `key` is still
        // Named::Enter. resolve_bytes also special-cases Enter via either.
        let enter_key = match (&key, &modified_key) {
            (keyboard::Key::Named(keyboard::key::Named::Enter), _)
            | (_, keyboard::Key::Named(keyboard::key::Named::Enter)) => {
                keyboard::Key::Named(keyboard::key::Named::Enter)
            }
            _ => modified_key.clone(),
        };
        let bytes = input::resolve_bytes(&input::KeyInput {
            key: &key,
            modified_key: &enter_key,
            mods,
            mode,
            location,
            text: text.as_deref(),
            repeat,
            modify_other_keys,
        });

        if let Some(bytes) = bytes {
            // Always log Enter presses to a dedicated file so we can diagnose
            // Shift+Enter without needing RUST_LOG. Probe side: scripts/keydebug.py.
            if matches!(
                (&key, &enter_key),
                (keyboard::Key::Named(keyboard::key::Named::Enter), _)
                    | (_, keyboard::Key::Named(keyboard::key::Named::Enter))
            ) || text.as_deref() == Some("\r")
                || text.as_deref() == Some("\n")
            {
                let hex = bytes
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                tracing::info!(
                    shift = mods.shift(),
                    alt = mods.alt(),
                    ctrl = mods.ctrl(),
                    event_shift = event_mods.shift(),
                    tracked_shift = tracked_mods.shift(),
                    keys_held_shift = keys_held.shift(),
                    key = ?key,
                    modified_key = ?modified_key,
                    text = ?text,
                    encoded = %hex,
                    "enter key → pty"
                );
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(sola_core::paths::log_path("sola-terminal-keys.log"))
                {
                    use std::io::Write;
                    let _ = writeln!(
                        f,
                        "event_shift={} tracked_shift={} keys_held_shift={} merged_shift={} alt={} ctrl={} key={key:?} mod_key={modified_key:?} text={text:?} → {hex}",
                        event_mods.shift(),
                        tracked_mods.shift(),
                        keys_held.shift(),
                        mods.shift(),
                        mods.alt(),
                        mods.ctrl(),
                    );
                }
            }
            {
                let term = rt.emulator.term();
                let mut guard = term.lock();
                if guard.grid().display_offset() != 0 {
                    guard.scroll_display(alacritty_terminal::grid::Scroll::Bottom);
                }
            }
            let is_enter = bytes == [b'\r'];
            rt.backend.write(&bytes);
            if is_enter {
                if let Some(session) = self.tabs.pane_meta(&pane).map(|m| m.tmux_session.clone()) {
                    let pid = pane.clone();
                    return Task::perform(
                        async move {
                            tokio::time::sleep(Duration::from_millis(CWD_REFRESH_MS)).await;
                            tokio::task::spawn_blocking(move || tmux::pane_current_path(&session))
                                .await
                                .ok()
                                .flatten()
                        },
                        move |path| Msg::CwdResult(pid, path),
                    );
                }
            }
        }
        Task::none()
    }

    /// Update [`Self::keys_held_mods`] from a modifier key press/release.
    fn apply_modifier_key(
        &mut self,
        key: &keyboard::Key,
        physical: &keyboard::key::Physical,
        pressed: bool,
    ) {
        let Some(bit) = modifier_key_bit(key, physical) else {
            return;
        };
        if pressed {
            self.keys_held_mods = self.keys_held_mods | bit;
        } else {
            self.keys_held_mods = self.keys_held_mods & !bit;
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(sola_core::paths::log_path("sola-terminal-keys.log"))
        {
            use std::io::Write;
            let _ = writeln!(
                f,
                "mod_key pressed={pressed} key={key:?} physical={physical:?} keys_held_shift={}",
                self.keys_held_mods.shift(),
            );
        }
    }

    /// The PaneId of the focused tab's active pane.
    fn active_pane(&self) -> Option<String> {
        let id = self.focused_tab()?;
        self.tabs.get_tab(&id).map(|t| t.active_pane.clone())
    }

    fn copy_selection(&self) -> Task<Msg> {
        let Some(pane) = self.active_pane() else {
            return Task::none();
        };
        let Some(rt) = self.tabs.pane_runtime(&pane) else {
            return Task::none();
        };
        let text = { rt.emulator.term().lock().selection_to_string() };
        match text {
            Some(s) if !s.is_empty() => iced::clipboard::write::<Msg>(s),
            _ => Task::none(),
        }
    }

    fn paste(&self) -> Task<Msg> {
        iced::clipboard::read().map(Msg::Pasted)
    }

    fn on_pasted(&mut self, text: Option<String>) -> Task<Msg> {
        let Some(text) = text else {
            return Task::none();
        };
        let Some(pane) = self.active_pane() else {
            return Task::none();
        };
        let Some(rt) = self.tabs.pane_runtime(&pane) else {
            return Task::none();
        };
        let mode = { *rt.emulator.term().lock().mode() };
        let bytes = input::paste(&text, mode);
        rt.backend.write(&bytes);
        Task::none()
    }

    /// A tab window's content area (the whole window) in its coordinates.
    fn content_rect(&self, window: window::Id) -> state::Rect {
        let size = self.sizes.get(&window).copied().unwrap_or(DEFAULT_WINDOW);
        state::Rect {
            x: 0.0,
            y: 0.0,
            w: size.width,
            h: size.height,
        }
    }

    /// Grid (cols, rows) a pane should have, given its tab's window and layout.
    fn pane_grid(&self, pane_id: &str) -> (u16, u16) {
        if let Some(tab_id) = self.tabs.tab_of_pane(pane_id) {
            if let (Some(tab), Some(&window)) =
                (self.tabs.get_tab(&tab_id), self.window_of.get(&tab_id))
            {
                for (pid, rect) in state::pane_rects(&tab.layout, self.content_rect(window)) {
                    if pid == pane_id {
                        return term_view::cols_rows_for(
                            iced::Size::new(rect.w, rect.h),
                            self.metrics,
                        );
                    }
                }
            }
        }
        (DEFAULT_COLS, DEFAULT_ROWS)
    }

    /// Recompute every pane's rect → cols/rows and drive the new size into any
    /// pane whose dimensions changed (the split/close/resize/drag fan-out).
    fn resize_all_panes(&mut self) {
        let mut targets: Vec<(String, u16, u16)> = Vec::new();
        for tab_id in self.tabs.tab_ids_in_order() {
            let Some(&window) = self.window_of.get(&tab_id) else {
                continue;
            };
            let content = self.content_rect(window);
            if let Some(tab) = self.tabs.get_tab(&tab_id) {
                for (pane_id, rect) in state::pane_rects(&tab.layout, content) {
                    let (c, r) =
                        term_view::cols_rows_for(iced::Size::new(rect.w, rect.h), self.metrics);
                    targets.push((pane_id, c, r));
                }
            }
        }
        for (pane_id, c, r) in targets {
            if self.pane_grids.get(&pane_id) == Some(&(c, r)) {
                continue;
            }
            if let Some(rt) = self.tabs.pane_runtime(&pane_id) {
                // Scrollback diagnostics for the parked divider-resize issue
                // (could not reproduce 2026-06-18). Debug-gated: zero cost unless
                // enabled via `RUST_LOG=sola_terminal=debug`. Grep `SCROLLBACK`.
                let dbg = tracing::enabled!(tracing::Level::DEBUG);
                let hb = if dbg {
                    rt.emulator.scrollback_stats().0
                } else {
                    0
                };
                rt.emulator.resize(c, r);
                if dbg {
                    tracing::debug!(
                        "SCROLLBACK resize pane={} -> {}x{} hist {}->{}",
                        &pane_id[..8.min(pane_id.len())],
                        c,
                        r,
                        hb,
                        rt.emulator.scrollback_stats().0
                    );
                }
                rt.backend.resize(c, r);
                rt.backend.sigwinch();
                rt.cache.clear();
                self.pane_grids.insert(pane_id, (c, r));
            }
        }
    }

    fn on_bus(&mut self, m: &Message) -> Task<Msg> {
        self.float.update(m);
        self.tab_windows.update(m);
        // 1. Live theme reload — rebuild widget theme + palette + cell metrics,
        //    then reflow every pane.
        if apply_theme_update(m, &mut self.theme) {
            if let Some(Topic::Theme(bus)) = Topic::parse(m) {
                self.palette = term_view::Palette::from_kit_theme(&atoms_from_bus_theme(&bus));
                self.metrics =
                    term_view::CellMetrics::for_font(self.metrics.font_size, fonts::mono_metrics());
                self.resize_all_panes();
                self.tabs.clear_all_caches();
            }
            return Task::none();
        }

        // 2. Quit request.
        if is_self_quit(m, APP_ID) {
            return iced::exit();
        }

        // 3. Dispatch by topic.
        match Topic::parse(m) {
            Some(Topic::TerminalSession(s)) => {
                if !m.sticky {
                    // Retraction from elsewhere — drop the tab and its window
                    // (plain drop preserves tmux).
                    let leaves = self
                        .tabs
                        .get_tab(&s.id)
                        .map(|t| state::leaves_of(&t.layout))
                        .unwrap_or_default();
                    for p in &leaves {
                        self.tabs.remove_pane(p);
                        self.titles.remove(p);
                        self.pane_grids.remove(p);
                        self.wheel_burst.remove(p);
                    }
                    self.tabs.remove_tab(&s.id);
                    return self.close_window_of(&s.id);
                }

                if self.tabs.get_tab(&s.id).is_some() {
                    // Our own echo / re-emit — local state is already current.
                    return Task::none();
                }
                if Instant::now() > self.startup_replay_until {
                    // Not our last run's tab: restore_tab would prune it
                    // against our boot snapshot and retract the sticky.
                    return Task::none();
                }
                // First sighting during sticky replay: our last run's tabs.
                return self.restore_tab(s);
            }
            Some(Topic::MenuAction(ref p)) if p.app_id == APP_ID => {
                return self.on_menu_action(&p.action_id);
            }
            _ => {}
        }
        Task::none()
    }

    /// Rebuild a tab from a persisted `TerminalSession`, reconciling each
    /// leaf's tmux session against the boot snapshot, then open its window
    /// and attach its panes.
    fn restore_tab(&mut self, s: sola_bus::topics::TerminalSession) -> Task<Msg> {
        let layout: PaneLayout = match s.layout.clone() {
            Some(l) => l,
            None => PaneLayout::Leaf {
                tmux_session: s.tmux_session.clone(),
                cwd: s.cwd.clone(),
            },
        };
        let Some(reconciled) = state::reconcile_layout(layout, &self.live_tmux_at_startup) else {
            tracing::info!(
                id = %s.id,
                tmux = %s.tmux_session,
                "retracting stale TerminalSession (all panes gone)"
            );
            if let Ok(mut client) = bus().lock() {
                let _ = client.retract(Topic::TerminalSession(s));
            }
            return Task::none();
        };

        let mut metas = Vec::new();
        let tree = state::from_layout(&reconciled, &mut metas);
        for meta in &metas {
            self.tabs.upsert_pane_meta(meta.clone());
        }
        let active_pane = state::first_leaf(&tree);
        self.tabs.upsert_tab(state::Tab {
            id: s.id.clone(),
            layout: tree,
            active_pane,
            ordinal: s.ordinal,
        });
        tracing::info!(
            id = %s.id,
            tmux = %s.tmux_session,
            ordinal = s.ordinal,
            panes = metas.len(),
            "restored TerminalSession tab"
        );
        let open = self.open_tab_window(&s.id);
        Task::batch([open, self.attach_all_panes(&s.id, true)])
    }

    /// Open `tab_id`'s window, announced to the shell's tab bar.
    fn open_tab_window(&mut self, tab_id: &str) -> Task<Msg> {
        let (window, task) = self
            .tab_windows
            .open(tab_id, window_settings_transparent(APP_ID));
        self.window_of.insert(tab_id.to_string(), window);
        self.tab_of.insert(window, tab_id.to_string());
        task.map(|_| Msg::Noop)
    }

    /// Close `tab_id`'s window (the tab is already gone). The last window
    /// closing quits.
    fn close_window_of(&mut self, tab_id: &str) -> Task<Msg> {
        let Some(window) = self.window_of.remove(tab_id) else {
            return Task::none();
        };
        self.tab_of.remove(&window);
        self.sizes.remove(&window);
        self.tab_windows.close(window);
        if self.focused == Some(window) {
            self.focused = self.tab_of.keys().next().copied();
        }
        if self.tab_of.is_empty() {
            tracing::info!("last terminal tab closed; quitting");
            return Task::batch([window::close(window), iced::exit()]);
        }
        window::close(window)
    }

    /// Open (or reattach) the PTY for one pane and start its reader thread.
    fn attach_pane(&mut self, pane_id: &str, seed_scrollback: bool) -> Task<Msg> {
        let Some(meta) = self.tabs.pane_meta(pane_id).cloned() else {
            tracing::warn!(pane = %pane_id, "attach_pane: no PaneMeta");
            return Task::none();
        };

        let (cols, rows) = self.pane_grid(pane_id);

        let listener = emulator::Listener::new(
            pane_id.to_string(),
            pty::pty_write_sender(),
            emulator::notify_sender(),
            emulator::title_sender(),
        );
        let em = emulator::Emulator::new(cols, rows, listener);
        let term = em.term();
        let cursor = em.cursor_snap();

        if seed_scrollback {
            match tmux::capture_scrollback(&meta.tmux_session) {
                Ok(text) if !text.trim().is_empty() => {
                    let seed = text.replace('\n', "\r\n");
                    let mut processor: alacritty_terminal::vte::ansi::Processor<
                        alacritty_terminal::vte::ansi::StdSyncHandler,
                    > = alacritty_terminal::vte::ansi::Processor::new();
                    let mut t = term.lock();
                    processor.advance(&mut *t, seed.as_bytes());
                    emulator::publish_cursor(&*t, &cursor);
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(pane = %pane_id, "scrollback capture failed: {e}"),
            }
        }

        let backend = match pty::PtyBackend::spawn_or_attach(
            pane_id,
            &meta.tmux_session,
            cols,
            rows,
            meta.cwd.as_deref(),
            term,
            cursor,
            emulator::notify_sender(),
            emulator::exit_sender(),
        ) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(pane = %pane_id, "spawn_or_attach failed: {e}");
                return Task::none();
            }
        };

        backend.resize(cols, rows);
        backend.sigwinch();

        self.tabs.insert_pane_runtime(
            pane_id.to_string(),
            state::PaneRuntime {
                emulator: em,
                backend,
                cache: canvas::Cache::default(),
            },
        );
        self.pane_grids.insert(pane_id.to_string(), (cols, rows));

        let pid = pane_id.to_string();
        let session = meta.tmux_session.clone();
        Task::perform(
            async move {
                tokio::time::sleep(Duration::from_millis(CWD_INITIAL_DELAY_MS)).await;
                tokio::task::spawn_blocking(move || tmux::pane_current_path(&session))
                    .await
                    .ok()
                    .flatten()
            },
            move |path| Msg::CwdResult(pid, path),
        )
    }

    /// Attach every not-yet-attached leaf of a tab.
    fn attach_all_panes(&mut self, tab_id: &str, seed: bool) -> Task<Msg> {
        let leaves = match self.tabs.get_tab(tab_id) {
            Some(t) => state::leaves_of(&t.layout),
            None => return Task::none(),
        };
        let mut tasks = Vec::new();
        for p in leaves {
            if !self.tabs.has_pane_runtime(&p) {
                tasks.push(self.attach_pane(&p, seed));
            }
        }
        Task::batch(tasks)
    }

    /// Mint a new tab (single pane) in a new window, persist it, and attach.
    /// The shell puts the window in its focused zone.
    fn new_tab(&mut self) -> Task<Msg> {
        let tab_id = uuid::Uuid::new_v4().to_string();
        let pane_id = uuid::Uuid::new_v4().to_string();
        let tmux_session = tmux::session_name(&pane_id);

        let source_cwd = self.active_pane().and_then(|p| self.tabs.pane_cwd(&p));
        let cwd = state::inherit_cwd(source_cwd.as_deref());

        let ordinals: Vec<u32> = self.tabs.tab_strip().iter().map(|t| t.ordinal).collect();
        let ordinal = state::next_ordinal(&ordinals);

        self.tabs.upsert_pane_meta(state::PaneMeta {
            id: pane_id.clone(),
            tmux_session,
            cwd,
        });
        self.tabs.upsert_tab(state::Tab {
            id: tab_id.clone(),
            layout: state::PaneNode::Leaf(pane_id.clone()),
            active_pane: pane_id.clone(),
            ordinal,
        });
        self.persist_tab(&tab_id);
        let open = self.open_tab_window(&tab_id);
        Task::batch([open, self.attach_pane(&pane_id, false)])
    }

    /// Split the focused tab's active pane in `dir`: mint a sibling pane (new
    /// tmux session inheriting cwd), insert it, focus it, reflow, persist,
    /// attach.
    fn split_active_pane(&mut self, dir: SplitDir) -> Task<Msg> {
        let Some(tab_id) = self.focused_tab() else {
            return Task::none();
        };
        let source_pane = match self.tabs.get_tab(&tab_id) {
            Some(t) => t.active_pane.clone(),
            None => return Task::none(),
        };
        let source_cwd = self.tabs.pane_cwd(&source_pane);
        let new_pane = uuid::Uuid::new_v4().to_string();
        let split_id = uuid::Uuid::new_v4().to_string();
        let tmux_session = tmux::session_name(&new_pane);
        let cwd = state::inherit_cwd(source_cwd.as_deref());

        if let Some(tab) = self.tabs.get_tab_mut(&tab_id) {
            state::split_leaf(&mut tab.layout, &source_pane, &split_id, dir, &new_pane);
            tab.active_pane = new_pane.clone();
        }
        self.tabs.upsert_pane_meta(state::PaneMeta {
            id: new_pane.clone(),
            tmux_session,
            cwd,
        });

        self.resize_all_panes();
        let task = self.attach_pane(&new_pane, false);
        self.persist_tab(&tab_id);
        self.tabs.clear_all_caches();
        task
    }

    fn close_active_pane(&mut self) -> Task<Msg> {
        match self.active_pane() {
            Some(pane) => self.close_pane_by_id(&pane),
            None => Task::none(),
        }
    }

    /// Close one pane: kill its tmux, drop its runtime, promote its sibling.
    /// Closing the last pane closes the whole tab.
    fn close_pane_by_id(&mut self, pane_id: &str) -> Task<Msg> {
        let Some(tab_id) = self.tabs.tab_of_pane(pane_id) else {
            return Task::none();
        };

        // Explicit close kills tmux (a plain drop would preserve it).
        if let Some(rt) = self.tabs.pane_runtime(pane_id) {
            rt.backend.close();
        }

        let Some(old_layout) = self.tabs.get_tab(&tab_id).map(|t| t.layout.clone()) else {
            return Task::none();
        };
        let new_tree = state::close_leaf(old_layout.clone(), pane_id);

        self.tabs.remove_pane(pane_id);
        self.titles.remove(pane_id);
        self.pane_grids.remove(pane_id);
        self.wheel_burst.remove(pane_id);

        match new_tree {
            None => self.close_tab(&tab_id),
            Some(tree) => {
                let next_active = state::sibling_first_leaf(&old_layout, pane_id)
                    .unwrap_or_else(|| state::first_leaf(&tree));
                if let Some(tab) = self.tabs.get_tab_mut(&tab_id) {
                    tab.layout = tree;
                    tab.active_pane = next_active;
                }
                self.resize_all_panes();
                self.persist_tab(&tab_id);
                self.tabs.clear_all_caches();
                Task::none()
            }
        }
    }

    /// Close a whole tab: kill every pane's tmux, retract the slot, drop the
    /// tab, and close its window.
    fn close_tab(&mut self, tab_id: &str) -> Task<Msg> {
        let leaves = self
            .tabs
            .get_tab(tab_id)
            .map(|t| state::leaves_of(&t.layout))
            .unwrap_or_default();
        for p in &leaves {
            if let Some(rt) = self.tabs.pane_runtime(p) {
                rt.backend.close();
            }
            self.tabs.remove_pane(p);
            self.titles.remove(p);
            self.pane_grids.remove(p);
            self.wheel_burst.remove(p);
        }

        let ordinal = self.tabs.get_tab(tab_id).map(|t| t.ordinal).unwrap_or(0);
        if let Ok(mut client) = bus().lock() {
            let _ = client.retract(Topic::TerminalSession(sola_bus::topics::TerminalSession {
                id: tab_id.to_string(),
                tmux_session: String::new(),
                cwd: None,
                ordinal,
                layout: None,
            }));
        }

        self.tabs.remove_tab(tab_id);
        self.close_window_of(tab_id)
    }

    /// Persist a tab as a `TerminalSession`: the pane tree when split, else a
    /// single-pane record (`layout: None`) for back-compat.
    fn persist_tab(&self, tab_id: &str) {
        let Some(tab) = self.tabs.get_tab(tab_id) else {
            return;
        };
        let leaves = state::leaves_of(&tab.layout);
        let first = leaves.first().cloned().unwrap_or_default();
        let tmux_session = self
            .tabs
            .pane_meta(&first)
            .map(|m| m.tmux_session.clone())
            .unwrap_or_else(|| tmux::session_name(&first));
        let cwd = self.tabs.pane_cwd(&tab.active_pane);
        let layout = if leaves.len() <= 1 {
            None
        } else {
            self.tabs.layout_of(tab_id)
        };

        if let Ok(mut client) = bus().lock() {
            if let Err(e) = client.emit(Topic::TerminalSession(sola_bus::topics::TerminalSession {
                id: tab_id.to_string(),
                tmux_session,
                cwd,
                ordinal: tab.ordinal,
                layout,
            })) {
                tracing::warn!(id = %tab_id, "persist_tab: emit TerminalSession failed: {e:?}");
            }
        }
    }

    fn on_cwd_result(&mut self, pane_id: String, path_opt: Option<String>) -> Task<Msg> {
        let Some(path) = path_opt else {
            return Task::none();
        };
        if self.tabs.pane_cwd(&pane_id).as_deref() == Some(path.as_str()) {
            return Task::none();
        }
        let Some(mut meta) = self.tabs.pane_meta(&pane_id).cloned() else {
            return Task::none();
        };
        meta.cwd = Some(path);
        self.tabs.upsert_pane_meta(meta);
        if let Some(tab_id) = self.tabs.tab_of_pane(&pane_id) {
            self.persist_tab(&tab_id);
        }
        Task::none()
    }

    /// Handle a menu action (also the path for shortcuts — click and chord
    /// share `on_menu_action`). Acts on the focused tab.
    fn on_menu_action(&mut self, action: &str) -> Task<Msg> {
        match action {
            "new_tab" => self.new_tab(),
            "split_vertical" => self.split_active_pane(SplitDir::Vertical),
            "split_horizontal" => self.split_active_pane(SplitDir::Horizontal),
            "close_pane" => self.close_active_pane(),
            "copy" => self.copy_selection(),
            "paste" => self.paste(),
            other => {
                tracing::debug!(action = %other, "on_menu_action: unknown action");
                Task::none()
            }
        }
    }
}

/// Signed direction of an encoded wheel mouse report: `+1` up, `-1` down,
/// `0` if the bytes don't look like a wheel report.
fn wheel_report_dir(bytes: &[u8]) -> i32 {
    // SGR: ESC [ < 64 ; … M  /  ESC [ < 65 ; … M
    // X10: ESC [ M  (32+64=96) …  /  ESC [ M  (32+65=97) …
    if bytes.windows(4).any(|w| w == b"<64;") || bytes.windows(3).any(|w| w == b"<64") {
        return 1;
    }
    if bytes.windows(4).any(|w| w == b"<65;") || bytes.windows(3).any(|w| w == b"<65") {
        return -1;
    }
    // Legacy X10 button byte is index 3: 32+64=96 (up), 32+65=97 (down).
    if bytes.len() >= 4 && bytes[0] == 0x1b && bytes[1] == b'[' && bytes[2] == b'M' {
        match bytes[3] {
            96 => return 1,
            97 => return -1,
            _ => {}
        }
    }
    0
}

/// Rewrite a sample wheel report so its button matches `up` (wheel-up vs down),
/// preserving col/row and SGR vs X10 form.
fn set_wheel_report_dir(sample: &[u8], up: bool) -> Vec<u8> {
    let mut out = sample.to_vec();
    let want = if up { b"64" } else { b"65" };
    let other = if up { b"65" } else { b"64" };
    // SGR: replace the first "64"/"65" after '<' .
    if let Some(i) = out.windows(2).position(|w| w == other) {
        out[i] = want[0];
        out[i + 1] = want[1];
        return out;
    }
    if out.windows(2).any(|w| w == want) {
        return out;
    }
    // X10: button byte at index 3.
    if out.len() >= 4 && out[0] == 0x1b && out[1] == b'[' && out[2] == b'M' {
        out[3] = if up { 96 } else { 97 };
    }
    out
}

#[cfg(test)]
mod wheel_throttle_tests {
    use super::{set_wheel_report_dir, wheel_report_dir};

    #[test]
    fn sgr_wheel_up_dir() {
        assert_eq!(wheel_report_dir(b"\x1b[<64;7;2M"), 1);
    }

    #[test]
    fn sgr_wheel_down_dir() {
        assert_eq!(wheel_report_dir(b"\x1b[<65;7;2M"), -1);
    }

    #[test]
    fn x10_wheel_dirs() {
        assert_eq!(wheel_report_dir(&[0x1b, b'[', b'M', 96, 33, 33]), 1);
        assert_eq!(wheel_report_dir(&[0x1b, b'[', b'M', 97, 33, 33]), -1);
    }

    #[test]
    fn flip_sgr_direction() {
        let up = b"\x1b[<64;10;3M";
        let down = set_wheel_report_dir(up, false);
        assert_eq!(down, b"\x1b[<65;10;3M");
        assert_eq!(set_wheel_report_dir(&down, true), up);
    }

    #[test]
    fn non_wheel_is_zero() {
        assert_eq!(wheel_report_dir(b"hello"), 0);
        assert_eq!(wheel_report_dir(b"\x1b[<0;1;1M"), 0);
    }
}

/// Map a key to its modifier bit, if it is a modifier key.
fn modifier_key_bit(
    key: &keyboard::Key,
    physical: &keyboard::key::Physical,
) -> Option<keyboard::Modifiers> {
    use keyboard::key::{Code, Named, Physical};
    if let keyboard::Key::Named(n) = key {
        match n {
            Named::Shift => return Some(keyboard::Modifiers::SHIFT),
            Named::Control => return Some(keyboard::Modifiers::CTRL),
            Named::Alt => return Some(keyboard::Modifiers::ALT),
            Named::Super | Named::Meta => return Some(keyboard::Modifiers::LOGO),
            _ => {}
        }
    }
    let code = match physical {
        Physical::Code(c) => *c,
        _ => return None,
    };
    match code {
        Code::ShiftLeft | Code::ShiftRight => Some(keyboard::Modifiers::SHIFT),
        Code::ControlLeft | Code::ControlRight => Some(keyboard::Modifiers::CTRL),
        Code::AltLeft | Code::AltRight => Some(keyboard::Modifiers::ALT),
        Code::SuperLeft | Code::SuperRight => Some(keyboard::Modifiers::LOGO),
        _ => None,
    }
}
