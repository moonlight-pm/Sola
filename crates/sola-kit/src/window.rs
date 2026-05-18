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
    /// WindowPolicy: whether the compositor's zoning system manages this window.
    pub zoned: bool,
    /// WindowPolicy: whether this window can receive keyboard input.
    pub keyboard_target: bool,
    /// Per-window override for the Remix v3 root component path. When
    /// `None`, the kit falls back to `SolaApp::ROOT_COMPONENT`. The
    /// referenced file is served under `app://` from this window's
    /// asset bundle and must export a `Main` factory.
    pub root_component: Option<&'static str>,
    /// Per-window seed state. When `Some`, the kit injects
    /// `<script>window.__solaInitial = <json>;</script>` into the head
    /// of this window's `index.html`. The kit's `index.tsx` reads it
    /// synchronously and passes it as the `initial` prop to `Main`.
    /// When `None`, `window.__solaInitial` is set to `null`.
    pub initial_state: Option<serde_json::Value>,
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
        // Pre-load risk: `CefFrame::execute_java_script` calls issued before
        // the main frame commits a document may be dropped by Chromium. The
        // WebKit-era sola-app worked around this with a `pending: Vec<String>`
        // queue drained on LoadHandler::OnLoadEnd (see git history). We're
        // not wiring LoadHandler yet and no Rust→JS message is emitted before
        // user input, so the race is dormant — re-add the queue when bus
        // dispatch (D3/D5) starts replaying sticky topics at startup.
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
        // Self-defending: if a Rust→JS push lands before the
        // bootstrap inline `<script>` has parsed (e.g. sticky theme
        // replay at startup, racing the document load), `__solaRecv`
        // is undefined and `window.__solaRecv(json)` would throw.
        // Fall back to pushing into `__solaRecvQueue` directly,
        // creating it if needed; the bootstrap reuses any
        // pre-existing queue so the message is preserved.
        self.eval_js(&format!(
            "(window.__solaRecv || function(j){{ \
              (window.__solaRecvQueue = window.__solaRecvQueue || []).push(j); \
            }})({js_literal});"
        ));
    }

    /// Access the underlying Surface (Wayland surface + xdg_toplevel).
    pub fn surface(&self) -> &Rc<Surface> {
        &self.inner.surface
    }

    /// Access the underlying Browser (CEF wrapper).
    pub fn browser(&self) -> &Browser {
        &self.inner.browser
    }

    /// Toggle the Chromium DevTools panel for this window's browser.
    /// See `Browser::toggle_dev_tools` for the behaviour and the note
    /// about bottom-half integration as future work.
    pub fn toggle_dev_tools(&self) {
        self.inner.browser.toggle_dev_tools();
    }

    /// Forward an Edit-menu clipboard action to the focused frame.
    /// Used by the kit's menu-action handler so Cmd+C / Cmd+V (and
    /// their menu items) cut/copy/paste through CEF's normal editing
    /// pipeline — i.e. into whatever text input or contenteditable
    /// element holds focus.
    pub fn cut(&self) {
        self.inner.browser.cut();
    }
    pub fn copy(&self) {
        self.inner.browser.copy();
    }
    pub fn paste(&self) {
        self.inner.browser.paste();
    }
    pub fn select_all(&self) {
        self.inner.browser.select_all();
    }
}

impl PartialEq for WindowHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for WindowHandle {}
