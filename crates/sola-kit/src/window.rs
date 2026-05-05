use std::cell::RefCell;
use std::rc::Rc;

use serde_json::Value;

use crate::assets::AssetBundle;
use crate::cef::Browser;
use crate::wayland::Surface;

/// Declarative window configuration passed to `AppCtx::add_window`.
pub struct WindowConfig {
    pub title: String,
    pub size: (i32, i32),
    pub position: Option<(i32, i32)>,
    pub decorated: bool,
    pub transparent: bool,
    pub assets: &'static AssetBundle,
    pub initial_state: Option<String>,
    /// WindowPolicy: whether the compositor's zoning system manages this window.
    pub zoned: bool,
    /// WindowPolicy: whether this window can receive keyboard input.
    pub keyboard_target: bool,
}

/// JS dispatcher installed per window by the runtime after `A::new`.
/// The `Option<u64>` is the request id used to correlate replies.
pub type JsDispatcher = Box<dyn FnMut(&str, &Value, Option<u64>)>;

/// Internal per-window state owned by sola-kit.
pub(crate) struct WindowInner {
    pub(crate) title: String,
    pub(crate) surface: Rc<Surface>,
    pub(crate) browser: Browser,
    /// Shared with the JS bridge: the cef::ipc handler reads, the
    /// runtime writes after `A::new` returns.
    pub(crate) dispatcher: Rc<RefCell<Option<JsDispatcher>>>,
    /// Set to `true` after the browser fires LoadHandler::OnLoadEnd.
    /// Until then, `eval_js` queues into `pending` so messages emitted
    /// in response to replayed sticky topics aren't lost on a window
    /// that hasn't even loaded our HTML yet. (LoadHandler wiring lands
    /// in a follow-up; for now `loaded` stays false and queued messages
    /// accumulate.)
    pub(crate) loaded: Rc<RefCell<bool>>,
    pub(crate) pending: Rc<RefCell<Vec<String>>>,
}

#[derive(Clone)]
pub struct WindowHandle {
    pub(crate) inner: Rc<WindowInner>,
}

impl WindowHandle {
    pub fn title(&self) -> &str {
        &self.inner.title
    }

    pub fn eval_js(&self, script: &str) {
        if !*self.inner.loaded.borrow() {
            // Browser hasn't fired OnLoadEnd yet — queue, drain on Finished.
            self.inner.pending.borrow_mut().push(script.to_string());
            return;
        }
        self.inner.browser.execute_js(script);
    }

    /// Send a JSON value to the frontend's `window.__solaRecv`. The frontend
    /// expects a JSON *string* (it calls `JSON.parse` on the argument), so
    /// this double-stringifies: once to turn the value into JSON, once to
    /// encode that JSON as a JS string literal.
    pub fn send_to_js(&self, value: &Value) {
        let json_str = serde_json::to_string(value).unwrap_or_default();
        self.send_raw_json_to_js(&json_str);
    }

    pub fn send_raw_json_to_js(&self, json: &str) {
        let js_literal = serde_json::to_string(json).unwrap_or_default();
        self.eval_js(&format!("window.__solaRecv({js_literal})"));
    }

    /// Access the underlying Surface (Wayland surface + xdg_toplevel).
    pub fn surface(&self) -> &Rc<Surface> {
        &self.inner.surface
    }

    /// Access the underlying Browser (CEF wrapper).
    pub fn browser(&self) -> &Browser {
        &self.inner.browser
    }
}

impl PartialEq for WindowHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for WindowHandle {}
