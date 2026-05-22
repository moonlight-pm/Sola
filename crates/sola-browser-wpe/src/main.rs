//! sola-browser-wpe — WPE-backed browser with iced chrome.
//!
//! `main` brings WPE up on a worker thread (rendering DMA-BUFs we
//! sample via wgpu) and hosts iced on the main thread for the
//! window + chrome (URL bar, back/forward/reload). The shader
//! widget below the chrome shows whatever WPE just rendered;
//! input events on it are forwarded to the WebProcess.

use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

use iced::futures::SinkExt;
use iced::futures::Stream;
use iced::stream;
use iced::widget::{Shader, button, column, row, text, text_input};
use iced::{Element, Length, Subscription, Task};

use sola_browser_wpe::shader::{FrameSlot, WpeProgram};
use sola_browser_wpe::wpe::{Cmd, NavCmd, WpeEngine};

const APP_ID: &str = "sola-browser-wpe";
/// Default URL when no argv is given. Override with `sola-browser-wpe <url>`.
const DEFAULT_URL: &str = "https://slate.auto";
const VIEW_W: u32 = 1280;
const VIEW_H: u32 = 800;
const CHROME_HEIGHT: f32 = 36.0;

/// WPE engine handle — global so the iced subscription can read
/// frames without iced needing to thread it through App state.
/// Initialized once in `main` before `iced::application` runs.
static ENGINE: OnceLock<WpeEngine> = OnceLock::new();

fn main() -> iced::Result {
    sola_core::log::init(APP_ID);
    tracing::info!("{APP_ID} starting");

    // Tell WPE where its helper processes live. Baked at build time
    // from `pkg-config --variable=exec_prefix wpe-webkit-2.0`.
    // SAFETY: single-threaded program startup, before any spawn.
    unsafe { std::env::set_var("WEBKIT_EXEC_PATH", env!("WEBKIT_EXEC_PATH")) };

    let _ = sola_core::env::activate_wayland_session(10_000);

    // Hide WAYLAND_DISPLAY from libWPEWebKit's init. Without this
    // libWPEWebKit's bundled wpe-platform-wayland module wakes up
    // alongside our headless one and registers a hidden Wayland
    // toplevel for the WebProcess — sola-shell sees that as a
    // second window with app_id `org.webkit.app-<sha256>`. We
    // restore WAYLAND_DISPLAY after `WpeEngine::spawn` returns
    // (it blocks until WPE init is past the parts that consult
    // the env var) so iced sees it on the main-thread side.
    //
    // SAFETY: single-threaded between log init and WpeEngine::spawn.
    let saved_wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
    unsafe { std::env::remove_var("WAYLAND_DISPLAY") };

    let url = std::env::args().nth(1).unwrap_or_else(|| DEFAULT_URL.to_string());
    tracing::info!(%url, "loading url");
    let engine = WpeEngine::spawn(&url, VIEW_W, VIEW_H);

    if let Some(d) = saved_wayland_display {
        unsafe { std::env::set_var("WAYLAND_DISPLAY", d) };
    }
    let releaser = engine.cmd_sender();
    let url_handle = engine.url_handle();
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

    iced::application(
        move || App {
            slot: slot.clone(),
            releaser: releaser.clone(),
            engine_url: url_handle.clone(),
            url_field: url.clone(),
            last_engine_url: url.clone(),
        },
        App::update,
        App::view,
    )
    .title(|app: &App| {
        if app.url_field.is_empty() {
            APP_ID.into()
        } else {
            format!("{APP_ID} — {}", app.url_field)
        }
    })
    .subscription(App::subscription)
    .window(iced::window::Settings {
        decorations: false,
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: APP_ID.into(),
            ..Default::default()
        },
        ..iced::window::Settings::default()
    })
    .run()
}

struct App {
    slot: Arc<FrameSlot>,
    /// Cmd channel to the WPE worker. Used for nav (back/forward/
    /// reload/load_url); input + resize cmds flow through the
    /// same sender from `FrameSlot::releaser`.
    releaser: Sender<Cmd>,
    /// Shared handle on the engine's current URL. Polled every
    /// `Tick` so the URL bar tracks page navigation that didn't
    /// originate from the chrome (link clicks, JS redirects, etc).
    engine_url: Arc<Mutex<String>>,
    /// The URL bar's editable text. Set from `engine_url` whenever
    /// the engine reports a new URL.
    url_field: String,
    /// Cache of the engine URL we last copied into `url_field`,
    /// so we only overwrite the field when the engine actually
    /// navigated to something new (not on every Tick).
    last_engine_url: String,
}

#[derive(Debug, Clone)]
enum Msg {
    /// New frame ready — triggers iced redraw which feeds the
    /// shader Pipeline.
    NewFrame,
    /// Chrome navigation buttons.
    NavBack,
    NavForward,
    NavReload,
    /// URL bar contents changed (every keystroke).
    UrlInput(String),
    /// URL bar Enter / Go button — load the field's current value.
    UrlSubmit,
    /// Timer tick — poll the engine for URL changes.
    Tick,
}

impl App {
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
                let _ = self.releaser.send(Cmd::Nav(NavCmd::LoadUrl(url)));
            }
            Msg::Tick => {
                let current = self.engine_url.lock().unwrap().clone();
                if current != self.last_engine_url {
                    self.last_engine_url = current.clone();
                    // Overwrite the URL field with the engine's
                    // current URL even if the user was mid-edit.
                    // v1 behaviour; refine later by tracking focus.
                    self.url_field = current;
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Msg> {
        let chrome = row![
            button(text("←")).on_press(Msg::NavBack),
            button(text("→")).on_press(Msg::NavForward),
            button(text("↻")).on_press(Msg::NavReload),
            text_input("Search or enter URL", &self.url_field)
                .on_input(Msg::UrlInput)
                .on_submit(Msg::UrlSubmit)
                .padding(6)
                .width(Length::Fill),
        ]
        .spacing(4)
        .padding(4)
        .height(Length::Fixed(CHROME_HEIGHT));

        let webview = Shader::new(WpeProgram {
            slot: self.slot.clone(),
        })
        .width(Length::Fill)
        .height(Length::Fill);

        column![chrome, webview].into()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch(vec![
            Subscription::run(frame_stream),
            // Poll the engine URL ~4×/s. Cheap (one Mutex acquire)
            // and good enough for an address bar to track JS
            // redirects without feeling laggy.
            iced::time::every(Duration::from_millis(250)).map(|_| Msg::Tick),
        ])
    }
}

/// Best-effort URL normalization. If the user typed something
/// without a scheme, prepend `https://`. Anything with a `:` early
/// in the string is assumed to already be a valid scheme + URI
/// (covers `http://`, `https://`, `about:`, `file://`, etc.).
/// Search-query handling (treat free text as a DuckDuckGo query
/// etc.) is left for a follow-up.
fn normalize_url(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // Pre-existing scheme — leave it alone.
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
        loop {
            let frame = match tokio::task::spawn_blocking({
                let rx = rx.clone();
                move || rx.lock().unwrap().recv().ok()
            })
            .await
            {
                Ok(Some(frame)) => frame,
                _ => break,
            };
            *slot.pending.lock().unwrap() = Some(frame);
            if output.send(Msg::NewFrame).await.is_err() {
                break;
            }
        }
    })
}

/// Mirror of the App's `slot` shared with the subscription stream.
/// OnceLock because the subscription stream's closure can't capture
/// the slot directly — it has to live in 'static land.
static SLOT_FOR_STREAM: OnceLock<Arc<FrameSlot>> = OnceLock::new();
