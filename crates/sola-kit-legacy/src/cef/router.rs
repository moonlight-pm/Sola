//! cefQuery (JS) ↔ Rust IPC bridge.
//!
//! CEF's MessageRouter has two halves:
//!
//!   - **Browser side** — lives in our main process. Receives query strings
//!     forwarded from the renderer, dispatches them to the per-window
//!     `JsDispatcher` registered in [`register_window`], and acks the
//!     query so the JS `cefQuery` promise's `onSuccess` fires.
//!   - **Renderer side** — lives in each renderer subprocess (CEF re-execs
//!     this binary as `--type=renderer`). Installs the `window.cefQuery`
//!     and `window.cefQueryCancel` JS functions on every V8 context, then
//!     forwards calls to the browser-side router via Chromium IPC.
//!
//! Replies to `invoke()` calls travel back via `__solaRecv` (the existing
//! `WindowHandle::send_to_js` path), not via cefQuery's `onSuccess`. We
//! call `callback.success_str("")` purely to satisfy cefQuery's contract
//! ("if you return true, you must call a callback method"). The id-based
//! correlation is handled by `web/lib/ipc.ts`'s `pending` map.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};

#[allow(unused_imports)]
use cef::{rc::*, *};
use cef::wrapper::message_router::{
    BrowserSideCallback, BrowserSideHandler, BrowserSideRouter, MessageRouterBrowserSide,
    MessageRouterConfig, MessageRouterRendererSide,
    MessageRouterRendererSideHandlerCallbacks, RendererSideRouter,
};

use crate::window::JsDispatcher;

static BROWSER_ROUTER: OnceLock<Arc<BrowserSideRouter>> = OnceLock::new();
static RENDERER_ROUTER: OnceLock<Arc<RendererSideRouter>> = OnceLock::new();

thread_local! {
    /// Per-browser dispatcher registry. Keyed by `Browser::identifier()`
    /// (CEF's stable per-browser id, an `i32`). Populated by
    /// [`register_window`] and read by [`KitBrowserHandler::on_query_str`].
    /// Both run on the CEF UI thread, so the thread-local single-threaded
    /// HashMap is safe and avoids the Send/Sync gymnastics that would
    /// otherwise be needed to share an `Rc<RefCell<...>>` across the
    /// `BrowserSideHandler: Send + Sync` boundary.
    static WINDOWS: RefCell<HashMap<i32, Rc<RefCell<Option<JsDispatcher>>>>>
        = RefCell::new(HashMap::new());
}

/// Lazily-initialized browser-side router. Per-process singleton.
pub fn browser_router() -> Arc<BrowserSideRouter> {
    BROWSER_ROUTER
        .get_or_init(|| {
            let r = BrowserSideRouter::new(MessageRouterConfig::default());
            r.add_handler(Arc::new(KitBrowserHandler), false);
            r
        })
        .clone()
}

/// Lazily-initialized renderer-side router. Per-renderer-subprocess singleton.
/// Called from `KitRenderProcessHandler` (which only fires in renderer
/// processes), so the browser process never instantiates one.
pub fn renderer_router() -> Arc<RendererSideRouter> {
    RENDERER_ROUTER
        .get_or_init(|| RendererSideRouter::new(MessageRouterConfig::default()))
        .clone()
}

/// Associate a browser with its window's dispatcher slot. Call from the
/// CEF UI thread immediately after `browser_host_create_browser_sync`
/// returns. The slot is the same `Rc<RefCell<Option<JsDispatcher>>>`
/// stored on `WindowInner`; the runtime fills it in after `A::new`.
pub fn register_window(browser_id: i32, slot: Rc<RefCell<Option<JsDispatcher>>>) {
    WINDOWS.with(|w| {
        w.borrow_mut().insert(browser_id, slot);
    });
}

/// Drop a window's dispatcher slot. Call from `LifeSpanHandler::on_before_close`.
pub fn unregister_window(browser_id: i32) {
    WINDOWS.with(|w| {
        w.borrow_mut().remove(&browser_id);
    });
}

/// Browser-side handler: decodes the JSON query, looks up the window's
/// dispatcher, and forwards. Send+Sync because the trait demands it; in
/// practice CEF calls the methods on the UI thread (per the trait
/// docstring), so the thread-local registry above is actually accessed
/// single-threadedly.
struct KitBrowserHandler;

impl BrowserSideHandler for KitBrowserHandler {
    fn on_query_str(
        &self,
        browser: Option<cef::Browser>,
        _frame: Option<cef::Frame>,
        _query_id: i64,
        request: &str,
        _persistent: bool,
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
    ) -> bool {
        let Some(browser) = browser else {
            return false;
        };
        let browser_id = browser.identifier();

        // Parse `{ id?: number, cmd: string, args?: object }`.
        let parsed: serde_json::Value = match serde_json::from_str(request) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(?e, request, "cefQuery: malformed JSON");
                if let Ok(cb) = callback.lock() {
                    cb.failure(-1, &format!("malformed JSON: {e}"));
                }
                return true;
            }
        };
        let cmd = parsed.get("cmd").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if cmd.is_empty() {
            tracing::warn!(request, "cefQuery: missing cmd");
            if let Ok(cb) = callback.lock() {
                cb.failure(-1, "missing cmd");
            }
            return true;
        }
        let id = parsed.get("id").and_then(|v| v.as_u64());
        let args = parsed
            .get("args")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let dispatcher_slot = WINDOWS.with(|w| w.borrow().get(&browser_id).cloned());
        let Some(slot) = dispatcher_slot else {
            tracing::warn!(browser_id, cmd = %cmd, "cefQuery: no dispatcher for browser");
            if let Ok(cb) = callback.lock() {
                cb.failure(-1, "no dispatcher registered");
            }
            return true;
        };

        if let Some(dispatcher) = slot.borrow_mut().as_mut() {
            dispatcher(&cmd, &args, id);
        } else {
            tracing::debug!(browser_id, cmd = %cmd, "cefQuery: dispatcher slot empty");
        }

        // Ack the query so the JS-side `cefQuery` promise settles. The actual
        // reply (when `id` is Some) goes back through `__solaRecv` in
        // `app.on_js_command` → `source.send_to_js({"id": ..., "result": ...})`.
        if let Ok(cb) = callback.lock() {
            cb.success_str("");
        }
        true
    }
}

// ── Render-process-handler ─────────────────────────────────────────────────────
//
// Runs in each renderer subprocess. Wires the renderer-side router into V8
// context lifecycle and forwards inter-process messages.

cef::wrap_render_process_handler! {
    pub struct KitRenderProcessHandler {}

    impl RenderProcessHandler {
        fn on_context_created(
            &self,
            browser: Option<&mut cef::Browser>,
            frame: Option<&mut cef::Frame>,
            context: Option<&mut cef::V8Context>,
        ) {
            renderer_router().on_context_created(
                browser.cloned(),
                frame.cloned(),
                context.cloned(),
            );
        }

        fn on_context_released(
            &self,
            browser: Option<&mut cef::Browser>,
            frame: Option<&mut cef::Frame>,
            context: Option<&mut cef::V8Context>,
        ) {
            renderer_router().on_context_released(
                browser.cloned(),
                frame.cloned(),
                context.cloned(),
            );
        }

        fn on_process_message_received(
            &self,
            browser: Option<&mut cef::Browser>,
            frame: Option<&mut cef::Frame>,
            source_process: cef::ProcessId,
            message: Option<&mut cef::ProcessMessage>,
        ) -> ::std::os::raw::c_int {
            let handled = renderer_router().on_process_message_received(
                browser.cloned(),
                frame.cloned(),
                Some(source_process),
                message.cloned(),
            );
            if handled { 1 } else { 0 }
        }
    }
}
