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
    /// The browser navigates to `app:///index.html`; the cef::scheme
    /// handler (registered separately, see C1) serves the HTML and
    /// embedded assets via the AssetBundle in `cfg.assets`.
    pub fn add_window(&mut self, cfg: WindowConfig) -> WindowHandle {
        let dispatcher_slot: Rc<RefCell<Option<JsDispatcher>>> = Rc::new(RefCell::new(None));
        let loaded: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
        let pending: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

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
