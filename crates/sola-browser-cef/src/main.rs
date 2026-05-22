//! sola-browser-cef — CEF-backed browser with iced chrome.
//!
//! Same overall shape as `sola-browser-wpe::main`. Differences are
//! all in the engine boundary: CEF subprocess gate before logger,
//! CEF lifecycle (init → run_message_loop → shutdown) instead of
//! WPE's GMain loop, no WAYLAND_DISPLAY env-var dance.

use std::process::ExitCode;
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

use sola_browser_cef::cef::{CefEngine, Cmd, NavCmd};
use sola_browser_cef::shader::{CefProgram, FrameSlot};

const APP_ID: &str = "sola-browser-cef";
/// Default URL when no argv is given. Override with `sola-browser-cef <url>`.
/// Note CEF re-execs this binary for each subprocess (renderer/GPU/utility/
/// zygote); the URL only matters in the *browser* process. The subprocesses
/// already exited from `dispatch_subprocess` before this constant is read.
const DEFAULT_URL: &str = "https://slate.auto";
const VIEW_W: u32 = 1280;
const VIEW_H: u32 = 800;
const CHROME_HEIGHT: f32 = 36.0;

/// CEF engine handle — global so the iced subscription can read
/// frames without iced needing to thread it through App state.
/// Initialized once in `main` before `iced::application` runs.
static ENGINE: OnceLock<CefEngine> = OnceLock::new();

fn main() -> ExitCode {
    // CEF subprocess gate — must run *before* logger init so renderer
    // / GPU / utility workers don't open the shared log file. CEF
    // re-execs this binary for every helper process; the gate
    // returns Some(exit_code) for workers and None for the browser
    // process.
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

    let result = iced::application(
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
    engine_url: Arc<Mutex<String>>,
    url_field: String,
    last_engine_url: String,
}

#[derive(Debug, Clone)]
enum Msg {
    NewFrame,
    NavBack,
    NavForward,
    NavReload,
    UrlInput(String),
    UrlSubmit,
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

        let webview = Shader::new(CefProgram {
            slot: self.slot.clone(),
        })
        .width(Length::Fill)
        .height(Length::Fill);

        column![chrome, webview].into()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch(vec![
            Subscription::run(frame_stream),
            iced::time::every(Duration::from_millis(250)).map(|_| Msg::Tick),
        ])
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

static SLOT_FOR_STREAM: OnceLock<Arc<FrameSlot>> = OnceLock::new();
