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

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use sola_bus::BusClient;
use sola_bus::topics::{
    AppMenuPayload, MenuDefinition, MenuItem, Topic, TopicKind,
};
use sola_core::KeyChord;

/// Wayland xdg_toplevel.app_id and bus app id, single source of truth
/// per app. Apps store this as `const APP_ID: &str = "sola-foo";`
/// and pass it through [`BusSetup::connect`] + iced window settings.
pub trait App {
    const APP_ID: &'static str;
}

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
    app_menu: Option<MenuDefinition>,
    connect_timeout: Duration,
}

impl BusSetup {
    pub fn new(app_id: &'static str) -> Self {
        Self {
            app_id,
            subscribe: None,
            app_menu: None,
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
        self.app_menu = Some(def);
        self
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
        if let Some(menu) = self.app_menu {
            let _ = client.emit(Topic::SetAppMenu(AppMenuPayload {
                app_id: self.app_id.into(),
                menus: vec![menu],
            }));
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
/// 2. `sola_core::env::activate_wayland_session(20s)`
///
/// Returns the resolved Wayland socket name so the caller can log it.
/// Apps that need different log init or a different Wayland timeout
/// should skip this and call the underlying helpers themselves.
pub fn startup(app_id: &str) -> String {
    sola_core::log::init(app_id);
    tracing::info!("{app_id} starting");
    let socket = sola_core::env::activate_wayland_session(20_000);
    tracing::info!(socket = %socket, "wayland socket resolved");
    socket
}

/// Placeholder for a future `sola_kit::run::<A>()` that owns iced's
/// `application` builder end-to-end. Today it just calls
/// [`startup`] — apps still build their own iced application by
/// hand because each one wants different update/view/subscription
/// types and a generic wrapper is more friction than it saves.
/// Promote logic here once we have a second app to compare against.
pub fn run<A: App>() -> String {
    startup(A::APP_ID)
}
