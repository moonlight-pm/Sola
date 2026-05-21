//! sola-browser-wpe — phase 0c skeleton.
//!
//! Joins the two phase-0 halves: WPE renders a hardcoded URL,
//! iced samples each DMA-BUF frame in a `shader::Program` and
//! draws it as the entire window content. No chrome, no input
//! forwarding yet — that comes in phase 1+ as we layer on the
//! actual browser UI.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

use iced::futures::SinkExt;
use iced::futures::Stream;
use iced::stream;
use iced::widget::Shader;
use iced::{Element, Length, Subscription, Task};

use sola_browser_wpe::shader::{FrameSlot, WpeProgram};
use sola_browser_wpe::wpe::WpeEngine;

const APP_ID: &str = "sola-browser-wpe";
/// Default URL when no argv is given. Override with `sola-browser-wpe <url>`.
const DEFAULT_URL: &str = "https://slate.auto";
const VIEW_W: u32 = 1280;
const VIEW_H: u32 = 800;

/// WPE engine handle — global so the iced subscription can read
/// frames without iced needing to thread it through App state.
/// Initialized once in `main` before `iced::application` runs.
static ENGINE: OnceLock<WpeEngine> = OnceLock::new();

fn main() -> iced::Result {
    sola_core::log::init(APP_ID);
    tracing::info!("{APP_ID} starting");

    // Tell WPE where its helper processes live. Baked at build time
    // from `pkg-config --variable=exec_prefix wpe-webkit-2.0` (see
    // build.rs); we set the env var here so the binary works the
    // same whether run inside the dev shell or from /opt/sola/bin.
    // SAFETY: single-threaded program startup, before any spawn.
    unsafe { std::env::set_var("WEBKIT_EXEC_PATH", env!("WEBKIT_EXEC_PATH")) };

    // Note: `WPE_BUFFER_FORMAT=AR24:0:*` (force LINEAR modifier)
    // does allocate LINEAR memory but the NVIDIA proprietary
    // driver's EGL can't bind a LINEAR DMA-BUF as a framebuffer
    // target — "AcceleratedSurface was unable to construct a
    // complete framebuffer" — so the WebProcess never actually
    // renders into it and we get an uninitialized buffer. NVIDIA
    // only allows block-linear modifiers for render targets in
    // its GL/EGL context. Modifier-aware Vulkan import is the
    // only path forward; see commit log around phase 0c.

    let _ = sola_core::env::activate_wayland_session(10_000);

    // Hide WAYLAND_DISPLAY from libWPEWebKit's init. With it set,
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

    // Spawn WPE first — its worker thread starts loading the URL
    // before iced has even opened a window. By the time the iced
    // shader Program's first prepare runs, a frame is usually ready.
    let url = std::env::args().nth(1).unwrap_or_else(|| DEFAULT_URL.to_string());
    tracing::info!(%url, "loading url");
    let engine = WpeEngine::spawn(&url, VIEW_W, VIEW_H);

    if let Some(d) = saved_wayland_display {
        // SAFETY: spawn() blocked until WPE finished the parts of
        // init that consult WAYLAND_DISPLAY; the GMainLoop on the
        // worker doesn't look it up. iced hasn't started yet.
        unsafe { std::env::set_var("WAYLAND_DISPLAY", d) };
    }
    let releaser = engine.cmd_sender();
    ENGINE.set(engine).map_err(|_| ()).expect("ENGINE set twice");

    let slot = Arc::new(FrameSlot {
        pending: Mutex::new(None),
        releaser,
        // Seed with the same dimensions we passed to spawn so the
        // shader Program's first prepare won't redundantly fire
        // another Resize for the size WPE is already targeting.
        last_size: Mutex::new((VIEW_W, VIEW_H)),
    });
    SLOT_FOR_STREAM
        .set(slot.clone())
        .map_err(|_| ())
        .expect("SLOT_FOR_STREAM set twice");
    let app_slot = slot.clone();

    iced::application(
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
    .run()
}

struct App {
    slot: Arc<FrameSlot>,
}

#[derive(Debug, Clone)]
enum Msg {
    /// A new frame is ready in the slot — view() will repaint and
    /// the shader Pipeline will pick it up on next prepare.
    NewFrame,
}

impl App {
    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            // The subscription stashed the frame in `slot.pending`
            // before sending NewFrame; nothing for App::update to
            // do beyond letting iced trigger a redraw.
            Msg::NewFrame => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        Shader::new(WpeProgram {
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

/// Bridge the std mpsc receiver from WpeEngine to an iced
/// Subscription. Frames land in `slot.pending`; we emit `Msg::NewFrame`
/// to trigger redraws.
fn frame_stream() -> impl Stream<Item = Msg> {
    stream::channel(64, async |mut output| {
        // Block on the std::sync::mpsc receiver inside a blocking
        // task. Each frame arrives, we stash it, and signal iced.
        // The receiver is owned by ENGINE; we steal exclusive use
        // by holding the Mutex.
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
            // spawn_blocking the recv so we don't block iced's runtime.
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
/// Set in `main` after creating the slot. OnceLock because the
/// iced subscription stream's closure can't capture the slot
/// directly — it has to live in 'static land.
static SLOT_FOR_STREAM: OnceLock<Arc<FrameSlot>> = OnceLock::new();
