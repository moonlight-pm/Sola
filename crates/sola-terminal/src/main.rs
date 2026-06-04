use std::collections::{HashMap, HashSet};
use std::time::Duration;
use std::sync::Arc;

use iced::widget::{Space, canvas, container, mouse_area, row, stack, text};
use iced::{Element, Event, Length, Subscription, Task, Theme};
use iced::{event, keyboard, mouse};

use sola_bus::topics::{TerminalConfig, Topic, TopicKind};
use sola_bus::Message;
use sola_kit::app::{BusSetup, apply_theme_update, bus, bus_subscription, is_self_quit, startup, window_settings};
use sola_kit::fonts;
use sola_kit::theme::{Atoms, atoms_from_bus_theme, default_theme};

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

/// Milliseconds to wait after an Enter keypress before querying tmux for the
/// pane's current working directory. Gives the shell time to process the
/// command and update its cwd before we read it.
const CWD_REFRESH_MS: u64 = 150;

/// Slightly longer initial delay used when attaching a tab for the first time,
/// so the tmux pane is ready before we query its cwd.
const CWD_INITIAL_DELAY_MS: u64 = 300;

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

/// Parse a `"select_tab_{N}"` menu action id into a 0-based tab index.
///
/// Returns `Some(N)` when `action` has the prefix `"select_tab_"` followed by
/// a valid `usize`; returns `None` for anything else.
///
/// # Examples
/// ```
/// assert_eq!(parse_select_tab_action("select_tab_0"), Some(0));
/// assert_eq!(parse_select_tab_action("select_tab_3"), Some(3));
/// assert_eq!(parse_select_tab_action("new_tab"),      None);
/// assert_eq!(parse_select_tab_action("select_tab_"),  None);
/// assert_eq!(parse_select_tab_action("select_tab_x"), None);
/// ```
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

    // --- parse_select_tab_action ---

    #[test]
    fn parse_select_tab_action_tab_zero() {
        assert_eq!(parse_select_tab_action("select_tab_0"), Some(0));
    }

    #[test]
    fn parse_select_tab_action_tab_three() {
        // menu labels Tab 4 as index 3
        assert_eq!(parse_select_tab_action("select_tab_3"), Some(3));
    }

    #[test]
    fn parse_select_tab_action_tab_large_index() {
        assert_eq!(parse_select_tab_action("select_tab_99"), Some(99));
    }

    #[test]
    fn parse_select_tab_action_unrelated_action() {
        assert_eq!(parse_select_tab_action("new_tab"), None);
    }

    #[test]
    fn parse_select_tab_action_close_tab() {
        assert_eq!(parse_select_tab_action("close_tab"), None);
    }

    #[test]
    fn parse_select_tab_action_empty_suffix() {
        // "select_tab_" with no digits is invalid
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

    #[test]
    fn parse_select_tab_action_out_of_range_at_callsite() {
        // index 999 parses fine; the caller guards with ids.get(index)
        assert_eq!(parse_select_tab_action("select_tab_999"), Some(999));
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
    /// Runtime-only window title cache: tab_id → most-recent OSC 0/2 title.
    /// Cleared on tab close. Not persisted — titles come from the running
    /// shell and need no bus round-trip.
    titles: HashMap<String, String>,
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    PtyOutput(String),
    PtyExit(String),
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
    /// The canvas mutated `term.selection` (drag start/extend/clear). The
    /// handler just clears the geometry cache so the highlight re-renders.
    SelectionChanged,
    /// Clipboard read completed (Ctrl+Shift+V or menu "paste"). `Some` carries
    /// the text to paste into the active PTY; `None` is an empty clipboard.
    Pasted(Option<String>),
    /// OSC 0/2 title set by the shell/TUI in tab `tab_id`. Empty string means
    /// ResetTitle — fall back to "Terminal".
    Title(String, String),
    /// Result of an async tmux cwd query for `tab_id`. `None` means the pane
    /// was gone or the query failed; `Some(path)` is the new working directory.
    CwdResult(String, Option<String>),
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
            sidebar: sidebar::SidebarState::default(), // reorder_cursor_y defaults to 0.0
            term_cache: canvas::Cache::default(),
            // Start themed from the kit's default atoms (the same atoms
            // `default_theme()` is built from), not the hardcoded fallback, so
            // the grid matches the rest of Sola before the first bus theme
            // arrives. `Palette::default()` remains the pre-theme fallback.
            palette: term_view::Palette::from_kit_theme(&Atoms::default()),
            window_size: iced::Size::new(800.0, 480.0),
            metrics: term_view::CellMetrics::default(),
            grid: (DEFAULT_COLS, DEFAULT_ROWS),
            titles: HashMap::new(),
        };
        (app, Task::none())
    }

    fn title(&self) -> String {
        self.active
            .as_ref()
            .and_then(|id| self.titles.get(id))
            .filter(|t| !t.is_empty())
            .cloned()
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
            // Task 2.6: drive Msg::Resized whenever the window changes size.
            // iced 0.14 API: `resize_events() -> Subscription<(Id, Size)>`.
            iced::window::resize_events().map(|(_id, size)| Msg::Resized(size)),
            // Single always-on global cursor + release listener shared by both
            // the divider-drag and tab-reorder gestures.  A no-op match fires on
            // every cursor sample when neither gesture is active — same pattern as
            // sola-monitor's DividerPress / CursorMoved / CursorReleased.
            //
            // CursorMoved carries both x (for divider drag) and y (for reorder).
            // We emit SidebarDragMove(x) and ReorderMove(y) from every move; the
            // update handlers gate on the respective active-flag so only the live
            // gesture actually processes the value.
            event::listen_with(|ev, _, _| match ev {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    // We need to emit a message for both possible active gestures.
                    // iced subscriptions can only return one message per event, so
                    // we emit SidebarDragMove(x) here (which also carries y via a
                    // dedicated field).  To avoid a second subscription, we encode
                    // both coordinates in a single variant.  Since the existing
                    // SidebarDragMove only carries x, we use ReorderMove(y) for
                    // reorder — the update arm for SidebarDragMove ignores the y
                    // and the ReorderMove arm ignores the x.  Both arms fire on
                    // every cursor sample; the active-flag guards ensure only one
                    // gesture does real work per sample.
                    //
                    // Implementation: produce TWO events by returning a batch.
                    // iced 0.14 listen_with returns Option<Msg>; to fire two
                    // messages we wrap them in a compound variant.  Instead, we
                    // pick the active gesture at subscription time using a shared
                    // flag — but the closure is `Fn` with no capture of &self.
                    //
                    // Simplest correct approach: always emit SidebarDragMove (x)
                    // AND ReorderMove (y).  We do this by returning a single
                    // compound Msg::CursorMoved variant that carries both.  Since
                    // Msg has no such variant, we use the two existing single-coord
                    // variants and return ReorderMove(y) (the update handler for
                    // SidebarDragMove also checks self.sidebar.dragging_divider so
                    // we still need to emit that variant).  The only way with a
                    // single return is to add a compound variant or to fire two
                    // subscriptions — but two subscriptions racing the same event
                    // source is fragile.
                    //
                    // Chosen approach: one subscription, one variant per event.
                    // CursorMoved emits SidebarDragMove(x).  A *second*
                    // event::listen_with emits ReorderMove(y).  The two subscriptions
                    // are independent (different Msg variants, no shared state in the
                    // closure) — iced merges them safely.  Each update arm is already
                    // guarded by its active flag so exactly one does real work per
                    // frame. (Comment retained to document the decision.)
                    Some(Msg::SidebarDragMove(position.x))
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Msg::SidebarDragEnd)
                }
                _ => None,
            }),
            // Second always-on listener for reorder cursor-y + release.
            // Independent of the divider-drag listener; no shared mutable state
            // in the closures, so iced can safely run both.
            event::listen_with(|ev, _, _| match ev {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Msg::ReorderMove(position.y))
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Msg::ReorderEnd)
                }
                _ => None,
            }),
        ])
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => self.on_bus(&m),
            Msg::NewTab => self.new_tab(),
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
            // The canvas just changed `term.selection`; drop the cached
            // geometry so the next `view` repaints the highlight.
            Msg::SelectionChanged => {
                self.term_cache.clear();
                Task::none()
            }
            Msg::Pasted(text) => self.on_pasted(text),
            Msg::ToggleCollapse => {
                self.config.sidebar_collapsed = !self.config.sidebar_collapsed;
                // Persist the updated config on the bus (sticky — survives
                // restarts). Emit before reflow so any observer sees the new
                // state together with the grid change.
                if let Ok(mut client) = bus().lock() {
                    if let Err(e) = client.emit(Topic::TerminalConfig(self.config.clone())) {
                        tracing::warn!("ToggleCollapse: emit TerminalConfig failed: {e:?}");
                    }
                }
                self.reflow_grid();
                Task::none()
            }
            Msg::SidebarDragStart => {
                self.sidebar.dragging_divider = true;
                // Anchor will be captured on the first SidebarDragMove, using
                // the cursor x at that moment + the current sidebar width.
                // This mirrors sola-monitor's DividerPress + last_cursor_x
                // pattern: press sets the flag, the first cursor-move event
                // (which carries the position) captures the anchor.
                self.sidebar.drag_anchor = None;
                Task::none()
            }
            Msg::SidebarDragMove(cursor_x) => {
                if self.sidebar.dragging_divider {
                    if let Some((anchor_x, anchor_w)) = self.sidebar.drag_anchor {
                        let new_w = sidebar::dragged_width(anchor_x, anchor_w, cursor_x);
                        self.config.sidebar_width = new_w as u32;
                        self.reflow_grid();
                    } else {
                        // First move after press: capture the anchor.
                        let current_w = self.config.sidebar_width as f32;
                        self.sidebar.drag_anchor = Some((cursor_x, current_w));
                    }
                }
                Task::none()
            }
            Msg::SidebarDragEnd => {
                if self.sidebar.dragging_divider {
                    self.sidebar.dragging_divider = false;
                    self.sidebar.drag_anchor = None;
                    // Persist the final width once on release (not on every move).
                    if let Ok(mut client) = bus().lock() {
                        if let Err(e) = client.emit(Topic::TerminalConfig(self.config.clone())) {
                            tracing::warn!("SidebarDragEnd: emit TerminalConfig failed: {e:?}");
                        }
                    }
                    self.reflow_grid();
                }
                Task::none()
            }
            Msg::ReorderStart(index) => {
                // A press on tab `index` begins a potential reorder gesture.
                // We record the start-index and leave start_y = 0.0 (sentinel);
                // the actual y is captured on the first ReorderMove, mirroring
                // the divider's anchor-on-first-move pattern.
                self.sidebar.reorder = Some((index, 0.0));
                self.sidebar.reorder_cursor_y = 0.0;
                Task::none()
            }
            Msg::ReorderMove(cursor_y) => {
                if let Some((from, ref mut start_y)) = self.sidebar.reorder {
                    // Capture anchor on first move (start_y == 0.0 sentinel).
                    if *start_y == 0.0 {
                        *start_y = cursor_y;
                    }
                    let _ = from; // used in ReorderEnd
                    self.sidebar.reorder_cursor_y = cursor_y;
                }
                Task::none()
            }
            Msg::ReorderEnd => {
                let gesture = self.sidebar.reorder.take();
                // Note: reorder_cursor_y holds the last cursor position from
                // ReorderMove. We read it before clearing.
                let final_cursor_y = self.sidebar.reorder_cursor_y;
                self.sidebar.reorder_cursor_y = 0.0;

                let Some((from, start_y)) = gesture else {
                    return Task::none();
                };

                let total_movement = (final_cursor_y - start_y).abs();
                // If start_y is still 0.0 (anchor never captured — no move
                // events fired between press and release), treat as a click.
                let is_click = start_y == 0.0 || total_movement < sidebar::REORDER_THRESHOLD;

                if is_click {
                    // Small/no movement → treat as SelectTab.
                    let ids = self.tabs.ids_in_order();
                    if let Some(id) = ids.get(from) {
                        return self.select_tab(&id.clone());
                    }
                    return Task::none();
                }

                // Real reorder: compute drop slot and renumber ordinals.
                let ids = self.tabs.ids_in_order();
                let n = ids.len();
                if n == 0 {
                    return Task::none();
                }

                let to = sidebar::drop_index(
                    final_cursor_y,
                    sidebar::SIDEBAR_HEADER_H,
                    sidebar::TAB_ROW_H,
                    n,
                );

                if from == to {
                    // Landed back on the same slot — no-op (no bus emit needed).
                    return Task::none();
                }

                let new_order = sidebar::reordered(&ids, from, to);

                // Collect current ordinals for the changed-pairs computation.
                let meta_ordinals: std::collections::HashMap<String, u32> = ids
                    .iter()
                    .filter_map(|id| {
                        self.tabs.get(id).map(|m| (id.clone(), m.ordinal))
                    })
                    .collect();

                let changed = sidebar::renumber_changed(&meta_ordinals, &new_order);

                // Apply ordinal updates locally and emit one TerminalSession per change.
                for (id, new_ordinal) in &changed {
                    if let Some(meta) = self.tabs.get(id).cloned() {
                        self.tabs.upsert_meta(state::TabMeta {
                            id: id.clone(),
                            tmux_session: meta.tmux_session.clone(),
                            cwd: meta.cwd.clone(),
                            ordinal: *new_ordinal,
                        });
                        if let Ok(mut client) = bus().lock() {
                            if let Err(e) = client.emit(Topic::TerminalSession(
                                sola_bus::topics::TerminalSession {
                                    id: id.clone(),
                                    tmux_session: meta.tmux_session,
                                    cwd: meta.cwd,
                                    ordinal: *new_ordinal,
                                },
                            )) {
                                tracing::warn!("ReorderEnd: emit TerminalSession failed: {e:?}");
                            }
                        }
                    }
                }

                // Republish the menu so tab numbers reflect the new order.
                if !changed.is_empty() {
                    self.republish_menu();
                }

                Task::none()
            }
            Msg::Title(tab_id, title) => {
                // Store for the active-tab window-title lookup in `title()`.
                // Stale entries for closed tabs are cleaned up in `close_tab`.
                self.titles.insert(tab_id, title);
                Task::none()
            }
            Msg::CwdResult(tab_id, path_opt) => {
                let Some(path) = path_opt else {
                    return Task::none();
                };
                // Only update if the path changed to avoid spurious bus emits.
                let current_cwd = self.tabs.get(&tab_id).and_then(|m| m.cwd.clone());
                if current_cwd.as_deref() == Some(path.as_str()) {
                    return Task::none();
                }
                // Tab may have been closed between the Task spawn and its completion.
                let Some(meta) = self.tabs.get(&tab_id).cloned() else {
                    return Task::none();
                };
                let updated = state::TabMeta {
                    id: meta.id.clone(),
                    tmux_session: meta.tmux_session.clone(),
                    cwd: Some(path.clone()),
                    ordinal: meta.ordinal,
                };
                self.tabs.upsert_meta(updated);
                // Re-emit the TerminalSession so the sidebar label updates and
                // the new cwd survives a restart. A re-emit of an already-tracked
                // tab is admitted (was_present=true in on_bus → Admit::Yes), so
                // this never triggers retraction.
                if let Ok(mut client) = bus().lock() {
                    if let Err(e) = client.emit(Topic::TerminalSession(
                        sola_bus::topics::TerminalSession {
                            id: meta.id.clone(),
                            tmux_session: meta.tmux_session,
                            cwd: Some(path),
                            ordinal: meta.ordinal,
                        },
                    )) {
                        tracing::warn!(
                            id = %meta.id,
                            "CwdResult: emit TerminalSession failed: {e:?}"
                        );
                    }
                }
                Task::none()
            }
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
                    on_select: Msg::SelectionChanged,
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

        // 6px-wide draggable divider between sidebar and pane.
        // Emits SidebarDragStart on press; cursor-move and release are caught
        // by the always-on event::listen_with subscription (same pattern as
        // sola-monitor's divider drag). Hidden when sidebar is collapsed.
        let divider: Element<'_, Msg> = if self.config.sidebar_collapsed {
            Space::new().into()
        } else {
            mouse_area(
                container(Space::new())
                    .width(Length::Fixed(6.0))
                    .height(Length::Fill),
            )
            .interaction(mouse::Interaction::ResizingColumn)
            .on_press(Msg::SidebarDragStart)
            .into()
        };

        let body: Element<'_, Msg> = row![
            sidebar::view(&self.sidebar, &self.tabs, self.active.as_deref(), &self.config),
            divider,
            pane,
        ]
        .into();

        // Transparent full-window overlay during drag, identical to monitor's
        // technique: keeps the ResizingColumn cursor while the pointer races
        // ahead of the lagging divider widget. Without this overlay, iced's
        // per-frame hit-test crosses into the pane and snaps back to the
        // default cursor, causing flicker.
        if self.sidebar.dragging_divider {
            stack![
                body,
                mouse_area(Space::new())
                    .interaction(mouse::Interaction::ResizingColumn),
            ]
            .into()
        } else {
            body
        }
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

        // Copy/paste shortcuts come FIRST, before any PTY encoding. Only the
        // +Shift variants are intercepted: plain Ctrl+C must still reach the
        // PTY as 0x03 (SIGINT) and plain Ctrl+V as 0x16, so we require BOTH
        // control and shift here. Match on the logical character so layout and
        // case ('c'/'C') don't matter.
        if modifiers.control() && modifiers.shift() {
            if let keyboard::Key::Character(s) = &key {
                match s.chars().next().map(|c| c.to_ascii_lowercase()) {
                    Some('c') => return self.copy_selection(),
                    Some('v') => return self.paste(),
                    _ => {}
                }
            }
        }

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
            // Detect Enter (carriage return) to schedule a cwd refresh.
            // The shell may `cd` on this keypress; we query tmux ~150ms later
            // so the shell has time to execute the command before we read cwd.
            let is_enter = bytes == [b'\r'];
            rt.backend.write(&bytes);
            if is_enter {
                if let Some(session) = self.tabs.get(&active).map(|m| m.tmux_session.clone()) {
                    let tab_id = active.clone();
                    return Task::perform(
                        async move {
                            tokio::time::sleep(Duration::from_millis(CWD_REFRESH_MS)).await;
                            tokio::task::spawn_blocking(move || tmux::pane_current_path(&session))
                                .await
                                .ok()
                                .flatten()
                        },
                        move |path| Msg::CwdResult(tab_id, path),
                    );
                }
            }
        }
        Task::none()
    }

    /// Copy the active tab's terminal selection to the clipboard.
    ///
    /// Locks the term briefly, asks alacritty for the selection text
    /// (`selection_to_string`), and writes non-empty text to the system
    /// clipboard via iced. No selection (or empty) → no-op.
    fn copy_selection(&self) -> Task<Msg> {
        let Some(active) = self.active.as_deref() else {
            return Task::none();
        };
        let Some(rt) = self.tabs.runtime(active) else {
            return Task::none();
        };
        let text = { rt.emulator.term().lock().selection_to_string() };
        match text {
            Some(s) if !s.is_empty() => iced::clipboard::write::<Msg>(s),
            _ => Task::none(),
        }
    }

    /// Read the system clipboard, routing the result to `Msg::Pasted`.
    ///
    /// The actual write to the PTY happens in `on_pasted` once the async read
    /// resolves, so paste honours the term's bracketed-paste mode at that time.
    fn paste(&self) -> Task<Msg> {
        iced::clipboard::read().map(Msg::Pasted)
    }

    /// Write pasted clipboard text to the active PTY.
    ///
    /// Reads the term mode (briefly locked) so `input::paste` can wrap the text
    /// in bracketed-paste markers when the application requested it and
    /// normalise newlines. `None` (empty clipboard) is a no-op.
    fn on_pasted(&mut self, text: Option<String>) -> Task<Msg> {
        let Some(text) = text else {
            return Task::none();
        };
        let Some(active) = self.active.clone() else {
            return Task::none();
        };
        let Some(rt) = self.tabs.runtime(&active) else {
            return Task::none();
        };
        let mode = { *rt.emulator.term().lock().mode() };
        let bytes = input::paste(&text, mode);
        rt.backend.write(&bytes);
        Task::none()
    }

    /// Handle a window resize event (Task 2.6).
    ///
    /// Updates `self.window_size` and delegates to `reflow_grid`.
    fn on_resized(&mut self, size: iced::Size) -> Task<Msg> {
        self.window_size = size;
        self.reflow_grid();
        Task::none()
    }

    /// Recompute the terminal grid dimensions from the current `window_size`,
    /// `config` (sidebar width / collapsed state), and `metrics`, then drive
    /// the new dimensions into every live tab.
    ///
    /// Called from `on_resized`, `ToggleCollapse`, and the drag-end/drag-move
    /// handlers so the terminal tracks any change that affects the pane area.
    /// Returns immediately without touching tabs when the grid dimensions have
    /// not changed (avoids spurious TIOCSWINSZ churn).
    fn reflow_grid(&mut self) {
        let sidebar_w = if self.config.sidebar_collapsed {
            36.0_f32
        } else {
            self.config.sidebar_width as f32
        };
        let pane = pane_size(self.window_size, sidebar_w);
        let (cols, rows) = term_view::cols_rows_for(pane, self.metrics);

        if (cols, rows) == self.grid {
            return; // no change — avoid churn
        }
        self.grid = (cols, rows);

        for rt in self.tabs.runtimes_mut() {
            rt.emulator.resize(cols, rows);
            rt.backend.resize(cols, rows);
            rt.backend.sigwinch();
        }

        // Geometry changed — invalidate the cached canvas so the next frame
        // repaints at the correct grid size.
        self.term_cache.clear();
    }

    fn on_bus(&mut self, m: &Message) -> Task<Msg> {
        // 1. Live theme reload. `apply_theme_update` rebuilds the iced widget
        //    theme (`self.theme`) and installs the font role table. We also
        //    drive the terminal grid's colours (`self.palette`) from the SAME
        //    bus theme, so chrome and grid update together from one message.
        //    Parse the topic ourselves to read the bus `Theme` for the palette;
        //    `apply_theme_update` handles the iced-side rebuild. Both consume
        //    only `Topic::Theme`, so there is no double-handling.
        if apply_theme_update(m, &mut self.theme) {
            if let Some(Topic::Theme(bus)) = Topic::parse(m) {
                self.palette =
                    term_view::Palette::from_kit_theme(&atoms_from_bus_theme(&bus));
                // Re-render the grid with the new colours.
                self.term_cache.clear();
            }
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

                // Was this tab already in our table before this delivery?
                // - true  → our own echo from new_tab / a re-emit (e.g. cwd update):
                //           skip boot-reconcile, just update meta, no double-attach.
                // - false → first time we see this id (boot replay of persisted tabs):
                //           run reconciliation against the boot tmux snapshot.
                let was_present = self.tabs.get(&s.id).is_some();

                match session::admit_session(
                    was_present,
                    &self.live_tmux_at_startup,
                    &s.tmux_session,
                ) {
                    session::Admit::Retract => {
                        // Only reachable when !was_present and tmux session is
                        // gone — a persisted tab whose tmux died while offline.
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
                        if !was_present && was_empty {
                            // Boot replay: attach the first (and initially only)
                            // persisted tab, seeding its tmux scrollback history.
                            return self.attach_tab(&s.id, true);
                        }
                        // was_present → already attached (our own echo or re-emit).
                        // !was_present && !was_empty → additional boot-replay tab;
                        //   lazy-attach handled by Task 3.1 (not yet implemented).
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

    /// Open (or reattach) the PTY for `id`, optionally seed tmux scrollback into
    /// the grid, and start the reader thread.
    ///
    /// `seed_scrollback` should be `true` when reattaching an existing tmux
    /// session (boot replay — show history) and `false` when creating a fresh
    /// tab (nothing to seed; the capture would fail with a WARN).
    ///
    /// Grid size is taken from `self.grid` (updated by Task 2.6 resize
    /// plumbing). Scrollback authority (OPEN QUESTION #2): the
    /// alacritty `Grid` is the live viewport + local history; tmux is the
    /// persistence layer. The captured scrollback is a ONE-SHOT seed fed before
    /// the reader thread starts, so reattach shows history without racing live
    /// output. It is not re-synced afterward — the grid is authoritative once
    /// live.
    fn attach_tab(&mut self, id: &str, seed_scrollback: bool) -> Task<Msg> {
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
            emulator::title_sender(),
        );
        let em = emulator::Emulator::new(cols, rows, listener);
        let term = em.term();

        // Seed tmux scrollback into the grid BEFORE the reader thread starts,
        // so history shows on reattach without racing live output. Drive a
        // one-shot Processor over the shared term handle.
        // Skip for fresh tabs — they have no scrollback yet and capture-pane
        // would fail with a warning.
        if seed_scrollback {
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

        // Initial cwd fetch: query tmux shortly after attach so the tab label
        // shows the working directory without needing the user to press Enter.
        // A slightly longer delay than CWD_REFRESH_MS gives the pty time to
        // finish attaching before we query.
        let tab_id = id.to_string();
        let session = meta.tmux_session.clone();
        Task::perform(
            async move {
                tokio::time::sleep(Duration::from_millis(CWD_INITIAL_DELAY_MS)).await;
                tokio::task::spawn_blocking(move || tmux::pane_current_path(&session))
                    .await
                    .ok()
                    .flatten()
            },
            move |path| Msg::CwdResult(tab_id, path),
        )
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
        self.attach_tab(&id, false)
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
        self.titles.remove(id);
        self.republish_menu();
        Task::none()
    }

    /// Handle a menu action from the bus.
    ///
    /// Action-id strings (from `menu.rs`):
    ///   - `"new_tab"`           — open a new tab
    ///   - `"close_tab"`         — close the currently active tab
    ///   - `"select_tab_{N}"`    — select the tab at 0-based index N in
    ///                             `ids_in_order()` (e.g. `"select_tab_0"` = Tab 1)
    ///   - `"copy"` / `"paste"`  — clipboard, Task 4.1
    ///   - everything else       — ignored
    fn on_menu_action(&mut self, action: &str) -> Task<Msg> {
        match action {
            "new_tab" => self.new_tab(),
            "close_tab" => {
                if let Some(id) = self.active.clone() {
                    self.close_tab(&id)
                } else {
                    Task::none()
                }
            }
            "copy" => self.copy_selection(),
            "paste" => self.paste(),
            other => {
                if let Some(index) = parse_select_tab_action(other) {
                    let ids = self.tabs.ids_in_order();
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

    /// Switch the active tab to `id`.
    ///
    /// - Ignores stale/unknown ids.
    /// - Clears `term_cache` so the next `view` re-renders for the new tab
    ///   (cache holds geometry for the *previously* active tab).
    /// - Lazy-attaches meta-only tabs (boot-replayed tabs that have never been
    ///   attached in this session) by calling `attach_tab(id, true)`.
    fn select_tab(&mut self, id: &str) -> Task<Msg> {
        if self.tabs.get(id).is_none() {
            return Task::none();
        }

        self.active = Some(id.to_string());
        // Clear cached geometry — it belongs to whichever tab was active before.
        self.term_cache.clear();

        // Lazy-attach: if this tab has no runtime yet (boot-replayed from the
        // bus but never opened in this session) attach it now.
        // seed_scrollback=true because it's an existing tmux session with history.
        if self.tabs.runtime(id).is_none() {
            self.attach_tab(id, true)
        } else {
            Task::none()
        }
    }
}
