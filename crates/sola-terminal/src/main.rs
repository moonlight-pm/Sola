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

use iced::widget::{canvas, container, mouse_area, row};
use iced::{Element, Event, Length, Subscription, Task, Theme};
use iced::{event, keyboard, mouse};

use sola_bus::topics::{PaneLayout, SplitDir, TerminalConfig, Topic, TopicKind};
use sola_bus::Message;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus, bus_subscription, is_self_quit, startup,
    window_settings_transparent,
};
use sola_kit::fonts;
use sola_kit::theme::{Atoms, atoms_from_bus_theme, default_theme};

use sola_terminal::{emulator, extkeys, input, links, perf, pty, state, term_view, tmux};

mod menu;
mod sidebar;

const APP_ID: &str = "sola-terminal";

/// Default grid until a pane reports a real size.
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

/// Milliseconds to wait after Enter before querying tmux for the pane's cwd.
const CWD_REFRESH_MS: u64 = 150;

/// Slightly longer initial delay used when attaching a pane for the first time.
const CWD_INITIAL_DELAY_MS: u64 = 300;

/// Minimum pane extent (logical px) along a split's main axis. Clamps the
/// divider drag so neither side collapses below it.
const MIN_PANE_PX: f32 = 80.0;

/// Compute the pane *area size* (the canvas region the terminal grids occupy)
/// from the window size and the sidebar width.
///
/// The sidebar panel takes `sidebar_w` logical pixels on the left, plus the
/// kit resize divider ([`sola_kit::components::DIVIDER_HIT_PX`]); the rest is
/// the content area, full height. Clamps to zero so the value is always safe
/// to pass to [`term_view::cols_rows_for`]. Pure for headless testing.
pub(crate) fn pane_size(window: iced::Size, sidebar_w: f32) -> iced::Size {
    let chrome = sidebar_w + sola_kit::components::DIVIDER_HIT_PX;
    let w = (window.width - chrome).max(0.0);
    iced::Size::new(w, window.height)
}

/// Parse a `"select_tab_{N}"` menu action id into a 0-based tab index.
fn parse_select_tab_action(action: &str) -> Option<usize> {
    action
        .strip_prefix("select_tab_")
        .and_then(|n| n.parse::<usize>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use term_view::CellMetrics;

    #[test]
    fn pane_size_subtracts_sidebar_and_divider() {
        let window = iced::Size::new(800.0, 480.0);
        let pane = pane_size(window, 200.0);
        let expect_w = 800.0 - 200.0 - sola_kit::components::DIVIDER_HIT_PX;
        assert_eq!(pane.width, expect_w);
        assert_eq!(pane.height, 480.0);
    }

    #[test]
    fn pane_size_clamps_to_zero() {
        let window = iced::Size::new(100.0, 480.0);
        let pane = pane_size(window, 200.0);
        assert_eq!(pane.width, 0.0);
    }

    #[test]
    fn pane_to_grid_end_to_end() {
        let window = iced::Size::new(800.0, 480.0);
        let pane = pane_size(window, 200.0);
        let (cols, rows) = term_view::cols_rows_for(pane, CellMetrics::default());
        // 592px content − pad, ÷ default cell metrics (see term_view).
        assert_eq!(cols, 64);
        assert_eq!(rows, 23);
    }

    #[test]
    fn parse_select_tab_action_tab_zero() {
        assert_eq!(parse_select_tab_action("select_tab_0"), Some(0));
    }

    #[test]
    fn parse_select_tab_action_tab_three() {
        assert_eq!(parse_select_tab_action("select_tab_3"), Some(3));
    }

    #[test]
    fn parse_select_tab_action_unrelated_action() {
        assert_eq!(parse_select_tab_action("new_tab"), None);
    }

    #[test]
    fn parse_select_tab_action_empty_suffix() {
        assert_eq!(parse_select_tab_action("select_tab_"), None);
    }

    #[test]
    fn parse_select_tab_action_non_numeric_suffix() {
        assert_eq!(parse_select_tab_action("select_tab_x"), None);
    }

    #[test]
    fn parse_select_tab_action_partial_prefix() {
        assert_eq!(parse_select_tab_action("select_tab"), None);
    }
}

fn main() -> iced::Result {
    startup(APP_ID);

    // Bring tmux server up before replaying tabs.
    tmux::cleanup_stale_socket();
    tmux::kill_orphaned_clients();
    tmux::ensure_server_running();
    tmux::reload_config();

    // Subscribe to chrome topics only here. `TerminalSession` /
    // `TerminalConfig` stickies are expanded in `Msg::SubscribeSessions`
    // *after* the iced bus pump is live — sticky replay at BusSetup time
    // races the poller handoff and was dropping every tab on restart
    // (tmux sessions still alive, UI empty).
    BusSetup::new(APP_ID)
        .subscribe(&[
            TopicKind::Theme,
            TopicKind::MenuAction,
            TopicKind::CloseApp,
        ])
        .install();

    // Publish the full multi-menu payload directly (BusSetup::app_menu only
    // handles a single-menu definition; terminal needs several menus).
    if let Ok(mut client) = bus().lock() {
        if let Err(e) = client.emit(Topic::SetAppMenu(menu::terminal_menu(&[]))) {
            tracing::warn!("initial app-menu publish failed: {e:?}");
        }
    }

    let app = iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::mono())
        .window(window_settings_transparent(APP_ID));
    app.run()
}

struct App {
    tabs: state::Tabs,
    /// Active TAB id.
    active: Option<String>,
    config: TerminalConfig,
    /// Snapshot of live tmux sessions at startup, used to prune persisted
    /// panes whose tmux peer is gone. None means the query failed — admit all.
    live_tmux_at_startup: Option<HashSet<String>>,
    theme: Theme,
    sidebar: sidebar::SidebarState,
    palette: term_view::Palette,
    window_size: iced::Size,
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
    /// Float tracker + iced window id for CSD while floating.
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
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
    /// Press on the sidebar resize divider.
    SidebarDragStart,
    /// Press on a tab row (potential reorder), carrying the row index.
    ReorderStart(usize),
    /// Animation tick while a tab reorder drag is live (sibling glides).
    ReorderTick,
    /// Press on a pane split divider (carries the SplitId).
    SplitDividerPress(String),
    /// Pointer entered a pane's area (focus-follows-mouse) — carries PaneId.
    PaneFocused(String),
    /// Global cursor sample (x, y). Drives whichever gesture is active.
    CursorMoved(f32, f32),
    /// Global left-button release. Ends whichever gesture is active.
    CursorReleased,
    Input(iced::Event),
    Resized(iced::Size),
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
    WindowReady(Option<iced::window::Id>),
    /// Expand bus subscription to TerminalSession/TerminalConfig so sticky
    /// tab replay lands after the iced bus pump is attached.
    SubscribeSessions,
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
}

/// Full topic set once the iced bus pump can receive sticky replay.
const SESSION_TOPICS: &[TopicKind] = &[
    TopicKind::Theme,
    TopicKind::MenuAction,
    TopicKind::CloseApp,
    TopicKind::TerminalConfig,
    TopicKind::TerminalSession,
];

impl App {
    fn boot() -> (Self, Task<Msg>) {
        let live_tmux_at_startup: Option<HashSet<String>> =
            tmux::list_sessions().map(|v| v.into_iter().collect());
        match &live_tmux_at_startup {
            Some(s) => tracing::info!(count = s.len(), "live sola-* tmux sessions at boot"),
            None => tracing::warn!("tmux list-sessions failed at boot; admitting all stickies"),
        }
        let app = Self {
            tabs: state::Tabs::default(),
            active: None,
            config: TerminalConfig::default(),
            live_tmux_at_startup,
            theme: default_theme(),
            sidebar: sidebar::SidebarState::default(),
            palette: term_view::Palette::from_kit_theme(&Atoms::default()),
            window_size: iced::Size::new(800.0, 480.0),
            metrics: term_view::CellMetrics::for_font(15.0, fonts::mono_metrics()),
            titles: HashMap::new(),
            cursor_on: true,
            dragging_split: None,
            pane_grids: HashMap::new(),
            keyboard_mods: keyboard::Modifiers::empty(),
            keys_held_mods: keyboard::Modifiers::empty(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
            wheel_burst: HashMap::new(),
        };
        (
            app,
            Task::batch([
                sola_kit::window_ready_task(Msg::WindowReady),
                // After the first iced frame the bus subscription is live;
                // expand to session topics so sticky replay is not dropped.
                Task::done(Msg::SubscribeSessions),
            ]),
        )
    }

    fn title(&self) -> String {
        self.active
            .as_deref()
            .and_then(|id| self.tabs.get_tab(id))
            .map(|t| t.active_pane.clone())
            .and_then(|p| self.titles.get(&p).cloned())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "Terminal".into())
    }

    fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.is_floating_any(), &self.theme)
    }

    fn subscription(&self) -> Subscription<Msg> {
        // IMPORTANT: keep this recipe **stable**. Toggling optional arms
        // rebuilds the whole batch and restarts `bus_subscription`, which
        // can drop sticky TerminalSession replay mid-handoff. Always
        // register the same set; gate ReorderTick work in update.
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            emulator::output_subscription().map(Msg::PtyOutput),
            emulator::exit_subscription().map(Msg::PtyExit),
            emulator::title_subscription().map(|(id, t)| Msg::Title(id, t)),
            // Keyboard must be seen even when a widget (e.g. canvas) has
            // captured the event — `event::listen()` only yields Ignored
            // events, which can drop keys. Mouse still uses the ignored-only
            // path below for the drag gestures.
            event::listen_with(|ev, status, _| match &ev {
                Event::Keyboard(_) => Some(Msg::Input(ev)),
                _ if matches!(status, iced::event::Status::Ignored) => Some(Msg::Input(ev)),
                _ => None,
            }),
            iced::window::resize_events().map(|(_id, size)| Msg::Resized(size)),
            iced::time::every(Duration::from_millis(530)).map(|_| Msg::BlinkTick),
            // Single always-on global cursor + release listener, shared by the
            // sidebar-divider, tab-reorder, and pane-divider gestures. Each
            // update arm gates on its own active-flag so only the live gesture
            // does work. CursorMoved carries (x, y) so all three can read it.
            event::listen_with(|ev, _, _| match ev {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Msg::CursorMoved(position.x, position.y))
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Msg::CursorReleased)
                }
                _ => None,
            }),
            // Always registered; `update` no-ops when not dragging.
            iced::time::every(Duration::from_millis(16)).map(|_| Msg::ReorderTick),
        ])
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => self.on_bus(&m),
            Msg::WindowReady(id) => {
                self.window_id = id;
                Task::none()
            }
            Msg::SubscribeSessions => {
                // Expand kinds for reconnect too (kit only remembers the
                // last set for bus restart recovery).
                sola_kit::app::set_bus_kinds(SESSION_TOPICS);
                if let Ok(mut client) = bus().lock() {
                    if let Err(e) = client.subscribe(SESSION_TOPICS) {
                        tracing::warn!("SubscribeSessions failed: {e}");
                    } else {
                        tracing::info!(
                            "subscribed TerminalSession/TerminalConfig (sticky tab replay)"
                        );
                    }
                }
                Task::none()
            }
            Msg::TitleDrag => sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => {
                sola_kit::close_app(APP_ID);
                Task::none()
            }
            Msg::Noop => Task::none(),
            Msg::PtyExit(pane_id) => self.close_pane_by_id(&pane_id),
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
                            &pane_id[..8.min(pane_id.len())], h, o
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
            Msg::Input(event) => self.on_input(event),
            Msg::Resized(size) => self.on_resized(size),
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
            Msg::SidebarDragStart => {
                self.sidebar.dragging_divider = true;
                self.sidebar.drag_anchor = None;
                Task::none()
            }
            Msg::ReorderStart(index) => {
                // start_y = 0.0 sentinel; captured on first CursorMoved.
                // Live-reorder stays off until movement crosses the threshold.
                self.sidebar.reorder = Some((index, 0.0));
                self.sidebar.reorder_cursor_y = 0.0;
                self.sidebar.reorder_dragging = false;
                self.sidebar.reorder_anim.clear();
                Task::none()
            }
            Msg::ReorderTick => {
                if !self.sidebar.reorder_dragging {
                    return Task::none();
                }
                self.sync_reorder_anim();
                Task::none()
            }
            Msg::SplitDividerPress(split_id) => {
                self.dragging_split = Some(split_id);
                Task::none()
            }
            Msg::PaneFocused(pane) => {
                let mut changed = false;
                if let Some(tab_id) = self.active.clone() {
                    let belongs = self
                        .tabs
                        .get_tab(&tab_id)
                        .map(|t| state::leaves_of(&t.layout).iter().any(|p| p == &pane))
                        .unwrap_or(false);
                    if belongs {
                        if let Some(tab) = self.tabs.get_tab_mut(&tab_id) {
                            if tab.active_pane != pane {
                                tab.active_pane = pane;
                                changed = true;
                            }
                        }
                    }
                }
                if changed {
                    self.tabs.clear_all_caches();
                }
                Task::none()
            }
            Msg::CursorMoved(x, y) => {
                // Sidebar resize divider.
                if self.sidebar.dragging_divider {
                    if let Some((anchor_x, anchor_w)) = self.sidebar.drag_anchor {
                        let new_w =
                            sola_kit::components::panel_dragged_width(anchor_x, anchor_w, x);
                        self.config.sidebar_width = new_w as u32;
                        self.resize_all_panes();
                    } else {
                        let current_w = self.config.sidebar_width as f32;
                        self.sidebar.drag_anchor = Some((x, current_w));
                    }
                }
                // Tab reorder.
                if let Some((_from, ref mut start_y)) = self.sidebar.reorder {
                    if *start_y == 0.0 {
                        *start_y = y;
                    }
                    self.sidebar.reorder_cursor_y = y;
                    // Promote to a live drag once the cursor moves past the
                    // threshold — until then it stays a candidate click.
                    if (y - *start_y).abs() >= sola_kit::components::PANEL_REORDER_THRESHOLD {
                        self.sidebar.reorder_dragging = true;
                    }
                    if self.sidebar.reorder_dragging {
                        self.sync_reorder_anim();
                    }
                }
                // Pane split divider.
                if let Some(split_id) = self.dragging_split.clone() {
                    if let Some(tab_id) = self.active.clone() {
                        let content = self.content_rect();
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
                let mut task = Task::none();
                if self.sidebar.dragging_divider {
                    self.sidebar.dragging_divider = false;
                    self.sidebar.drag_anchor = None;
                    if let Ok(mut client) = bus().lock() {
                        if let Err(e) = client.emit(Topic::TerminalConfig(self.config.clone())) {
                            tracing::warn!("emit TerminalConfig failed: {e:?}");
                        }
                    }
                    self.resize_all_panes();
                }
                if self.sidebar.reorder.is_some() {
                    task = self.finish_reorder();
                }
                if self.dragging_split.take().is_some() {
                    if let Some(tab_id) = self.active.clone() {
                        self.persist_tab(&tab_id);
                    }
                }
                task
            }
            Msg::Title(pane_id, title) => {
                self.titles.insert(pane_id, title);
                Task::none()
            }
            Msg::CwdResult(pane_id, path) => self.on_cwd_result(pane_id, path),
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        let pane: Element<'_, Msg> =
            match self.active.as_deref().and_then(|id| self.tabs.get_tab(id)) {
                Some(tab) => self.render_node(&tab.layout, &tab.active_pane),
                None => container(
                    sola_kit::components::text::body("terminal pane (placeholder)")
                        .style(sola_kit::components::text::muted),
                )
                    .padding(sola_kit::components::style::SPACE_MD)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
            };

        let body: Element<'_, Msg> = row![
            sidebar::view(
                &self.sidebar,
                &self.tabs,
                self.active.as_deref(),
                &self.config,
                &self.theme,
                self.palette.bg,
            ),
            pane,
        ]
        .into();

        let bg = self.palette.bg;
        let framed: Element<'_, Msg> = container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(bg.into()),
                ..container::Style::default()
            })
            .into();

        sola_kit::wrap_if_floating(
            self.float.is_floating_any(),
            "Terminal",
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            framed,
        )
    }

    /// Recursively fold a pane tree into kit splits; each leaf is a `TermView`
    /// canvas wrapped with focus-follows-mouse + an active-pane border.
    fn render_node<'a>(&'a self, node: &state::PaneNode, active_pane: &str) -> Element<'a, Msg> {
        match node {
            state::PaneNode::Leaf(pane_id) => self.render_leaf(pane_id, active_pane),
            state::PaneNode::Split { id, dir, ratio, a, b } => {
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

    /// Route a raw iced keyboard event to the active pane's PTY.
    fn on_input(&mut self, event: iced::Event) -> Task<Msg> {
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
            key,
            physical_key,
            ..
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

        let Some(pane) = self.active_pane() else {
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
                    .open("/opt/sola/log/sola-terminal-keys.log")
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
            .open("/opt/sola/log/sola-terminal-keys.log")
        {
            use std::io::Write;
            let _ = writeln!(
                f,
                "mod_key pressed={pressed} key={key:?} physical={physical:?} keys_held_shift={}",
                self.keys_held_mods.shift(),
            );
        }
    }

    /// The PaneId of the active tab's focused pane.
    fn active_pane(&self) -> Option<String> {
        let id = self.active.as_deref()?;
        self.tabs.get_tab(id).map(|t| t.active_pane.clone())
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

    fn on_resized(&mut self, size: iced::Size) -> Task<Msg> {
        self.window_size = size;
        self.resize_all_panes();
        self.tabs.clear_all_caches();
        Task::none()
    }

    /// The content area (window minus sidebar) in window-logical coordinates.
    fn content_rect(&self) -> state::Rect {
        let sidebar_w = self.config.sidebar_width as f32;
        let pane = pane_size(self.window_size, sidebar_w);
        state::Rect {
            x: sidebar_w,
            y: 0.0,
            w: pane.width,
            h: pane.height,
        }
    }

    /// Grid (cols, rows) a pane should have, given its tab's current layout.
    fn pane_grid(&self, pane_id: &str) -> (u16, u16) {
        let content = self.content_rect();
        if let Some(tab_id) = self.tabs.tab_of_pane(pane_id) {
            if let Some(tab) = self.tabs.get_tab(&tab_id) {
                for (pid, rect) in state::pane_rects(&tab.layout, content) {
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
        let content = self.content_rect();
        let mut targets: Vec<(String, u16, u16)> = Vec::new();
        for tab_id in self.tabs.tab_ids_in_order() {
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
                let hb = if dbg { rt.emulator.scrollback_stats().0 } else { 0 };
                rt.emulator.resize(c, r);
                if dbg {
                    tracing::debug!(
                        "SCROLLBACK resize pane={} -> {}x{} hist {}->{}",
                        &pane_id[..8.min(pane_id.len())], c, r, hb,
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
        // 1. Live theme reload — rebuild widget theme + palette + cell metrics,
        //    then reflow every pane.
        if apply_theme_update(m, &mut self.theme) {
            if let Some(Topic::Theme(bus)) = Topic::parse(m) {
                self.palette =
                    term_view::Palette::from_kit_theme(&atoms_from_bus_theme(&bus));
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
            Some(Topic::TerminalConfig(cfg)) => {
                let preferred = cfg.active_tab_id.clone();
                self.config = cfg;
                // Sticky replay / re-emit may land after sessions. Apply the
                // remembered active tab once the target exists.
                if let Some(id) = preferred {
                    if self.active.as_deref() != Some(id.as_str())
                        && self.tabs.get_tab(&id).is_some()
                    {
                        return self.select_tab(&id);
                    }
                }
            }
            Some(Topic::TerminalSession(s)) => {
                if !m.sticky {
                    // Retraction from elsewhere — drop the tab + panes (plain
                    // drop preserves tmux).
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
                    self.republish_menu();
                    return Task::none();
                }

                if self.tabs.get_tab(&s.id).is_some() {
                    // Our own echo / re-emit — local state is already current.
                    return Task::none();
                }
                // First sighting: boot replay of a persisted tab.
                return self.restore_tab(s);
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
            if let Err(e) =
                client.emit(Topic::SetAppMenu(menu::terminal_menu(&self.tabs.tab_strip())))
            {
                tracing::warn!("republish_menu failed: {e:?}");
            }
        }
    }

    /// Rebuild a tab from a persisted `TerminalSession`, reconciling each
    /// leaf's tmux session against the boot snapshot, then attach if active.
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
        // Prefer the tab remembered in TerminalConfig. Until that config
        // (or a matching session) arrives, provisionally take the first
        // sticky tab so the window isn't blank during boot.
        let preferred = self.config.active_tab_id.as_deref();
        if preferred == Some(s.id.as_str()) {
            self.active = Some(s.id.clone());
        } else if self.active.is_none() {
            self.active = Some(s.id.clone());
        }
        self.republish_menu();

        // Attach the active tab's panes eagerly (seed scrollback). Other tabs
        // lazy-attach on `select_tab`.
        if self.active.as_deref() == Some(s.id.as_str()) {
            return self.attach_all_panes(&s.id, true);
        }
        Task::none()
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

    /// Mint a new tab (single pane), persist it, and attach.
    fn new_tab(&mut self) -> Task<Msg> {
        let tab_id = uuid::Uuid::new_v4().to_string();
        let pane_id = uuid::Uuid::new_v4().to_string();
        let tmux_session = tmux::session_name(&pane_id);

        let source_cwd = self.active_pane().and_then(|p| self.tabs.pane_cwd(&p));
        let cwd = state::inherit_cwd(source_cwd.as_deref());

        let ordinals: Vec<u32> = self
            .tabs
            .tab_strip()
            .iter()
            .map(|t| t.ordinal)
            .collect();
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
        self.active = Some(tab_id.clone());
        self.config.active_tab_id = Some(tab_id.clone());
        self.persist_config();

        self.persist_tab(&tab_id);
        self.republish_menu();
        self.attach_pane(&pane_id, false)
    }

    /// Split the active pane in `dir`: mint a sibling pane (new tmux session
    /// inheriting cwd), insert it, focus it, reflow, persist, attach.
    fn split_active_pane(&mut self, dir: SplitDir) -> Task<Msg> {
        let Some(tab_id) = self.active.clone() else {
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
    /// tab, and pick the next active tab.
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

        if self.active.as_deref() == Some(tab_id) {
            let order = self.tabs.tab_ids_in_order();
            self.active = state::next_active_after_close(&order, tab_id);
            self.config.active_tab_id = self.active.clone();
            self.persist_config();
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
        self.republish_menu();
        Task::none()
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
    /// share `on_menu_action`).
    fn on_menu_action(&mut self, action: &str) -> Task<Msg> {
        match action {
            "new_tab" => self.new_tab(),
            "split_vertical" => self.split_active_pane(SplitDir::Vertical),
            "split_horizontal" => self.split_active_pane(SplitDir::Horizontal),
            "close_pane" => self.close_active_pane(),
            "copy" => self.copy_selection(),
            "paste" => self.paste(),
            other => {
                if let Some(index) = parse_select_tab_action(other) {
                    let ids = self.tabs.tab_ids_in_order();
                    if let Some(id) = ids.get(index) {
                        self.select_tab(&id.clone())
                    } else {
                        Task::none()
                    }
                } else {
                    tracing::debug!(action = %other, "on_menu_action: unknown action");
                    Task::none()
                }
            }
        }
    }

    /// Switch the active tab to `id`, lazy-attaching its panes the first time.
    fn select_tab(&mut self, id: &str) -> Task<Msg> {
        let leaves = match self.tabs.get_tab(id) {
            Some(tab) => state::leaves_of(&tab.layout),
            None => return Task::none(),
        };
        self.active = Some(id.to_string());
        if self.config.active_tab_id.as_deref() != Some(id) {
            self.config.active_tab_id = Some(id.to_string());
            self.persist_config();
        }
        self.tabs.clear_all_caches();

        if leaves.iter().any(|p| !self.tabs.has_pane_runtime(p)) {
            return self.attach_all_panes(id, true);
        }
        self.resize_all_panes();
        Task::none()
    }

    /// Emit the current `TerminalConfig` (sidebar + active tab) as sticky state.
    fn persist_config(&self) {
        if let Ok(mut client) = bus().lock() {
            if let Err(e) = client.emit(Topic::TerminalConfig(self.config.clone())) {
                tracing::warn!("emit TerminalConfig failed: {e:?}");
            }
        }
    }

    /// Drive sibling-offset animations for the live tab-reorder preview.
    fn sync_reorder_anim(&mut self) {
        let Some((from, start_y)) = self.sidebar.reorder else {
            return;
        };
        if !self.sidebar.reorder_dragging {
            return;
        }
        let n = self.tabs.tab_ids_in_order().len();
        if n == 0 {
            return;
        }
        let to = sola_kit::components::panel_drop_index_relative(
            from,
            start_y,
            self.sidebar.reorder_cursor_y,
            sola_kit::components::PANEL_ROW_H,
            n,
        );
        self.sidebar
            .reorder_anim
            .sync(from, to, n, iced::time::Instant::now());
    }

    /// Finish a tab-reorder gesture: click → select; drag → renumber ordinals.
    fn finish_reorder(&mut self) -> Task<Msg> {
        let gesture = self.sidebar.reorder.take();
        let final_cursor_y = self.sidebar.reorder_cursor_y;
        let was_dragging = self.sidebar.reorder_dragging;
        self.sidebar.reorder_cursor_y = 0.0;
        self.sidebar.reorder_dragging = false;
        self.sidebar.reorder_anim.clear();

        let Some((from, start_y)) = gesture else {
            return Task::none();
        };

        let ids = self.tabs.tab_ids_in_order();
        // Never crossed the threshold → click, not drag: select the tab.
        // (`start_y == 0.0` covers press-with-no-move before the first sample.)
        if !was_dragging || start_y == 0.0 {
            if let Some(id) = ids.get(from) {
                return self.select_tab(&id.clone());
            }
            return Task::none();
        }

        let n = ids.len();
        if n == 0 {
            return Task::none();
        }
        // Anchor-relative: same formula the kit uses for the drop-slot
        // highlight. Absolute `panel_drop_index` + PANEL_HEADER_H was wrong
        // here (terminal has no collapse header, and Y is window-absolute).
        let to = sola_kit::components::panel_drop_index_relative(
            from,
            start_y,
            final_cursor_y,
            sola_kit::components::PANEL_ROW_H,
            n,
        );
        if from == to {
            return Task::none();
        }

        let new_order = sola_kit::components::panel_reordered(&ids, from, to);
        let ordinals: HashMap<String, u32> = ids
            .iter()
            .filter_map(|id| self.tabs.get_tab(id).map(|t| (id.clone(), t.ordinal)))
            .collect();
        let changed = sola_kit::components::panel_renumber_changed(&ordinals, &new_order);

        for (id, new_ordinal) in &changed {
            if let Some(tab) = self.tabs.get_tab_mut(id) {
                tab.ordinal = *new_ordinal;
            }
            self.persist_tab(id);
        }
        if !changed.is_empty() {
            self.republish_menu();
        }
        Task::none()
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
