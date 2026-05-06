use std::cell::RefCell;
use std::rc::Rc;

use sola_bus::BusClient;
use sola_bus::topics::Topic;

use crate::cef::Browser;
use crate::wayland::{Surface, WaylandClient};
use crate::window::{JsDispatcher, WindowConfig, WindowHandle, WindowInner};

/// Effect handle passed to every `SolaApp` trait method.
/// Holds the bus, the Wayland client, and the list of live windows.
pub struct AppCtx {
    pub(crate) bus: Rc<RefCell<BusClient>>,
    pub(crate) wayland: Rc<RefCell<WaylandClient>>,
    pub(crate) windows: Vec<WindowHandle>,
    /// `SolaApp::APP_ID` for this process. Reported to the compositor as
    /// `xdg_toplevel.app_id` (so window-manager rules + sola-river's
    /// per-app focus/zoning logic see the right id) and used by future
    /// bus-loop intercepts that need to filter messages addressed to us.
    pub(crate) app_id: &'static str,
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
        }
    }

    /// Create a new window: pair a Wayland surface with a CEF browser.
    ///
    /// Builds the final HTML for this window — index.html lookup,
    /// `__RESTORED_STATE__` substitution, and the queueing bootstrap
    /// script injection — then registers it with the `app://` scheme
    /// handler before creating the browser. The browser navigates to
    /// `app:///index.html`, where the scheme handler serves the
    /// pre-built HTML and all assets from `cfg.assets` (with TS/JSX
    /// transform applied on-demand). The app's `index.html` is responsible
    /// for declaring its own `<script type="importmap">` — the kit makes
    /// no assumption about the JS framework.
    pub fn add_window(&mut self, cfg: WindowConfig) -> WindowHandle {
        let dispatcher_slot: Rc<RefCell<Option<JsDispatcher>>> = Rc::new(RefCell::new(None));

        // Build the HTML that the scheme handler will serve for /index.html.
        let html_raw = cfg
            .assets
            .find("/index.html")
            .map(|a| a.content.to_string())
            .unwrap_or_else(|| "<html><body>No index.html in bundle</body></html>".to_string());

        let html = if let Some(state_json) = cfg.initial_state.as_ref() {
            html_raw.replace("__RESTORED_STATE__", state_json)
        } else {
            html_raw
        };
        let html = crate::inject_solarecv_bootstrap(&html);

        // Register the bundle + HTML with the static scheme handler before
        // the browser is created so the first navigation is served correctly.
        crate::cef::scheme::register_window(cfg.assets, html);

        let surface = Surface::new(self.wayland.clone(), &cfg, self.app_id);
        let browser = Browser::new(surface.clone(), "app:///index.html");

        // Wire this browser's identifier to its dispatcher slot so the
        // MessageRouter's `KitBrowserHandler::on_query_str` can route
        // cefQuery requests back to the right window.
        crate::cef::router::register_window(browser.identifier(), dispatcher_slot.clone());

        // Bind the browser to the surface so xdg configures can drive
        // `BrowserHost::was_resized`.
        surface.bind_browser(browser.inner.clone());

        let inner = WindowInner {
            title: cfg.title,
            surface,
            browser,
            dispatcher: dispatcher_slot,
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
