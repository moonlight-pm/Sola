//! sola-browser-cef — CEF-backed browser with iced chrome + tabs.

use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

use iced::futures::SinkExt;
use iced::futures::Stream;
use iced::stream;
use iced::widget::{Shader, Space, column, container, mouse_area, row, stack, text_input};
use iced::{Element, Event, Length, Subscription, Task, event, mouse};

use sola_browser_cef::cef::{CefEngine, Cmd, NavCmd, TabId, TabInfo};
use sola_browser_cef::shader::{CefProgram, FrameSlot};
use sola_kit::components::{
    TabDescriptor, horizontal_divider, toolbar_button, vertical_divider, vertical_tabs,
};

mod integration;

const APP_ID: &str = "sola-browser-cef";
const DEFAULT_URL: &str = "https://slate.auto";
const VIEW_W: u32 = 1280;
const VIEW_H: u32 = 800;
/// Height of the top navigation bar (back/forward/reload + URL).
const CHROME_HEIGHT: f32 = 38.0;
/// Tab sidebar width (logical px) — the value the draggable divider
/// edits, clamped to `[MIN, MAX]`.
const SIDEBAR_W_DEFAULT: f32 = 200.0;
const SIDEBAR_W_MIN: f32 = 120.0;
const SIDEBAR_W_MAX: f32 = 420.0;

static ENGINE: OnceLock<CefEngine> = OnceLock::new();
static SLOT_FOR_STREAM: OnceLock<Arc<FrameSlot>> = OnceLock::new();
static ACTIVE_TAB_FOR_STREAM: OnceLock<Arc<AtomicU64>> = OnceLock::new();

fn main() -> ExitCode {
    // CEF subprocess gate — must run *before* logger init so renderer
    // / GPU / utility workers don't open the shared log file.
    if let Some(code) = CefEngine::dispatch_subprocess(APP_ID) {
        return code;
    }

    sola_core::log::init(APP_ID);
    tracing::info!("{APP_ID} starting");

    let _ = sola_core::env::activate_wayland_session(10_000);

    let url = std::env::args().nth(1).unwrap_or_else(|| DEFAULT_URL.to_string());
    tracing::info!(%url, "loading url");
    let engine = CefEngine::spawn(APP_ID, &url, VIEW_W, VIEW_H);
    let releaser = engine.cmd_sender();
    let tabs_handle = engine.tabs_handle();
    let active_handle = engine.active_tab_handle();
    let cursor = engine.cursor_handle();
    ENGINE.set(engine).map_err(|_| ()).expect("ENGINE set twice");

    let slot = Arc::new(FrameSlot {
        pending: Mutex::new(None),
        releaser: releaser.clone(),
        last_size: Mutex::new((VIEW_W, VIEW_H)),
        cursor,
    });
    SLOT_FOR_STREAM
        .set(slot.clone())
        .map_err(|_| ())
        .expect("SLOT_FOR_STREAM set twice");
    ACTIVE_TAB_FOR_STREAM
        .set(active_handle.clone())
        .map_err(|_| ())
        .expect("ACTIVE_TAB_FOR_STREAM set twice");

    // Join the Sola bus: subscribe to the topics we act on and publish the
    // "Browser" app-menu (which is also how the shell binds our shortcuts).
    // Connecting is best-effort — without a bus the browser still runs
    // standalone (no theme/menu/OpenUrl).
    sola_kit::app::BusSetup::new(APP_ID)
        .subscribe(integration::SUBSCRIBE)
        .app_menu("Browser", integration::MENU_ITEMS)
        .install();

    let result = iced::application(
        move || App {
            slot: slot.clone(),
            releaser: releaser.clone(),
            tabs_handle: tabs_handle.clone(),
            active_handle: active_handle.clone(),
            cached_tabs: Vec::new(),
            cached_active: TabId(active_handle.load(Ordering::Relaxed)),
            url_field: url.clone(),
            last_seen_url: url.clone(),
            theme: sola_kit::theme::default_theme(),
            sidebar_w: SIDEBAR_W_DEFAULT,
            dragging_divider: false,
            last_cursor_x: None,
            drag_anchor: None,
            hovered_tab: None,
        },
        App::update,
        App::view,
    )
    .title(|app: &App| match app.active_tab_info() {
        Some(t) if !t.title.is_empty() => format!("{APP_ID} — {}", t.title),
        Some(t) if !t.url.is_empty() => format!("{APP_ID} — {}", t.url),
        _ => APP_ID.into(),
    })
    .subscription(App::subscription)
    .theme(App::theme)
    .default_font(sola_kit::fonts::ui())
    .window(iced::window::Settings {
        decorations: false,
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: APP_ID.into(),
            ..Default::default()
        },
        ..iced::window::Settings::default()
    })
    .run();

    if let Err(e) = result {
        tracing::error!("iced::application returned: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

struct App {
    slot: Arc<FrameSlot>,
    releaser: Sender<Cmd>,
    tabs_handle: Arc<Mutex<Vec<TabInfo>>>,
    active_handle: Arc<AtomicU64>,
    cached_tabs: Vec<TabInfo>,
    cached_active: TabId,
    url_field: String,
    last_seen_url: String,
    /// Active iced theme, refreshed live from `Topic::Theme` so the
    /// chrome (tab strip, URL bar, buttons) tracks the system theme.
    theme: iced::Theme,
    /// Tab sidebar width; edited by the draggable divider.
    sidebar_w: f32,
    /// True while the divider is being dragged.
    dragging_divider: bool,
    /// Most-recent global cursor x, tracked continuously so the drag
    /// anchor is current at `DividerPress` time.
    last_cursor_x: Option<f32>,
    /// `(cursor_x_at_press, sidebar_w_at_press)` — anchor-relative drag
    /// (recompute from displacement, never accumulate deltas).
    drag_anchor: Option<(f32, f32)>,
    /// Index of the hovered tab row, if any — drives the float-in close
    /// button. Recomputed from `mouse_area` enter/exit each frame.
    hovered_tab: Option<usize>,
}

#[derive(Debug, Clone)]
enum Msg {
    NewFrame,
    NavBack,
    NavForward,
    NavReload,
    UrlInput(String),
    UrlSubmit,
    CloseTab(TabId),
    ActivateTab(TabId),
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
}

impl App {
    fn active_tab_info(&self) -> Option<&TabInfo> {
        self.cached_tabs.iter().find(|t| t.id == self.cached_active)
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
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
            Msg::UrlInput(s) => self.url_field = s,
            Msg::UrlSubmit => {
                let url = normalize_url(&self.url_field);
                self.url_field = url.clone();
                self.last_seen_url = url.clone();
                let _ = self.releaser.send(Cmd::Nav(NavCmd::LoadUrl(url)));
            }
            Msg::CloseTab(id) => {
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
                        self.url_field = url;
                    }
                }
            }
            Msg::Bus(message) => return integration::handle_bus(self, message),
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
        }
        Task::none()
    }

    /// Open a new tab loading `url`, focusing it when `activate`. Shared by
    /// the chrome "+" button (`Msg::OpenTab`) and bus-driven OpenUrl.
    fn open_tab(&mut self, url: String, activate: bool) {
        let url = normalize_url(&url);
        let id = ENGINE.get().expect("ENGINE").alloc_tab_id();
        let _ = self.releaser.send(Cmd::OpenTab { id, url });
        if activate {
            let _ = self.releaser.send(Cmd::SetActiveTab(id));
            self.cached_active = id;
            self.active_handle.store(id.0, Ordering::Relaxed);
        }
    }

    /// Current iced theme (chrome styling), refreshed from `Topic::Theme`.
    fn theme(&self) -> iced::Theme {
        self.theme.clone()
    }

    fn pick_new_active_after_close(&self, closing: TabId) -> Option<TabId> {
        let idx = self.cached_tabs.iter().position(|t| t.id == closing)?;
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

    fn view(&self) -> Element<'_, Msg> {
        let webview = Shader::new(CefProgram {
            slot: self.slot.clone(),
        })
        .width(Length::Fill)
        .height(Length::Fill);

        // Right side: nav bar on top of the web content.
        let content = column![self.view_nav_bar(), horizontal_divider(), webview];

        // Left tab column (resizable) | divider | content.
        let main = row![
            container(self.view_tab_sidebar())
                .width(Length::Fixed(self.sidebar_w))
                .height(Length::Fill),
            vertical_divider(Msg::DividerPress),
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
    fn view_tab_sidebar(&self) -> Element<'_, Msg> {
        let tabs: Vec<TabDescriptor<Msg>> = self
            .cached_tabs
            .iter()
            .map(|t| {
                let label = if !t.title.is_empty() {
                    truncate(&t.title, 20)
                } else if !t.url.is_empty() {
                    truncate(&t.url, 20)
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

        vertical_tabs(tabs, self.hovered_tab, Msg::TabHover).into()
    }

    /// Top navigation bar: back / forward / reload + the URL field. All
    /// widgets are kit-styled, so they track the bus theme.
    fn view_nav_bar(&self) -> Element<'_, Msg> {
        row![
            toolbar_button("←").on_press(Msg::NavBack),
            toolbar_button("→").on_press(Msg::NavForward),
            toolbar_button("↻").on_press(Msg::NavReload),
            text_input("Search or enter URL", &self.url_field)
                .id(integration::url_input_id())
                .on_input(Msg::UrlInput)
                .on_submit(Msg::UrlSubmit)
                .padding(6)
                .width(Length::Fill)
                .style(sola_kit::components::text_input::style),
        ]
        .spacing(6)
        .padding(6)
        .align_y(iced::Alignment::Center)
        .height(Length::Fixed(CHROME_HEIGHT))
        .into()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch(vec![
            Subscription::run(frame_stream),
            iced::time::every(Duration::from_millis(250)).map(|_| Msg::Tick),
            sola_kit::app::bus_subscription().map(Msg::Bus),
            // Global pointer stream so a divider drag can follow the
            // cursor past the divider's hit-rect (iced has no pointer
            // capture). `CursorMoved` is a no-op unless dragging.
            event::listen_with(|event, _, _| match event {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Msg::CursorMoved(position.x))
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Msg::CursorReleased)
                }
                _ => None,
            }),
        ])
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn normalize_url(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(colon) = trimmed.find(':') {
        let scheme = &trimmed[..colon];
        if !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphabetic()) {
            return trimmed.to_string();
        }
    }
    format!("https://{trimmed}")
}

fn frame_stream() -> impl Stream<Item = Msg> {
    stream::channel(64, async |mut output| {
        let engine = ENGINE.get().expect("ENGINE not initialized");
        let rx = engine.frames();
        let slot = match SLOT_FOR_STREAM.get() {
            Some(s) => s.clone(),
            None => {
                tracing::error!("SLOT_FOR_STREAM not set before subscription started");
                return;
            }
        };
        let active = match ACTIVE_TAB_FOR_STREAM.get() {
            Some(a) => a.clone(),
            None => {
                tracing::error!("ACTIVE_TAB_FOR_STREAM not set before subscription started");
                return;
            }
        };
        loop {
            let tagged = match tokio::task::spawn_blocking({
                let rx = rx.clone();
                move || rx.lock().unwrap().recv().ok()
            })
            .await
            {
                Ok(Some(f)) => f,
                _ => break,
            };
            if tagged.tab_id.0 != active.load(Ordering::Relaxed) {
                continue;
            }
            *slot.pending.lock().unwrap() = Some(tagged.frame);
            if output.send(Msg::NewFrame).await.is_err() {
                break;
            }
        }
    })
}
