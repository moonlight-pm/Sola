//! App scaffolding — the boilerplate every sola iced app would
//! otherwise repeat byte-for-byte.
//!
//! Implementing [`App`] is *not yet* the way to ship a sola iced
//! app — iced's `application` builder threads typed state into
//! its update/view fns and that's hard to wrap generically without
//! either macros or HRTBs that defeat the point. Instead, this
//! module exposes the building blocks (font loading, app-menu
//! publishing, window settings, bus singleton helpers) so an app's
//! `main` is a thin compose of them. Once we have a second iced
//! app and see what the right shape is, we promote the common
//! parts into a `run::<A>()` entry point.
//!
//! For now: see `sola-monitor::main` for the canonical wiring.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use iced::futures::Stream;
use iced::Subscription;
use sola_bus::BusClient;
use sola_bus::Message;
use sola_bus::topics::{
    AppMenuPayload, MenuDefinition, MenuItem, Topic, TopicKind,
};
use sola_core::KeyChord;

/// Bus singleton — the kit installs one global `BusClient` per
/// process because iced's `application` builder doesn't thread
/// caller-supplied state into the `App::default()` constructor.
/// A static is the natural fit; thread-locals are the alternative.
static BUS: OnceLock<Arc<Mutex<BusClient>>> = OnceLock::new();

/// Borrow the process-wide bus. Panics if [`BusSetup::install`]
/// has not been called yet — that's a setup-order bug, not a
/// recoverable runtime condition.
pub fn bus() -> &'static Mutex<BusClient> {
    BUS.get().expect("sola_kit::bus: BUS not initialised").as_ref()
}

/// Convenience builder for the bus connect + subscribe + app-menu
/// dance every sola iced app does in its `main` before iced takes
/// over the thread. Build it, configure, then call [`install`] to
/// hand the connected client off to the kit's global slot.
///
/// ```ignore
/// use sola_kit::BusSetup;
///
/// BusSetup::new("sola-foo")
///     .subscribe(sola_bus::topics::TopicKind::ALL)
///     .app_menu("Foo", [("quit", "Quit Foo", sola_core::KeyCode::Q.meta())]) // .meta() → KeyChord
///     .install();
/// ```
///
/// [`install`]: BusSetup::install
pub struct BusSetup {
    app_id: &'static str,
    subscribe: Option<&'static [TopicKind]>,
    app_menus: Vec<MenuDefinition>,
    connect_timeout: Duration,
}

impl BusSetup {
    pub fn new(app_id: &'static str) -> Self {
        Self {
            app_id,
            subscribe: None,
            app_menus: Vec::new(),
            connect_timeout: Duration::from_millis(250),
        }
    }

    /// Subscribe the bus client to the given topic kinds before
    /// installation. Pass `TopicKind::ALL` to mirror sola-monitor's
    /// audit posture; pass a narrower slice for normal apps.
    pub fn subscribe(mut self, kinds: &'static [TopicKind]) -> Self {
        self.subscribe = Some(kinds);
        self
    }

    /// Publish a single-menu app menu before installation. Most apps
    /// only declare a "Quit App" action under their own name; this
    /// helper covers that 90% case. For richer menus, build a
    /// `MenuDefinition` directly and pass it to
    /// [`Self::app_menu_definition`].
    pub fn app_menu<I>(
        self,
        menu_label: impl Into<String>,
        items: I,
    ) -> Self
    where
        I: IntoIterator<Item = (&'static str, &'static str, KeyChord)>,
    {
        let menu_items = items
            .into_iter()
            .map(|(id, label, shortcut)| MenuItem::Action {
                id: id.into(),
                label: label.into(),
                shortcut: Some(shortcut),
                disabled: false,
                checked: false,
            })
            .collect();
        self.app_menu_definition(MenuDefinition {
            label: menu_label.into(),
            items: menu_items,
        })
    }

    /// Replace the configured app menu with a fully-built
    /// `MenuDefinition`. Use this when [`Self::app_menu`]'s
    /// (id, label, shortcut) shorthand is insufficient (submenus,
    /// separators, dynamic items, …).
    pub fn app_menu_definition(mut self, def: MenuDefinition) -> Self {
        self.app_menus.push(def);
        self
    }

    /// Declare an additional top-level menu (same (id, label, shortcut)
    /// shorthand as [`Self::app_menu`]). Call once per extra menu; menus
    /// publish in call order. Delegates to [`Self::app_menu`], which now
    /// appends rather than replaces.
    pub fn app_menu_more<I>(self, menu_label: impl Into<String>, items: I) -> Self
    where
        I: IntoIterator<Item = (&'static str, &'static str, KeyChord)>,
    {
        self.app_menu(menu_label, items)
    }

    /// Connect, subscribe, publish the menu, and stash the client
    /// in the global slot. Panics if called twice in the same
    /// process — that's a setup-order bug.
    pub fn install(self) {
        let mut client = BusClient::new();
        client.connect_blocking(self.connect_timeout);
        if let Some(kinds) = self.subscribe {
            if let Err(e) = client.subscribe(kinds) {
                tracing::warn!("bus subscribe failed: {e}");
            }
        }
        if !self.app_menus.is_empty() {
            if let Err(e) = client.emit(Topic::SetAppMenu(AppMenuPayload {
                app_id: self.app_id.into(),
                menus: self.app_menus,
            })) {
                tracing::warn!(app_id = self.app_id, "publish app menu failed: {e}");
            }
        }
        BUS.set(Arc::new(Mutex::new(client)))
            .map_err(|_| ())
            .expect("sola_kit::BusSetup::install called twice");
    }
}

/// Window settings every sola iced app uses: no decorations
/// (sola-shell draws chrome) + the correct `xdg_toplevel.app_id`
/// so the shell can match `SetAppMenu` against the surface.
///
/// On Linux iced reads app_id from
/// `window::Settings.platform_specific.application_id`, NOT from
/// the top-level `Settings::id` (which is only wired to
/// `winit::with_name` on dragonfly/freebsd/netbsd/openbsd).
/// Without setting it here, the window has empty app_id and the
/// shell can't match the menu.
pub fn window_settings(app_id: &'static str) -> iced::window::Settings {
    iced::window::Settings {
        decorations: false,
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: app_id.into(),
            ..Default::default()
        },
        ..iced::window::Settings::default()
    }
}

/// Convenience: kick off the standard sola startup flow up to (but
/// not including) the iced builder.
///
/// 1. `sola_core::log::init(app_id)`
/// 2. `sola_core::env::activate_wayland_session(20s)` — sets
///    `WAYLAND_DISPLAY` so winit's wayland client finds the river
///    socket.
/// 3. `sola_core::env::wait_for_wayland_socket(10s)` — blocks until
///    the socket file actually exists. river publishes the name file
///    a beat before the socket is bind-ready on cold boot; winit
///    connects fast enough to race that, so the wait is required.
/// 4. `sola_core::env::activate_gpu_env()` — points NixOS GPU
///    dispatch env (`VK_ICD_FILENAMES`, `__EGL_VENDOR_LIBRARY_DIRS`,
///    `LIBVA_DRIVERS_PATH`, `GSETTINGS_BACKEND`) at
///    `/run/opengl-driver/` so wgpu/EGL/Vulkan can initialise when
///    the app is launched from a bare TTY (no desktop session to
///    set them via `pam_env` or `~/.profile`).
/// 5. [`crate::fonts::ensure_system_fonts`] — load every system-installed
///    font family into iced's font db so `Font::with_name(family)` resolves
///    (Sola bundles no fonts; everything comes from system fontconfig).
/// 6. `sola_core::watcher::watch_own_binary()` — re-exec this process
///    in-place when its binary at `/opt/sola/bin/<name>` changes on
///    disk, so `cargo make install` is enough to pick up new code
///    without manually quitting the running app. Skipped when
///    `SOLA_NO_SELF_WATCH=1` is set in the environment (sola the
///    process manager sets this when launching apps it already
///    supervises directly, to avoid a double restart). Mirrors the
///    legacy `sola-app` behavior.
///
/// Returns the resolved Wayland socket name so the caller can log it.
/// Apps that need different log init or a different Wayland timeout
/// should skip this and call the underlying helpers themselves.
pub fn startup(app_id: &str) -> String {
    sola_core::log::init(app_id);
    tracing::info!("{app_id} starting");
    let socket = sola_core::env::activate_wayland_session(20_000);
    tracing::info!(socket = %socket, "wayland socket resolved");
    if sola_core::env::wait_for_wayland_socket(&socket, 10_000) {
        tracing::info!(socket = %socket, "wayland socket ready");
    } else {
        tracing::warn!(socket = %socket, "wayland socket not present after 10s — connecting anyway");
    }
    sola_core::env::activate_gpu_env();
    tracing::debug!("nixos gpu dispatch env activated");
    crate::fonts::ensure_system_fonts();
    if std::env::var_os("SOLA_NO_SELF_WATCH").is_none() {
        sola_core::watcher::watch_own_binary();
    } else {
        tracing::debug!("SOLA_NO_SELF_WATCH set, skipping self-watch");
    }
    socket
}

/// Iced subscription that drains the kit's bus client into a stream of
/// `Arc<Message>`. Apps wire it into their `subscription` callback and
/// `.map(...)` into their own message enum:
///
/// ```ignore
/// fn subscription(&self) -> Subscription<Msg> {
///     sola_kit::app::bus_subscription().map(Msg::Bus)
/// }
/// ```
///
/// Internally spawns a polling thread (the bus client's sync `recv`
/// API doesn't expose a futures-friendly stream) that forwards every
/// arriving message into an unbounded channel; the channel feeds
/// iced's `stream::channel` so iced wakes the runtime on each event.
/// Polling cadence is 8ms — keeps latency below a 120Hz frame budget
/// while staying cheap when no traffic is flowing.
///
/// Use this OR a manual `bus().lock().try_recv()` loop, not both —
/// the bus client has a single receiver per process.
pub fn bus_subscription() -> Subscription<Arc<Message>> {
    Subscription::run(bus_stream)
}

/// The action id the kit's default app-menu Quit item carries, and the
/// string [`is_self_quit`] matches. One constant instead of the `"quit"`
/// magic string repeated across every consumer.
pub const QUIT_ACTION_ID: &str = "quit";

/// Apply a bus delivery to an app's iced theme. If `message` is a
/// `Topic::Theme`, rebuilds `*theme` via [`crate::theme::theme_from_bus`]
/// **and** installs the font role table via
/// [`crate::theme::fonts_from_bus_theme`] — the explicit pairing that
/// replaced `theme_from_bus`'s old hidden font side-effect — then returns
/// `true`. Otherwise leaves `*theme` untouched and returns `false`.
///
/// ```ignore
/// Msg::Bus(msg) => { sola_kit::app::apply_theme_update(&msg, &mut self.theme); }
/// ```
pub fn apply_theme_update(message: &Message, theme: &mut iced::Theme) -> bool {
    apply_theme_topic(&Topic::parse(message), theme)
}

fn apply_theme_topic(topic: &Option<Topic>, theme: &mut iced::Theme) -> bool {
    if let Some(Topic::Theme(bus)) = topic {
        *theme = crate::theme::theme_from_bus(bus);
        crate::fonts::install(crate::theme::fonts_from_bus_theme(bus));
        crate::theme::install_selection(crate::theme::atoms_from_bus_theme(bus).selection);
        true
    } else {
        false
    }
}

/// Whether a bus delivery asks *this* app to quit: either its own
/// `MenuAction(app_id, "quit")` (the Cmd+Q path) or a `CloseApp(app_id)`
/// addressed to it. Handles both so consumers don't reinvent (and get
/// wrong) the pair, and removes the `"quit"` magic string
/// ([`QUIT_ACTION_ID`]).
pub fn is_self_quit(message: &Message, app_id: &str) -> bool {
    matches_self_quit(&Topic::parse(message), app_id)
}

fn matches_self_quit(topic: &Option<Topic>, app_id: &str) -> bool {
    match topic {
        Some(Topic::MenuAction(p)) => p.app_id == app_id && p.action_id == QUIT_ACTION_ID,
        Some(Topic::CloseApp(id)) => id.as_str() == app_id,
        _ => false,
    }
}

/// Guards the single bus poller — the client has one process receiver,
/// so at most one polling thread may run. Set while a [`bus_stream`]
/// thread is alive, cleared when it exits.
static POLLER_ACTIVE: AtomicBool = AtomicBool::new(false);
fn bus_stream() -> impl Stream<Item = Arc<Message>> {
    let (tx, rx) = iced::futures::channel::mpsc::unbounded::<Arc<Message>>();

    // The bus client has a single process receiver, so at most one
    // polling thread may drain it. If a poller is already running (a
    // second `bus_subscription` in the same process), refuse rather than
    // race `try_recv` and split the message stream.
    if POLLER_ACTIVE.swap(true, Ordering::AcqRel) {
        tracing::warn!(
            "bus_subscription started while a poller is already active; \
             returning an empty stream (one receiver per process)"
        );
        drop(tx);
        return rx;
    }

    std::thread::spawn(move || {
        loop {
            // Exit as soon as the iced side drops the subscription, so a
            // dropped subscription stops draining the bus instead of
            // looping forever on failed sends.
            if tx.is_closed() {
                break;
            }
            let next = match bus().lock() {
                Ok(guard) => {
                    guard.drain_notify();
                    guard.try_recv()
                }
                Err(poisoned) => {
                    // A peer thread panicked holding the bus mutex.
                    // Recover the client and keep delivering rather than
                    // panicking the poller (which would silently stop all
                    // bus events — violating "never lose output"). Clear
                    // the poison so we don't re-warn every tick.
                    tracing::warn!("bus mutex poisoned; recovering and continuing");
                    let guard = poisoned.into_inner();
                    guard.drain_notify();
                    let out = guard.try_recv();
                    bus().clear_poison();
                    out
                }
            };
            match next {
                Some(msg) => {
                    if tx.unbounded_send(Arc::new(msg)).is_err() {
                        break;
                    }
                }
                None => std::thread::sleep(Duration::from_millis(8)),
            }
        }
        POLLER_ACTIVE.store(false, Ordering::Release);
    });

    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_bus::topics::MenuActionPayload;

    fn menu_action(app_id: &str, action_id: &str) -> Option<Topic> {
        Some(Topic::MenuAction(MenuActionPayload {
            app_id: app_id.to_string(),
            action_id: action_id.to_string(),
        }))
    }

    #[test]
    fn self_menu_quit_matches() {
        assert!(matches_self_quit(&menu_action("sola-foo", "quit"), "sola-foo"));
    }

    #[test]
    fn other_app_menu_quit_ignored() {
        assert!(!matches_self_quit(&menu_action("sola-bar", "quit"), "sola-foo"));
    }

    #[test]
    fn non_quit_action_ignored() {
        assert!(!matches_self_quit(&menu_action("sola-foo", "reload"), "sola-foo"));
    }

    // C3 regression: a CloseApp addressed to us must count as a self-quit
    // (monitor used to ignore it on the bus).
    #[test]
    fn self_close_app_matches() {
        assert!(matches_self_quit(
            &Some(Topic::CloseApp("sola-foo".to_string())),
            "sola-foo"
        ));
    }

    #[test]
    fn other_close_app_ignored() {
        assert!(!matches_self_quit(
            &Some(Topic::CloseApp("sola-bar".to_string())),
            "sola-foo"
        ));
    }

    #[test]
    fn theme_topic_is_not_quit() {
        assert!(!matches_self_quit(&Some(Topic::Theme(Default::default())), "sola-foo"));
    }

    #[test]
    fn apply_theme_topic_applies_theme_delivery() {
        let mut theme = iced::Theme::Light;
        assert!(apply_theme_topic(&Some(Topic::Theme(Default::default())), &mut theme));
        assert!(matches!(theme, iced::Theme::Custom(_)));
    }

    #[test]
    fn apply_theme_topic_ignores_non_theme() {
        let mut theme = iced::Theme::Light;
        assert!(!apply_theme_topic(&menu_action("sola-foo", "quit"), &mut theme));
        assert!(matches!(theme, iced::Theme::Light));
    }
}
