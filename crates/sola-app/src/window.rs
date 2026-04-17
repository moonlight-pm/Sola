use std::cell::RefCell;
use std::rc::Rc;

use serde_json::Value;

use crate::assets::AssetBundle;

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
/// Converts a UCM script message into `SolaApp::on_js_command`.
/// The `Option<u64>` is the request id used to correlate replies.
pub type JsDispatcher = Box<dyn FnMut(&str, &Value, Option<u64>)>;

/// Internal per-window state owned by sola-app.
pub(crate) struct WindowInner {
    pub(crate) title: String,
    pub(crate) webview: webkit6::WebView,
    pub(crate) gtk_window: gtk4::ApplicationWindow,
    /// Shared with the UCM handler: the UCM reads from this slot, the
    /// runtime writes into it after `A::new` returns.
    pub(crate) dispatcher: Rc<RefCell<Option<JsDispatcher>>>,
    pub(crate) zoned: bool,
    pub(crate) keyboard_target: bool,
    pub(crate) size: (i32, i32),
    pub(crate) position: Option<(i32, i32)>,
}

/// Cheap-clone handle to a window created via `AppCtx::add_window`.
#[derive(Clone)]
pub struct WindowHandle {
    pub(crate) inner: Rc<WindowInner>,
}

impl WindowHandle {
    pub fn title(&self) -> &str {
        &self.inner.title
    }

    pub fn eval_js(&self, script: &str) {
        use webkit6::prelude::WebViewExt;
        self.inner.webview.evaluate_javascript(
            script,
            None,
            None,
            None::<&gio::Cancellable>,
            |_| {},
        );
    }

    /// Send a JSON value to the frontend's `window.__solaRecv`. The frontend
    /// expects a JSON *string* (it calls `JSON.parse` on the argument), so
    /// this double-stringifies: once to turn the value into JSON, once to
    /// encode that JSON as a JS string literal.
    pub fn send_to_js(&self, value: &Value) {
        let json_str = serde_json::to_string(value).unwrap_or_default();
        self.send_raw_json_to_js(&json_str);
    }

    /// Variant of `send_to_js` for callers that already have a JSON-encoded
    /// string to forward (e.g. messages coming off an mpsc channel).
    pub fn send_raw_json_to_js(&self, json: &str) {
        let js_literal = serde_json::to_string(json).unwrap_or_default();
        self.eval_js(&format!("window.__solaRecv({js_literal})"));
    }

    /// Access the underlying GTK window for event controllers etc.
    pub fn gtk_window(&self) -> &gtk4::ApplicationWindow {
        &self.inner.gtk_window
    }

    /// Access the underlying WebKit WebView. Apps that need to restructure
    /// the window's widget tree (e.g. reparent the WebView into a container
    /// to add sibling WebViews) use this. The JS dispatcher / UCM stays
    /// attached to the WebView across reparenting.
    pub fn webview(&self) -> &webkit6::WebView {
        &self.inner.webview
    }
}

impl PartialEq for WindowHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for WindowHandle {}
