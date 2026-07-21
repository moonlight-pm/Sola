//! Browser chrome: message type, layout constants, and the generic `App<E>`.
//!
//! `Msg` and the consts were stubbed out in Task 1 and are kept here. Task 2
//! adds `App<E>`, its constructor, and all update/view/subscription methods.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use iced::widget::{Shader, Space, column, container, mouse_area, row, stack};
use sola_kit::components::text_input::text_input;
use iced::{Element, Event, Length, Subscription, Task, event, mouse};
use sola_kit::components::{
    TabDescriptor, TabSize, horizontal_divider, toolbar_button, vertical_divider_with,
    vertical_tabs_sized,
};

use crate::engine::{Cmd, EditCmd, Engine, FrameSlot, NavCmd, TabId, TabInfo, TabsHandle};

pub const DEFAULT_URL: &str = "https://www.wikipedia.org";
/// A fresh blank tab (⌘T). Loaded as an empty page; the chrome shows an empty,
/// focused URL bar rather than the literal "about:blank".
pub const BLANK_URL: &str = "about:blank";
pub const VIEW_W: u32 = 1280;
pub const VIEW_H: u32 = 800;
pub const CHROME_HEIGHT: f32 = 46.0;
/// Tab sidebar width (logical px) — the value the draggable divider
/// edits, clamped to `[MIN, MAX]`.
pub const SIDEBAR_W_DEFAULT: f32 = 200.0;
pub const SIDEBAR_W_MIN: f32 = 120.0;
pub const SIDEBAR_W_MAX: f32 = 420.0;

#[derive(Debug, Clone)]
pub enum Msg {
    NewFrame,
    NavBack,
    NavForward,
    NavReload,
    UrlInput(String),
    UrlSubmit,
    CloseTab(TabId),
    ActivateTab(TabId),
    /// Timer tick — refresh `cached_tabs`/`cached_active` and
    /// sync `url_field` if the active tab's URL changed.
    Tick,
    /// A message delivered over the Sola bus (theme, open-url, menu
    /// action, close-app). Handled by `integration::handle_bus`.
    Bus(Arc<sola_bus::Message>),
    /// User pressed the mouse on the sidebar divider.
    DividerPress,
    /// Global cursor moved — only acted on while dragging the divider.
    CursorMoved(f32),
    /// Global left-button released — ends a divider drag.
    CursorReleased,
    /// Hovered tab row changed (index into `cached_tabs`), or `None`.
    TabHover(Option<usize>),
    /// A left button press landed inside the web view — the page took
    /// keyboard focus, so edit commands route to the engine (not the URL bar).
    WebViewFocused,
    /// A global left press — triggers a URL-bar focus query so we can
    /// select-all when the field has just gained focus (browser behavior).
    LeftPressed,
    /// Result of the focus query started by [`Msg::LeftPressed`]: whether the
    /// URL bar currently holds focus. Selects-all on the false→true edge.
    UrlBarFocusSync(bool),
    /// Result of the live focus query for an Edit action (⌘C/⌘X/⌘V/⌘A):
    /// route `cmd` to the URL bar when `url_bar_focused`, else the engine.
    EditRouted { cmd: EditCmd, url_bar_focused: bool },
    /// Result of an `iced::clipboard::read` kicked off by a URL-bar paste.
    UrlPasted(Option<String>),
}

/// Browser chrome application state, generic over the web engine.
///
/// `engine` owns the worker and keeps it alive for the lifetime of the
/// process — no process-wide statics needed.
pub struct App<E: Engine> {
    /// The running web engine. Kept alive here so neither a static nor an
    /// `Arc` is needed to keep the worker alive.
    pub engine: E,
    pub slot: Arc<FrameSlot<E>>,
    pub releaser: Sender<Cmd<E>>,
    /// Live tab snapshot, owned by the engine. We re-read on
    /// every Tick; `cached_tabs` is the value at last read.
    pub tabs_handle: TabsHandle,
    /// Atomic id of the currently-active tab. Mirror cached in
    /// `cached_active`.
    pub active_handle: Arc<AtomicU64>,
    /// Snapshot of tabs as of the last Tick — view() and
    /// subscription helpers read from here so they don't have to
    /// re-lock the engine's Mutex on every frame.
    pub cached_tabs: Vec<TabInfo>,
    pub cached_active: TabId,
    /// Editable contents of the URL bar.
    pub url_field: String,
    /// The URL we last copied from the engine into `url_field`,
    /// so we only overwrite on actual change.
    pub last_seen_url: String,
    /// Active iced theme, refreshed live from `Topic::Theme` so the
    /// chrome (tab strip, URL bar, buttons) tracks the system theme.
    pub theme: iced::Theme,
    /// Tab sidebar width; edited by the draggable divider.
    pub sidebar_w: f32,
    /// True while the divider is being dragged.
    pub dragging_divider: bool,
    /// Most-recent global cursor x, tracked continuously so the drag
    /// anchor is current at `DividerPress` time.
    pub last_cursor_x: Option<f32>,
    /// `(cursor_x_at_press, sidebar_w_at_press)` — anchor-relative drag
    /// (recompute from displacement, never accumulate deltas).
    pub drag_anchor: Option<(f32, f32)>,
    /// Index of the hovered tab row, if any — drives the float-in close
    /// button. Recomputed from `mouse_area` enter/exit each frame.
    pub hovered_tab: Option<usize>,
    /// The app_id string passed to `run::<E>`, stored so `Msg::Bus` can
    /// forward it to `integration::handle_bus` without a static.
    pub app_id: &'static str,
    /// True when the chrome URL bar holds keyboard focus, so `Edit`
    /// commands target it instead of the web content. Set by ⌘L / ⌘T /
    /// typing in the bar; cleared when a press lands in the web view.
    /// Best-effort heuristic — see the design spec's documented edge case.
    pub url_bar_focused: bool,
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
        releaser: Sender<Cmd<E>>,
        tabs_handle: TabsHandle,
        active_handle: Arc<AtomicU64>,
        url: String,
        app_id: &'static str,
    ) -> Self {
        let cached_active = TabId(active_handle.load(Ordering::Relaxed));
        Self {
            engine,
            slot,
            releaser,
            tabs_handle,
            active_handle,
            cached_tabs: Vec::new(),
            cached_active,
            url_field: url.clone(),
            last_seen_url: url,
            theme: sola_kit::theme::default_theme(),
            sidebar_w: SIDEBAR_W_DEFAULT,
            dragging_divider: false,
            last_cursor_x: None,
            drag_anchor: None,
            hovered_tab: None,
            app_id,
            url_bar_focused: false,
        }
    }

    pub fn active_tab_info(&self) -> Option<&TabInfo> {
        self.cached_tabs.iter().find(|t| t.id == self.cached_active)
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::NewFrame => {}
            Msg::NavBack => {
                let _ = self.releaser.send(Cmd::Nav(NavCmd::Back));
            }
            Msg::NavForward => {
                let _ = self.releaser.send(Cmd::Nav(NavCmd::Forward));
            }
            Msg::NavReload => {
                let _ = self.releaser.send(Cmd::Nav(NavCmd::Reload));
            }
            Msg::UrlInput(s) => {
                self.url_field = s;
                self.url_bar_focused = true;
            }
            Msg::UrlSubmit => {
                // A URL navigates directly; anything else is searched on Kagi.
                let url = crate::util::resolve_query(&self.url_field);
                if url.is_empty() {
                    return Task::none();
                }
                self.url_field = url.clone();
                self.last_seen_url = url.clone();
                let _ = self.releaser.send(Cmd::Nav(NavCmd::LoadUrl(url)));
            }
            Msg::CloseTab(id) => {
                // If closing the active tab, pick a new active tab
                // first so the engine never sees `active` pointing
                // at a closed tab.
                let was_active = self.cached_active == id;
                if was_active {
                    if let Some(new_active) = self.pick_new_active_after_close(id) {
                        let _ = self.releaser.send(Cmd::SetActiveTab(new_active));
                        self.cached_active = new_active;
                        self.active_handle.store(new_active.0, Ordering::Relaxed);
                    }
                }
                let _ = self.releaser.send(Cmd::CloseTab(id));
            }
            Msg::ActivateTab(id) => {
                let _ = self.releaser.send(Cmd::SetActiveTab(id));
                self.cached_active = id;
                self.active_handle.store(id.0, Ordering::Relaxed);
                // Force the URL bar to immediately reflect the new
                // active tab's url on the next Tick (not on next
                // engine URL change).
                self.last_seen_url.clear();
            }
            Msg::Tick => {
                self.cached_tabs = self.tabs_handle.lock().unwrap().clone();
                let engine_active = TabId(self.active_handle.load(Ordering::Relaxed));
                self.cached_active = engine_active;
                let active_url = self.active_tab_info().map(|t| t.url.clone());
                if let Some(url) = active_url {
                    if url != self.last_seen_url {
                        self.last_seen_url = url.clone();
                        // A blank tab shows an empty URL bar, not "about:blank".
                        self.url_field = if url == BLANK_URL { String::new() } else { url };
                    }
                }
                // Drain any page-selection text the engine extracted for a copy
                // and put it on the system clipboard via iced. The engine's own
                // clipboard can't reach Wayland (headless display); iced's can.
                if let Some(text) = self.engine.clipboard_handle().lock().unwrap().take() {
                    tracing::debug!(len = text.len(), "draining page selection → system clipboard");
                    return iced::clipboard::write(text);
                }
            }
            Msg::Bus(message) => {
                return crate::integration::handle_bus(self, message, self.app_id);
            }
            Msg::DividerPress => {
                self.dragging_divider = true;
                if let Some(x) = self.last_cursor_x {
                    self.drag_anchor = Some((x, self.sidebar_w));
                }
            }
            Msg::CursorMoved(x) => {
                self.last_cursor_x = Some(x);
                if self.dragging_divider {
                    if let Some((anchor_x, anchor_w)) = self.drag_anchor {
                        // Sidebar is on the LEFT: it grows as the cursor
                        // moves right of the anchor, shrinks moving left.
                        let desired = anchor_w + (x - anchor_x);
                        self.sidebar_w = desired.clamp(SIDEBAR_W_MIN, SIDEBAR_W_MAX);
                    }
                }
            }
            Msg::CursorReleased => {
                if self.dragging_divider {
                    self.dragging_divider = false;
                    self.drag_anchor = None;
                }
            }
            Msg::TabHover(i) => self.hovered_tab = i,
            Msg::WebViewFocused => self.url_bar_focused = false,
            Msg::LeftPressed => {
                // A press landed somewhere. Resolve, against the real widget
                // tree, whether it focused the URL bar — `text_input` captures
                // the click so no wrapper can tell us directly.
                return crate::integration::url_bar_is_focused(Msg::UrlBarFocusSync);
            }
            Msg::UrlBarFocusSync(now) => {
                // Select-all only on the false→true edge, so a second click in
                // an already-focused field can place the caret normally.
                let gained = now && !self.url_bar_focused;
                self.url_bar_focused = now;
                if gained {
                    return crate::integration::select_url_bar();
                }
            }
            Msg::EditRouted { cmd, url_bar_focused } => {
                if url_bar_focused {
                    tracing::debug!(?cmd, "edit → URL bar (iced clipboard)");
                    return match cmd {
                        EditCmd::Copy => iced::clipboard::write(self.url_field.clone()),
                        EditCmd::Cut => {
                            let task = iced::clipboard::write(self.url_field.clone());
                            self.url_field.clear();
                            task
                        }
                        EditCmd::Paste => iced::clipboard::read().map(Msg::UrlPasted),
                        EditCmd::SelectAll => crate::integration::select_url_bar(),
                        // The URL bar has no app-level undo/redo stack.
                        EditCmd::Undo | EditCmd::Redo => Task::none(),
                    };
                }
                tracing::debug!(?cmd, "edit → engine (web content)");
                let _ = self.releaser.send(Cmd::Edit(cmd));
            }
            Msg::UrlPasted(text) => {
                if let Some(s) = text {
                    // Best-effort: iced exposes no caret/selection, so append
                    // at the end (cursor-at-end assumption).
                    self.url_field.push_str(&s);
                }
            }
        }
        Task::none()
    }

    /// Open a new tab loading `url`, focusing it when `activate`. Called from
    /// app-menu intents (e.g., ⌘T for new tab) and bus-driven OpenUrl via
    /// `integration::run_intent`.
    pub fn open_tab(&mut self, url: String, activate: bool) {
        let url = crate::util::normalize_url(&url);
        let id = self.engine.alloc_tab_id();
        let _ = self.releaser.send(Cmd::OpenTab { id, url });
        if activate {
            let _ = self.releaser.send(Cmd::SetActiveTab(id));
            self.cached_active = id;
            self.active_handle.store(id.0, Ordering::Relaxed);
        }
    }

    /// Current iced theme (chrome styling), refreshed from `Topic::Theme`.
    pub fn theme(&self) -> iced::Theme {
        self.theme.clone()
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

    pub fn view(&self) -> Element<'_, Msg> {
        let webview = Shader::new(E::make_program(self.slot.clone()))
            .width(Length::Fill)
            .height(Length::Fill);

        // Right side: nav bar on top of the web content.
        let content = column![self.view_nav_bar(), horizontal_divider(), webview];

        // Left tab column (resizable) | divider | content.
        let main = row![
            container(self.view_tab_sidebar())
                .width(Length::Fixed(self.sidebar_w))
                .height(Length::Fill),
            vertical_divider_with(
                Msg::DividerPress,
                sola_kit::components::DividerColors::raised_to_canvas(&self.theme),
            ),
            container(content).width(Length::Fill).height(Length::Fill),
        ]
        .height(Length::Fill);

        let body: Element<'_, Msg> = main.into();

        // While dragging, a transparent top layer holds the resize
        // cursor steady even when the pointer races ahead of the divider.
        if self.dragging_divider {
            stack![
                body,
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .interaction(mouse::Interaction::ResizingColumn),
            ]
            .into()
        } else {
            body
        }
    }

    /// Left vertical tab column, built from the kit `vertical_tabs`
    /// component so it tracks the shared theme. Single-line labels (no
    /// wrap), active-row highlight, and a close `×` that floats in on
    /// hover. New tabs come from `⌘T` (the app-menu shortcut), so there's
    /// no in-column "+" button.
    pub fn view_tab_sidebar(&self) -> Element<'_, Msg> {
        let tabs: Vec<TabDescriptor<Msg>> = self
            .cached_tabs
            .iter()
            .map(|t| {
                let label = if !t.title.is_empty() {
                    crate::util::truncate(&t.title, 20)
                } else if !t.url.is_empty() {
                    crate::util::truncate(&t.url, 20)
                } else {
                    String::from("Loading…")
                };
                TabDescriptor::new(
                    label,
                    t.id == self.cached_active,
                    Msg::ActivateTab(t.id),
                    Msg::CloseTab(t.id),
                )
            })
            .collect();

        vertical_tabs_sized(tabs, self.hovered_tab, Msg::TabHover, TabSize::Large).into()
    }

    /// Top navigation bar: back / forward / reload + the URL field. All
    /// widgets are kit-styled, so they track the bus theme.
    ///
    /// The URL field isn't wrapped in a `mouse_area`: `text_input` captures
    /// the click to place its caret, and `mouse_area` skips `on_press` for
    /// captured events. Click-into-focus + select-all is handled instead via
    /// the global press subscription (`Msg::LeftPressed`) plus a live focus
    /// query, which sees the press regardless of widget capture.
    pub fn view_nav_bar(&self) -> Element<'_, Msg> {
        row![
            toolbar_button("←").on_press(Msg::NavBack),
            toolbar_button("→").on_press(Msg::NavForward),
            toolbar_button("↻").on_press(Msg::NavReload),
            text_input("Search or enter URL", &self.url_field)
                .id(crate::integration::url_input_id())
                .on_input(Msg::UrlInput)
                .on_submit(Msg::UrlSubmit)
                .size(15)
                .padding([8, 12])
                .width(Length::Fill)
                .style(sola_kit::components::text_input::style),
        ]
        .spacing(8)
        .padding([4, 10])
        .align_y(iced::Alignment::Center)
        .height(Length::Fixed(CHROME_HEIGHT))
        .into()
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        let frames = self.engine.frames();
        let slot = self.slot.clone();
        let active = self.active_handle.clone();
        Subscription::batch(vec![
            crate::run::frame_subscription::<E>(frames, slot, active),
            iced::time::every(Duration::from_millis(250)).map(|_| Msg::Tick),
            sola_kit::app::bus_subscription().map(Msg::Bus),
            event::listen_with(|event, _, _| match event {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Msg::CursorMoved(position.x))
                }
                // A left press anywhere: resolve whether it focused the URL bar
                // (for click-to-select-all). Received regardless of which widget
                // captures it, unlike a wrapping `mouse_area`.
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    Some(Msg::LeftPressed)
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Msg::CursorReleased)
                }
                _ => None,
            }),
        ])
    }
}
