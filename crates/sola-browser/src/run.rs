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
use std::sync::Arc;

use iced::futures::{SinkExt, Stream, StreamExt as _};
use iced::stream;
use iced::Subscription;
use iced_futures::subscription::{self, EventStream, Recipe};

use crate::app::{App, Msg, DEFAULT_URL, VIEW_H, VIEW_W};
use crate::engine::{AbsorbDamage, ActiveHandle, Engine, FrameReceiver, FrameSlot};

// ---------------------------------------------------------------------------
// Frame stream
// ---------------------------------------------------------------------------

/// The frame slots a frame can go to: one per page window when the browser
/// has a window per page, else the single page slot.
pub type SlotRegistry<E> = Arc<std::sync::Mutex<Vec<Arc<FrameSlot<E>>>>>;

/// Produces a `Stream<Item = Msg>` that reads decoded frames from the engine
/// worker and hands each to the slot that paints its tab (a page or the
/// docked inspector), waking the iced renderer.
///
/// With one slot (a single-window chrome), frames of other
/// tabs park in it for instant tab switches; with a slot per page window,
/// a frame goes to its page's window and nowhere else.
///
/// Parameters are owned `Arc`s cloned out of `App<E>` fields — no
/// process-wide statics involved.
pub fn frame_stream<E: Engine>(
    frames: FrameReceiver<E::Frame>,
    slots: SlotRegistry<E>,
    active: ActiveHandle,
) -> impl Stream<Item = Msg> {
    stream::channel(64, async move |mut output| {
        // One long-lived blocking thread — NOT spawn_blocking per frame
        // (that alone made caret blink / placeholder motion feel laggy).
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let frames_thread = frames.clone();
        std::thread::Builder::new()
            .name("browser-frames".into())
            .spawn(move || loop {
                match frames_thread.recv() {
                    Ok(f) => {
                        if tx.send(f).is_err() {
                            break;
                        }
                    }
                    Err(()) => break,
                }
            })
            .expect("spawn browser-frames thread");

        while let Some(mut tagged) = rx.recv().await {
            let tid = tagged.tab_id.0;
            let is_drag = E::frame_is_page_drag(&tagged.frame);
            let (slot, page) = {
                let set = slots.lock().unwrap();
                let page = set
                    .iter()
                    .find(|s| s.paint_tab.load(Ordering::Relaxed) == tid)
                    .map(|s| (s.clone(), true));
                let found = page.or_else(|| {
                    set.iter()
                        .find(|s| s.devtools_tab.load(Ordering::Relaxed) == tid)
                        .map(|s| (s.clone(), false))
                });
                match found {
                    Some(hit) => hit,
                    // Single slot: other tabs park for an instant switch.
                    None if set.len() == 1 => {
                        let slot = &set[0];
                        if !is_drag {
                            slot.parked_frames
                                .lock()
                                .unwrap()
                                .insert(tid, tagged.frame.clone());
                        }
                        slot.need_park_prime.lock().unwrap().remove(&tid);
                        continue;
                    }
                    None => continue,
                }
            };
            // Page and docked inspector may both update their shader slots.
            if page {
                slot.page_drag.store(is_drag, Ordering::Relaxed);
                let mut g = slot.pending.lock().unwrap();
                if let Some(old) = g.take() {
                    slot.osr.note_pending_drop();
                    tagged.frame.absorb_damage(old.frame);
                }
                if !is_drag {
                    slot.parked_frames
                        .lock()
                        .unwrap()
                        .insert(tid, tagged.frame.clone());
                }
                *g = Some(crate::engine::PendingFrame {
                    tab_id: tagged.tab_id,
                    frame: tagged.frame,
                });
            } else {
                let mut g = slot.devtools_pending.lock().unwrap();
                if let Some(old) = g.take() {
                    tagged.frame.absorb_damage(old.frame);
                }
                *g = Some(crate::engine::PendingFrame {
                    tab_id: tagged.tab_id,
                    frame: tagged.frame,
                });
            }
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
    slots: SlotRegistry<E>,
    active: ActiveHandle,
}

impl<E: Engine> Recipe for FrameStreamRecipe<E> {
    type Output = Msg;

    fn hash(&self, state: &mut subscription::Hasher) {
        "web-frames".hash(state);
    }

    fn stream(self: Box<Self>, _input: EventStream) -> iced_futures::BoxStream<Self::Output> {
        frame_stream::<E>(self.frames, self.slots, self.active).boxed()
    }
}

/// Build the frame-stream `Subscription` for one page slot.
///
/// Uses a custom `Recipe` via `iced_futures::subscription::from_recipe`
/// since `Subscription::run_with_id` is not available in iced 0.14.
pub fn frame_subscription<E: Engine>(
    frames: FrameReceiver<E::Frame>,
    slot: Arc<FrameSlot<E>>,
    active: ActiveHandle,
) -> Subscription<Msg> {
    frame_subscription_slots(frames, Arc::new(std::sync::Mutex::new(vec![slot])), active)
}

/// Build the frame-stream `Subscription` over a set of slots (a window per
/// page): the registry is read per frame, so slots can come and go.
pub fn frame_subscription_slots<E: Engine>(
    frames: FrameReceiver<E::Frame>,
    slots: SlotRegistry<E>,
    active: ActiveHandle,
) -> Subscription<Msg> {
    subscription::from_recipe(FrameStreamRecipe::<E> {
        frames,
        slots,
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

    // `--app <id>`: a web app's launch — its window, in this chrome or the
    // one running. Its value is not a URL.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let launch_app = args
        .windows(2)
        .find(|w| w[0] == "--app")
        .map(|w| w[1].clone());
    // First non-flag argv token only — never treat `--password-store=…` /
    // CEF switches as open-URL (they become `https://--…` tabs otherwise).
    let argv = args
        .iter()
        .filter(|a| Some(*a) != launch_app.as_ref())
        .find(|a| crate::session::is_cli_open_url(a))
        // Resolve relative HTML paths against *this* process cwd (xdg-open
        // child) before chrome.sock handoff — the live chrome has another cwd.
        .map(|a| crate::util::normalize_url(&a));

    // A web app's launch with no browser up starts the browser through the
    // session, as itself, and hands it the app.
    if let Some(app_id) = launch_app.as_deref() {
        if !crate::instance::chrome_is_live() && crate::instance::start_browser_for_app(app_id) {
            return ExitCode::SUCCESS;
        }
    }

    // One iced window. A second process (MIME / solactl open / launcher)
    // used to reap the live CEF helpers and leave a blank parked frame.
    let _chrome_lock = match crate::instance::claim() {
        Ok(lock) => lock,
        Err(()) => match launch_app.as_deref().map_or_else(
            || crate::instance::handoff(argv.as_deref()),
            crate::instance::handoff_app,
        ) {
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
    // `exec_self` after install leaves helpers with ppid == us. Ask them
    // to Quit (cookie flush) before SIGTERM leftovers.
    crate::cef::host::stop_all_profile_engines();
    // Only orphan helpers / pre-exec children — never another chrome's engines.
    crate::cef::host::reap_stale_browser_procs();
    let app_id = base_id;
    tracing::info!(
        %app_id,
        profile = %crate::profiles::active().id,
        "browser chrome (one window; CEF in helpers)"
    );

    #[cfg(feature = "bitwarden")]
    crate::vault::passkey_bridge::install();

    // Every profile a workspace runs opens its saved pages (Next slice 5).
    let (boot_tabs, boot_active, boot_closed) = crate::app::boot_pages(argv, DEFAULT_URL);
    tracing::info!(
        tabs = boot_tabs.len(),
        active_index = boot_active,
        profiles = ?crate::app::count_by_profile(&boot_tabs),
        "bootstrapping session"
    );

    // Engine starts with no tabs; chrome opens the restored set.
    let engine = E::spawn(app_id, "", VIEW_W, VIEW_H);

    let cmd_tx = engine.cmd_sender();
    let tabs_handle = engine.tabs_handle();
    let active_handle = engine.active_tab_handle();
    let cursor = engine.cursor_handle();

    let slot = Arc::new(FrameSlot::<E>::new(
        cmd_tx.clone(),
        cursor,
        engine.ime_handle(),
        VIEW_W,
        VIEW_H,
    ));

    // Browser + Edit + Profiles (dynamic profile list with active check).
    let mut bus = sola_kit::app::BusSetup::new(app_id).subscribe(crate::integration::SUBSCRIBE);
    for def in crate::integration::browser_app_menu(app_id).menus {
        bus = bus.app_menu_definition(def);
    }
    bus = bus.calls(crate::calls::OWNER, crate::calls::methods());
    bus.install();

    // `engine` is moved into the App on first call. The iced application
    // initializer must be `Fn`, so we wrap `engine` in `Option` and take
    // it once via `Option::take`.
    let engine_cell = std::cell::Cell::new(Some(engine));
    let boot_tabs = std::cell::RefCell::new(Some(boot_tabs));
    let boot_closed = std::cell::RefCell::new(Some(boot_closed));

    // A window per page (Next): the daemon opens one per restored page.
    let result = iced::daemon(
        move || {
            let engine = engine_cell
                .take()
                .expect("browser App init called more than once");
            let tabs = boot_tabs
                .borrow_mut()
                .take()
                .expect("browser App init called more than once");
            let closed = boot_closed
                .borrow_mut()
                .take()
                .expect("browser App init called more than once");
            let mut app = App::<E>::new(
                engine,
                slot.clone(),
                cmd_tx.clone(),
                tabs_handle.clone(),
                active_handle.clone(),
                app_id,
                tabs,
                boot_active,
                closed,
            );
            // A web app's launch opens it once its catalog entry is in.
            app.webapps.launch = launch_app.clone();
            // Restore the saved pages now; each gets its window.
            (app, iced::Task::done(crate::app::Msg::WindowReady(None)))
        },
        App::<E>::update,
        App::<E>::view,
    )
    .title(App::<E>::title)
    .subscription(App::<E>::subscription)
    .theme(App::<E>::theme)
    .default_font(sola_kit::fonts::ui())
    .run();

    if let Err(e) = result {
        tracing::error!("iced::daemon returned: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
