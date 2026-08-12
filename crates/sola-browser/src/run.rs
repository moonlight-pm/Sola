//! Generic browser entry point and frame-stream subscription.
//!
//! `run::<E>()` is the single function an engine-specific `main` calls.
//! `frame_stream::<E>` is the stream that moves decoded frames from the
//! engine worker into the `FrameSlot` and wakes the iced renderer.
//!
//! # Subscription wiring
//!
//! iced 0.14 has no `Subscription::run_with_id`. We implement a custom
//! `iced_futures::subscription::Recipe` (`FrameStreamRecipe<E>`) that carries
//! the owned `Arc`s and uses a fixed string `"web-frames"` as its hash
//! identity (one frame subscription per browser process). `iced_futures` is
//! already a transitive dependency of `iced`; we declare it explicitly to
//! access the `Recipe` trait and `from_recipe` helper.
//!
//! This satisfies the brief's hard requirement: the frame stream is built
//! from owned `Arc`s cloned out of `App<E>`, not from process-wide statics.

use std::hash::Hash;
use std::process::ExitCode;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use iced::futures::{SinkExt, Stream, StreamExt as _};
use iced::stream;
use iced::Subscription;
use iced_futures::subscription::{self, EventStream, Recipe};


use crate::app::{App, Msg, DEFAULT_URL, VIEW_H, VIEW_W};
use crate::engine::{ActiveHandle, Engine, FrameReceiver, FrameSlot};

// ---------------------------------------------------------------------------
// Frame stream
// ---------------------------------------------------------------------------

/// Produces a `Stream<Item = Msg>` that reads decoded frames from the engine
/// worker, drops frames from inactive tabs, and wakes the iced renderer for
/// each frame belonging to the currently-active tab.
///
/// Parameters are owned `Arc`s cloned out of `App<E>` fields — no
/// process-wide statics involved.
pub fn frame_stream<E: Engine>(
    frames: FrameReceiver<E::Frame>,
    slot: Arc<FrameSlot<E>>,
    active: ActiveHandle,
) -> impl Stream<Item = Msg> {
    stream::channel(64, async move |mut output| {
        // One long-lived blocking thread — NOT spawn_blocking per frame
        // (that alone made caret blink / placeholder motion feel laggy).
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let frames_thread = frames.clone();
        std::thread::Builder::new()
            .name("browser-frames".into())
            .spawn(move || {
                loop {
                    match frames_thread.recv() {
                        Ok(f) => {
                            if tx.send(f).is_err() {
                                break;
                            }
                        }
                        Err(()) => break,
                    }
                }
            })
            .expect("spawn browser-frames thread");

        while let Some(tagged) = rx.recv().await {
            // Only the painted tab may update the shader slot. Background
            // frames drop immediately so inactive tabs cannot thrash GPU
            // upload or steal the latest-wins mailbox.
            let paint_tab = slot.paint_tab.load(Ordering::Relaxed);
            let tid = tagged.tab_id.0;
            if tid != paint_tab {
                let mut need = slot.need_park_prime.lock().unwrap();
                need.remove(&tid); // consume one-shot primes without holding
                continue;
            }
            // Keep only the latest pending frame.
            *slot.pending.lock().unwrap() = Some(crate::engine::PendingFrame {
                tab_id: tagged.tab_id,
                frame: tagged.frame,
            });
            let _ = &active;
            // Coalesce wakeups: if iced hasn't processed the last NewFrame yet,
            // don't enqueue another — keyboard/input must stay ahead of paints.
            if slot
                .redraw_queued
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                if output.send(Msg::NewFrame).await.is_err() {
                    break;
                }
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Custom Recipe (iced_futures) — carries Arc fields without Hash
// ---------------------------------------------------------------------------

/// A one-shot iced subscription recipe that runs `frame_stream<E>`.
///
/// `Recipe::hash` uses the fixed string `"web-frames"` — there is exactly
/// one frame subscription per browser process, so a constant identity is
/// correct and stable across re-renders.
struct FrameStreamRecipe<E: Engine> {
    frames: FrameReceiver<E::Frame>,
    slot: Arc<FrameSlot<E>>,
    active: ActiveHandle,
}

impl<E: Engine> Recipe for FrameStreamRecipe<E> {
    type Output = Msg;

    fn hash(&self, state: &mut subscription::Hasher) {
        "web-frames".hash(state);
    }

    fn stream(self: Box<Self>, _input: EventStream) -> iced_futures::BoxStream<Self::Output> {
        frame_stream::<E>(self.frames, self.slot, self.active).boxed()
    }
}

/// Build the frame-stream `Subscription` from owned `Arc`s.
///
/// Uses a custom `Recipe` via `iced_futures::subscription::from_recipe`
/// since `Subscription::run_with_id` is not available in iced 0.14.
pub fn frame_subscription<E: Engine>(
    frames: FrameReceiver<E::Frame>,
    slot: Arc<FrameSlot<E>>,
    active: ActiveHandle,
) -> Subscription<Msg> {
    subscription::from_recipe(FrameStreamRecipe::<E> { frames, slot, active })
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Generic browser entry point. Call this from an engine-specific `main`:
///
/// ```rust,ignore
/// fn main() -> std::process::ExitCode {
///     crate::run::run::<MyEngine>("my-browser-app-id")
/// }
/// ```
pub fn run<E: Engine>(app_id: &'static str) -> ExitCode {
    if let Some(code) = E::dispatch_subprocess(app_id) {
        return code;
    }

    // Wayland/GPU env, fonts, and watch_own_binary (re-exec on
    // /opt/sola/bin/<app> change — same as other kit apps). Without this
    // the browser never auto-restarts after `cargo make install`.
    let _socket = sola_kit::app::startup(app_id);

    // D8: registry + active profile dirs; wipe pre-profile flat data.
    let _profile = crate::profiles::ensure_active();

    #[cfg(feature = "bitwarden")]
    crate::vault::passkey_bridge::install();

    let argv = std::env::args().nth(1);
    let (boot_tabs, boot_active, sidebar_w) =
        crate::session::BrowserSession::load().bootstrap(argv, DEFAULT_URL);
    tracing::info!(
        tabs = boot_tabs.len(),
        active_index = boot_active,
        "bootstrapping session"
    );

    // Engine starts with no tabs; chrome opens the restored set.
    let engine = E::spawn(app_id, "", VIEW_W, VIEW_H);

    let cmd_tx = engine.cmd_sender();
    let tabs_handle = engine.tabs_handle();
    let active_handle = engine.active_tab_handle();
    let cursor = engine.cursor_handle();

    let slot = Arc::new(FrameSlot::<E> {
        pending: Mutex::new(None),
        cmd_tx: cmd_tx.clone(),
        last_size: Mutex::new((VIEW_W, VIEW_H)),
        cursor,
        paint_tab: std::sync::atomic::AtomicU64::new(u64::MAX),
        need_park_prime: Mutex::new(std::collections::HashSet::new()),
        drop_paint_tabs: Mutex::new(Vec::new()),
        redraw_queued: std::sync::atomic::AtomicBool::new(false),
    });

    // Browser + Edit + Profiles (dynamic profile list with active check).
    let menus = crate::integration::browser_app_menu(app_id).menus;
    let mut bus = sola_kit::app::BusSetup::new(app_id).subscribe(crate::integration::SUBSCRIBE);
    for def in menus {
        bus = bus.app_menu_definition(def);
    }
    bus.install();

    // `engine` is moved into the App on first call. The iced application
    // initializer must be `Fn`, so we wrap `engine` in `Option` and take
    // it once via `Option::take`.
    let engine_cell = std::cell::Cell::new(Some(engine));
    let boot_tabs = std::cell::RefCell::new(Some(boot_tabs));

    let result = iced::application(
        move || {
            let engine = engine_cell
                .take()
                .expect("browser App init called more than once");
            let tabs = boot_tabs
                .borrow_mut()
                .take()
                .expect("browser App init called more than once");
            let app = App::<E>::new(
                engine,
                slot.clone(),
                cmd_tx.clone(),
                tabs_handle.clone(),
                active_handle.clone(),
                app_id,
                tabs,
                boot_active,
                sidebar_w,
            );
            (
                app,
                sola_kit::window_ready_task(crate::app::Msg::WindowReady),
            )
        },
        App::<E>::update,
        App::<E>::view,
    )
    .title(move |app: &App<E>| match app.active_tab_info() {
        Some(t) if !t.title.is_empty() => format!("{app_id} — {}", t.title),
        Some(t) if !t.url.is_empty() => format!("{app_id} — {}", t.url),
        _ => app_id.to_string(),
    })
    .subscription(App::<E>::subscription)
    .theme(App::<E>::theme)
    .default_font(sola_kit::fonts::ui())
    .window(sola_kit::app::window_settings_transparent(app_id))
    .run();

    if let Err(e) = result {
        tracing::error!("iced::application returned: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
