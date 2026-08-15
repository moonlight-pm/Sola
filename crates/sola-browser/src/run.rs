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

use iced::Subscription;
use iced::futures::{SinkExt, Stream, StreamExt as _};
use iced::stream;
use iced_futures::subscription::{self, EventStream, Recipe};

use crate::app::{App, DEFAULT_URL, Msg, VIEW_H, VIEW_W};
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
            // Park every frame we see so a later tab/profile switch can
            // present it synchronously (no helper round-trip).
            slot.parked_frames
                .lock()
                .unwrap()
                .insert(tid, tagged.frame.clone());
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
            slot.last_frame_ms
                .store(crate::engine::monotonic_ms(), Ordering::Relaxed);
            let _ = &active;
            // If the shader is already request_redraw-pumping, do not enqueue
            // NewFrame — that rebuilds the whole chrome tree at 60 Hz and
            // starves input / menus.
            if slot.pumping.load(Ordering::Relaxed) {
                continue;
            }
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
    subscription::from_recipe(FrameStreamRecipe::<E> {
        frames,
        slot,
        active,
    })
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
pub fn run<E: Engine>(base_id: &'static str) -> ExitCode {
    if let Some(code) = E::dispatch_subprocess(base_id) {
        return code;
    }

    // Headless CEF helper: no Wayland window, no kit startup.
    if let Some(code) = crate::cef::host::try_run(base_id) {
        return code;
    }

    // First non-flag argv token only — never treat `--password-store=…` /
    // CEF switches as open-URL (they become `https://--…` tabs otherwise).
    let argv = std::env::args()
        .skip(1)
        .find(|a| crate::session::is_cli_open_url(a));

    // One iced window. A second process (MIME / solactl open / launcher)
    // used to reap the live CEF helpers and leave a blank parked frame.
    let _chrome_lock = match crate::instance::claim() {
        Ok(lock) => lock,
        Err(()) => match crate::instance::handoff(argv.as_deref()) {
            Ok(()) => {
                tracing::info!("existing chrome accepted handoff — this process exits");
                return ExitCode::SUCCESS;
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "existing chrome is up but handoff failed — not starting a second window"
                );
                return ExitCode::FAILURE;
            }
        },
    };

    // Wayland/GPU env, fonts, and watch_own_binary (re-exec on
    // /opt/sola/bin/<app> change — same as other kit apps). Without this
    // the browser never auto-restarts after `cargo make install`.
    let _socket = sola_kit::app::startup(base_id);

    // D8: registry + active profile dirs; wipe pre-profile flat data.
    let _ = crate::profiles::ensure_active();
    // Only orphan helpers / pre-exec children — never another chrome's engines.
    crate::cef::host::reap_stale_browser_procs();
    let app_id = base_id;
    tracing::info!(
        %app_id,
        profile = %crate::profiles::active().name,
        "browser chrome (one window; CEF in helpers)"
    );

    #[cfg(feature = "bitwarden")]
    crate::vault::passkey_bridge::install();

    let boot_session = crate::session::BrowserSession::load();
    let boot_groups = boot_session.groups.clone();
    let (boot_tabs, boot_active, sidebar_w) = boot_session.bootstrap(argv, DEFAULT_URL);
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
        parked_frames: Mutex::new(std::collections::HashMap::new()),
        blank_content: std::sync::atomic::AtomicBool::new(false),
        redraw_queued: std::sync::atomic::AtomicBool::new(false),
        pumping: std::sync::atomic::AtomicBool::new(false),
        last_frame_ms: std::sync::atomic::AtomicU64::new(0),
        ime: engine.ime_handle(),
    });

    // Browser + Edit + Profiles (dynamic profile list with active check).
    let mut bus = sola_kit::app::BusSetup::new(app_id).subscribe(crate::integration::SUBSCRIBE);
    for def in crate::integration::browser_app_menu(app_id).menus {
        bus = bus.app_menu_definition(def);
    }
    bus.install();

    // `engine` is moved into the App on first call. The iced application
    // initializer must be `Fn`, so we wrap `engine` in `Option` and take
    // it once via `Option::take`.
    let engine_cell = std::cell::Cell::new(Some(engine));
    let boot_tabs = std::cell::RefCell::new(Some(boot_tabs));
    let boot_groups = std::cell::RefCell::new(Some(boot_groups));

    let result = iced::application(
        move || {
            let engine = engine_cell
                .take()
                .expect("browser App init called more than once");
            let tabs = boot_tabs
                .borrow_mut()
                .take()
                .expect("browser App init called more than once");
            let groups = boot_groups
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
                groups,
            );
            (
                app,
                sola_kit::window_ready_task(crate::app::Msg::WindowReady),
            )
        },
        App::<E>::update,
        App::<E>::view,
    )
    .title(move |app: &App<E>| {
        let profile = crate::profiles::active().name;
        match app.active_tab_info() {
            Some(t) if !t.title.is_empty() => format!("{profile} — {}", t.title),
            Some(t) if !t.url.is_empty() => format!("{profile} — {}", t.url),
            _ => profile,
        }
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
