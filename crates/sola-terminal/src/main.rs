use std::collections::{HashMap, HashSet};
use std::time::Duration;
use std::sync::Arc;

use iced::widget::{canvas, container, mouse_area, row, text};
use iced::{Border, Element, Event, Length, Subscription, Task, Theme};
use iced::{event, keyboard, mouse};

use sola_bus::topics::{PaneLayout, SplitDir, TerminalConfig, Topic, TopicKind};
use sola_bus::Message;
use sola_kit::app::{BusSetup, apply_theme_update, bus, bus_subscription, is_self_quit, startup, window_settings};
use sola_kit::fonts;
use sola_kit::theme::{Atoms, atoms_from_bus_theme, default_theme};

mod emulator;
mod extkeys;
mod input;
mod menu;
mod pty;
mod sidebar;
mod state;
mod term_view;
mod tmux;

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
/// The sidebar takes `sidebar_w` logical pixels on the left; the rest is the
/// content area, full height. Clamps to zero so the value is always safe to
/// pass to [`term_view::cols_rows_for`]. Pure for headless testing.
pub(crate) fn pane_size(window: iced::Size, sidebar_w: f32) -> iced::Size {
    let w = (window.width - sidebar_w).max(0.0);
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
    fn pane_size_subtracts_sidebar() {
        let window = iced::Size::new(800.0, 480.0);
        let pane = pane_size(window, 200.0);
        assert_eq!(pane.width, 600.0);
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
        assert_eq!(cols, 65);
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

    BusSetup::new(APP_ID)
        .subscribe(&[
            TopicKind::Theme,
            TopicKind::MenuAction,
            TopicKind::CloseApp,
            TopicKind::TerminalConfig,
            TopicKind::TerminalSession,
        ])
        .install();

    // Publish the full multi-menu payload directly (BusSetup::app_menu only
    // handles a single-menu definition; terminal needs several menus).
    if let Ok(mut client) = bus().lock() {
        if let Err(e) = client.emit(Topic::SetAppMenu(menu::terminal_menu(&[]))) {
            tracing::warn!("initial app-menu publish failed: {e:?}");
        }
    }

    let app = iced::application(App::new, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::mono())
        .window(window_settings(APP_ID));
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
    Scrolled,
    Pasted(Option<String>),
    /// OSC 0/2 title for a pane (PaneId, title).
    Title(String, String),
    /// Result of an async tmux cwd query (PaneId, path).
    CwdResult(String, Option<String>),
    BlinkTick,
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
            palette: term_view::Palette::from_kit_theme(&Atoms::default()),
            window_size: iced::Size::new(800.0, 480.0),
            metrics: term_view::CellMetrics::for_font(15.0, fonts::mono_metrics()),
            titles: HashMap::new(),
            cursor_on: true,
            dragging_split: None,
            pane_grids: HashMap::new(),
        };
        (app, Task::none())
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
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            emulator::output_subscription().map(Msg::PtyOutput),
            emulator::exit_subscription().map(Msg::PtyExit),
            emulator::title_subscription().map(|(id, t)| Msg::Title(id, t)),
            iced::event::listen().map(Msg::Input),
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
        ])
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => self.on_bus(&m),
            Msg::Noop => Task::none(),
            Msg::PtyExit(pane_id) => self.close_pane_by_id(&pane_id),
            Msg::PtyOutput(pane_id) => {
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
                self.cursor_on = !self.cursor_on;
                self.tabs.clear_all_caches();
                Task::none()
            }
            Msg::Input(event) => self.on_input(event),
            Msg::Resized(size) => self.on_resized(size),
            Msg::SelectionChanged | Msg::Scrolled => {
                self.tabs.clear_all_caches();
                Task::none()
            }
            Msg::Pasted(text) => self.on_pasted(text),
            Msg::SidebarDragStart => {
                self.sidebar.dragging_divider = true;
                self.sidebar.drag_anchor = None;
                Task::none()
            }
            Msg::ReorderStart(index) => {
                self.sidebar.reorder = Some((index, 0.0));
                self.sidebar.reorder_cursor_y = 0.0;
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
                None => container(text("terminal pane (placeholder)"))
                    .padding(8)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
            };

        let body: Element<'_, Msg> = row![
            sidebar::view(&self.sidebar, &self.tabs, self.active.as_deref(), &self.config),
            pane,
        ]
        .into();

        let bg = self.palette.bg;
        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(bg.into()),
                ..container::Style::default()
            })
            .into()
    }

    /// Recursively fold a pane tree into kit splits; each leaf is a `TermView`
    /// canvas wrapped with focus-follows-mouse + an active-pane border.
    fn render_node<'a>(&'a self, node: &state::PaneNode, active_pane: &str) -> Element<'a, Msg> {
        match node {
            state::PaneNode::Leaf(pane_id) => self.render_leaf(pane_id, active_pane),
            state::PaneNode::Split { id, dir, ratio, a, b } => {
                let a_el = self.render_node(a, active_pane);
                let b_el = self.render_node(b, active_pane);
                sola_kit::components::split(
                    *dir,
                    a_el,
                    *ratio,
                    Msg::SplitDividerPress(id.clone()),
                    b_el,
                )
            }
        }
    }

    fn render_leaf<'a>(&'a self, pane_id: &str, active_pane: &str) -> Element<'a, Msg> {
        let inner: Element<'a, Msg> = match self.tabs.pane_runtime(pane_id) {
            Some(rt) => {
                let view = term_view::TermView {
                    term: rt.emulator.term(),
                    cache: &rt.cache,
                    palette: &self.palette,
                    metrics: self.metrics,
                    cursor_on: self.cursor_on,
                    on_select: Msg::SelectionChanged,
                    on_scroll: Msg::Scrolled,
                };
                canvas(view).width(Length::Fill).height(Length::Fill).into()
            }
            None => container(text("…"))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        };

        // Pointer-enter focuses this pane (sloppy focus).
        let focusable = mouse_area(inner).on_enter(Msg::PaneFocused(pane_id.to_string()));

        // Active-pane border (accent); inactive panes get a same-as-bg 1px
        // border so the layout doesn't shift between focus states.
        let border_color = if pane_id == active_pane {
            self.theme.extended_palette().primary.base.color
        } else {
            self.palette.bg
        };
        container(focusable)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                border: Border {
                    color: border_color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..container::Style::default()
            })
            .into()
    }

    /// Route a raw iced keyboard event to the active pane's PTY.
    fn on_input(&mut self, event: iced::Event) -> Task<Msg> {
        let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modified_key,
            modifiers,
            location,
            text,
            repeat,
            ..
        }) = event
        else {
            return Task::none();
        };

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

        // tmux negotiates modifyOtherKeys on behalf of its inner app; when
        // active, fold it into the kitty disambiguate path so Shift+Enter is
        // distinct from Enter. Keyed by PaneId (the emulator listener id).
        let modify_other_keys = extkeys::level(&pane) >= 1;
        if modify_other_keys {
            mode |= alacritty_terminal::term::TermMode::DISAMBIGUATE_ESC_CODES;
        }

        let mods = input::Mods::from(modifiers);
        let bytes = input::resolve_bytes(&input::KeyInput {
            key: &key,
            modified_key: &modified_key,
            mods,
            mode,
            location,
            text: text.as_deref(),
            repeat,
            modify_other_keys,
        });

        if let Some(bytes) = bytes {
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
                self.config = cfg;
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
            tracing::info!(id = %s.id, "retracting stale TerminalSession (all panes gone)");
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
        let was_empty = self.tabs.is_empty();
        self.tabs.upsert_tab(state::Tab {
            id: s.id.clone(),
            layout: tree,
            active_pane,
            ordinal: s.ordinal,
        });
        if self.active.is_none() {
            self.active = Some(s.id.clone());
        }
        self.republish_menu();

        // Attach the active tab's panes eagerly (seed scrollback). Other tabs
        // lazy-attach on `select_tab`.
        if was_empty || self.active.as_deref() == Some(s.id.as_str()) {
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

        if seed_scrollback {
            match tmux::capture_scrollback(&meta.tmux_session) {
                Ok(text) if !text.trim().is_empty() => {
                    let seed = text.replace('\n', "\r\n");
                    let mut processor: alacritty_terminal::vte::ansi::Processor<
                        alacritty_terminal::vte::ansi::StdSyncHandler,
                    > = alacritty_terminal::vte::ansi::Processor::new();
                    let mut t = term.lock();
                    processor.advance(&mut *t, seed.as_bytes());
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
        }

        if self.active.as_deref() == Some(tab_id) {
            let order = self.tabs.tab_ids_in_order();
            self.active = state::next_active_after_close(&order, tab_id);
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
        self.tabs.clear_all_caches();

        if leaves.iter().any(|p| !self.tabs.has_pane_runtime(p)) {
            return self.attach_all_panes(id, true);
        }
        self.resize_all_panes();
        Task::none()
    }

    /// Finish a tab-reorder gesture: click → select; drag → renumber ordinals.
    fn finish_reorder(&mut self) -> Task<Msg> {
        let gesture = self.sidebar.reorder.take();
        let final_cursor_y = self.sidebar.reorder_cursor_y;
        self.sidebar.reorder_cursor_y = 0.0;

        let Some((from, start_y)) = gesture else {
            return Task::none();
        };

        let total_movement = (final_cursor_y - start_y).abs();
        let is_click = start_y == 0.0
            || total_movement < sola_kit::components::PANEL_REORDER_THRESHOLD;

        let ids = self.tabs.tab_ids_in_order();
        if is_click {
            if let Some(id) = ids.get(from) {
                return self.select_tab(&id.clone());
            }
            return Task::none();
        }

        let n = ids.len();
        if n == 0 {
            return Task::none();
        }
        let to = sola_kit::components::panel_drop_index(
            final_cursor_y,
            sola_kit::components::PANEL_HEADER_H,
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
