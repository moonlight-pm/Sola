//! sola-browser-cef — CEF-backed sibling of sola-browser-wpe.
//!
//! Same iced chrome, same `shader::Program` sampling a frame texture.
//! Engine differs: CEF off-screen-rendering via `on_paint` (CPU OSR)
//! on this NVIDIA box, because CEF's dma-buf `on_accelerated_paint`
//! transport doesn't work on NVIDIA proprietary — see the project
//! plan in `docs/specs/2026-05-21-sola-browser-cef-port-and-benchmark.md`.

use std::process::ExitCode;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

use iced::futures::SinkExt;
use iced::futures::Stream;
use iced::stream;
use iced::widget::Shader;
use iced::{Element, Length, Subscription, Task};

use sola_browser_cef::cef::CefEngine;
use sola_browser_cef::shader::{CefProgram, FrameSlot};

const APP_ID: &str = "sola-browser-cef";
/// Default URL when no argv is given. Override with `sola-browser-cef <url>`.
/// Note CEF re-execs this binary for each subprocess (renderer/GPU/utility/
/// zygote); the URL only matters in the *browser* process. The subprocesses
/// already exited from `dispatch_subprocess` before this constant is read.
const DEFAULT_URL: &str = "https://slate.auto";
const VIEW_W: u32 = 1280;
const VIEW_H: u32 = 800;

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
    ENGINE.set(engine).map_err(|_| ()).expect("ENGINE set twice");

    let slot = Arc::new(FrameSlot {
        pending: Mutex::new(None),
        releaser,
        last_size: Mutex::new((VIEW_W, VIEW_H)),
    });
    SLOT_FOR_STREAM
        .set(slot.clone())
        .map_err(|_| ())
        .expect("SLOT_FOR_STREAM set twice");
    let app_slot = slot.clone();

    let result = iced::application(
        move || App {
            slot: app_slot.clone(),
        },
        App::update,
        App::view,
    )
    .title(|_: &App| APP_ID.into())
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
}

#[derive(Debug, Clone)]
enum Msg {
    NewFrame,
}

impl App {
    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::NewFrame => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        Shader::new(CefProgram {
            slot: self.slot.clone(),
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::run(frame_stream)
    }
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
