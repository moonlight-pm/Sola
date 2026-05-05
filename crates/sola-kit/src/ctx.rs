use std::cell::RefCell;
use std::rc::Rc;

use sola_bus::BusClient;
use sola_bus::topics::{Topic, Window};

use crate::cef::Browser;
use crate::wayland::{Surface, WaylandClient};
use crate::window::{JsDispatcher, WindowConfig, WindowHandle, WindowInner};

/// Effect handle passed to every `SolaApp` trait method.
/// Holds the bus, the Wayland client, and the list of live windows.
pub struct AppCtx {
    pub(crate) bus: Rc<RefCell<BusClient>>,
    pub(crate) wayland: Rc<RefCell<WaylandClient>>,
    pub(crate) windows: Vec<WindowHandle>,
    pub(crate) app_id: &'static str,
    /// Latest `Windows` sticky snapshot, used by the framework to correlate
    /// window_ids in bus topics (e.g. Copy/Paste) back to a `WindowHandle`.
    pub(crate) known_windows: Vec<Window>,
}

impl AppCtx {
    pub(crate) fn new(
        bus: Rc<RefCell<BusClient>>,
        wayland: Rc<RefCell<WaylandClient>>,
        app_id: &'static str,
    ) -> Self {
        Self {
            bus,
            wayland,
            windows: Vec::new(),
            app_id,
            known_windows: Vec::new(),
        }
    }

    /// Create a new window: pair a Wayland surface with a CEF browser.
    ///
    /// Builds the final HTML for this window — index.html lookup,
    /// `__RESTORED_STATE__` substitution, bootstrap script injection,
    /// and import map injection — then registers it with the `app://`
    /// scheme handler before creating the browser. The browser navigates
    /// to `app:///index.html`, where the scheme handler serves the
    /// pre-built HTML and all assets from `cfg.assets` (with TS/JSX
    /// transform applied on-demand).
    pub fn add_window(&mut self, cfg: WindowConfig) -> WindowHandle {
        let dispatcher_slot: Rc<RefCell<Option<JsDispatcher>>> = Rc::new(RefCell::new(None));
        let loaded: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
        let pending: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

        // Build the HTML that the scheme handler will serve for /index.html.
        let html_raw = cfg
            .assets
            .find("/index.html")
            .map(|a| a.content.to_string())
            .unwrap_or_else(|| "<html><body>No index.html in bundle</body></html>".to_string());

        // Substitute __RESTORED_STATE__ with the window's initial state JSON
        // (if any), then inject the queueing bootstrap stub and import map.
        let html = if let Some(state_json) = cfg.initial_state.as_ref() {
            html_raw.replace("__RESTORED_STATE__", state_json)
        } else {
            html_raw
        };
        let html = crate::inject_solarecv_bootstrap(&html);
        let html = crate::inject_import_map(&html);

        // Register the bundle + HTML with the static scheme handler before
        // the browser is created so the first navigation is served correctly.
        crate::cef::scheme::register_window(cfg.assets, html);

        let surface = Surface::new(self.wayland.clone(), &cfg);
        let browser = Browser::new(surface.clone(), "app:///index.html");

        let inner = WindowInner {
            title: cfg.title,
            surface,
            browser,
            dispatcher: dispatcher_slot,
            loaded,
            pending,
        };

        let handle = WindowHandle { inner: Rc::new(inner) };
        self.windows.push(handle.clone());
        handle
    }

    /// Close and remove a window. (Surface drop → wl_surface destroy;
    /// Browser drop → cef close; sctk + cef handle teardown internally.)
    pub fn remove_window(&mut self, handle: &WindowHandle) {
        self.windows.retain(|w| w != handle);
    }

    /// Emit a bus event. Sticky/persistent semantics are determined by
    /// the topic kind's `Behavior`.
    pub fn emit(&self, topic: Topic) {
        let _ = self.bus.borrow_mut().emit(topic);
    }

    /// Retract a sticky bus event. Symmetric to [`emit`](Self::emit).
    pub fn retract(&self, topic: Topic) {
        let _ = self.bus.borrow_mut().retract(topic);
    }

    /// Trigger a clean shutdown: posts `cef::quit_message_loop()` so
    /// `cef::run_message_loop` returns and `lib.rs::run<A>` proceeds to
    /// `cef::shutdown()`.
    pub fn shutdown(&self) {
        cef::quit_message_loop();
    }

    /// Resolve a `window_id` (as seen on the bus) to one of *this process's*
    /// owned `WindowHandle`s. Returns `None` if the id doesn't belong to us.
    pub(crate) fn find_window_by_id(&self, window_id: u32) -> Option<&WindowHandle> {
        let entry = self
            .known_windows
            .iter()
            .find(|a| a.window_id == window_id)?;
        if entry.app_id != self.app_id {
            return None;
        }
        self.windows.iter().find(|w| w.title() == entry.title)
    }

    /// Return a Clone-able handle to the bus client. Use this when you need
    /// to emit topics from a CEF-thread closure that outlives the `&mut
    /// AppCtx` borrow. Not Send — must run on the CEF UI thread.
    pub fn bus_proxy(&self) -> BusProxy {
        BusProxy { bus: self.bus.clone() }
    }
}

/// Cheap clone of the bus client, usable from any CEF UI-thread closure
/// (not Send — runs on the CEF UI thread).
#[derive(Clone)]
pub struct BusProxy {
    bus: Rc<RefCell<BusClient>>,
}

impl BusProxy {
    pub fn emit(&self, topic: Topic) {
        let _ = self.bus.borrow_mut().emit(topic);
    }

    pub fn retract(&self, topic: Topic) {
        let _ = self.bus.borrow_mut().retract(topic);
    }
}
