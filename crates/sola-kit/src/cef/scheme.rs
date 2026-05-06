//! `app://` scheme handler. Bridges CEF's resource model to our
//! AssetBundle + swc TS+JSX transform.
//!
//! For storybook scope: a single static registration holds the most
//! recently registered window's bundle + HTML. Multi-window apps (e.g.
//! sola-browser) will need a per-host or per-browser dispatch — out of
//! scope for C1.

use std::sync::Mutex;

// `wrap_scheme_handler_factory!` and `wrap_resource_handler!` expand to code
// that references bare names from the cef crate (SchemeHandlerFactory,
// ResourceHandler, WrapSchemeHandlerFactory, ImplSchemeHandlerFactory,
// WrapResourceHandler, ImplResourceHandler, RcImpl, etc.). They must be in
// scope via wildcard import, mirroring the macro docstring example.
#[allow(unused_imports)]
use cef::{rc::*, *};

use crate::assets::AssetBundle;
use crate::strip::transform;

struct RegisteredWindow {
    app_assets: &'static AssetBundle,
    html: String,
}

static REGISTRATION: Mutex<Option<RegisteredWindow>> = Mutex::new(None);

/// Register (or replace) the active app:// scheme target. Called from
/// `ctx::add_window` before the Browser navigates to app:///index.html.
///
/// For storybook scope this is single-window — a second call warns and
/// replaces the previous registration. Multi-window dispatch is post-C1.
pub fn register_window(app_assets: &'static AssetBundle, html: String) {
    let mut guard = REGISTRATION.lock().expect("scheme REGISTRATION poisoned");
    if guard.is_some() {
        tracing::warn!(
            "cef::scheme: replacing existing app:// registration \
             (multi-window scope is post-C1)"
        );
    }
    *guard = Some(RegisteredWindow { app_assets, html });
}

// ── Factory ───────────────────────────────────────────────────────────────────

cef::wrap_scheme_handler_factory! {
    pub struct AppSchemeFactory {}

    impl SchemeHandlerFactory {
        fn create(
            &self,
            _browser: Option<&mut cef::Browser>,
            _frame: Option<&mut cef::Frame>,
            _scheme_name: Option<&CefString>,
            request: Option<&mut cef::Request>,
        ) -> Option<ResourceHandler> {
            // Extract the path from the URL, stripping scheme + authority.
            // Example: "app:///src/main.tsx" → "/src/main.tsx"
            //          "app:///index.html"  → "/index.html"
            //          "app:///"            → "/"
            // `url()` returns `CefStringUserfreeUtf16` which has no Display.
            // Convert via `CefStringUtf16` (which does implement Display).
            let url_str: String = match request.as_ref() {
                Some(r) => {
                    let userfree = r.url();
                    CefStringUtf16::from(&userfree).to_string()
                }
                None => String::new(),
            };

            // Strip "app://<authority>" prefix to get "<path>[?query][#frag]".
            let after_scheme = url_str.strip_prefix("app://").unwrap_or(&url_str);
            let path_and_rest = match after_scheme.find('/') {
                Some(i) => &after_scheme[i..],
                None => "/",
            };
            // Drop query and fragment.
            let path = path_and_rest
                .split('?')
                .next()
                .unwrap_or("/")
                .split('#')
                .next()
                .unwrap_or("/");
            let path = if path.is_empty() { "/" } else { path };

            let guard = REGISTRATION.lock().expect("scheme REGISTRATION poisoned");
            let reg = match guard.as_ref() {
                Some(r) => r,
                None => {
                    tracing::warn!(
                        path,
                        "cef::scheme: request arrived before any window was registered"
                    );
                    return Some(make_string_resource(
                        "No window registered".to_string(),
                        "text/plain",
                    ));
                }
            };

            tracing::debug!(url = %url_str, path, "cef::scheme: incoming request");

            // index.html — return the pre-built HTML (bootstrap + import map
            // + initial state already injected by ctx::add_window).
            if path == "/" || path == "/index.html" {
                return Some(make_string_resource(reg.html.clone(), "text/html"));
            }

            // Asset lookup: app bundle first, platform assets as fallback.
            let platform = crate::assets::platform_assets();
            let asset = reg.app_assets.find(path).or_else(|| platform.find(path));

            match asset {
                Some(asset) => {
                    let body = if asset.content_type.has_jsx()
                        || asset.content_type.has_types()
                    {
                        transform(
                            asset.content,
                            asset.content_type.has_jsx(),
                            asset.content_type.has_types(),
                        )
                    } else {
                        asset.content.to_string()
                    };
                    let mime = asset.content_type.mime().to_string();
                    Some(make_string_resource(body, mime))
                }
                None => {
                    tracing::warn!(path, "cef::scheme: 404");
                    Some(make_string_resource("Not Found".to_string(), "text/plain"))
                }
            }
        }
    }
}

// ── ResourceHandler — string-backed ──────────────────────────────────────────

/// Construct a `ResourceHandler` that serves `body` as `mime`.
///
/// The `wrap_resource_handler!` macro's generated `StringResource::new()`
/// takes all three fields including `pos`. This helper hides the
/// `Arc<Mutex<usize>>` boilerplate from the factory.
fn make_string_resource(body: String, mime: impl Into<String>) -> ResourceHandler {
    StringResource::new(
        body.into_bytes(),
        mime.into(),
        std::sync::Arc::new(Mutex::new(0usize)),
    )
}

cef::wrap_resource_handler! {
    pub struct StringResource {
        body: Vec<u8>,
        mime: String,
        // Arc<Mutex<>> makes the generated Clone impl work (Mutex alone is not Clone).
        pos: std::sync::Arc<Mutex<usize>>,
    }

    impl ResourceHandler {
        // open() is called by CEF to ask if we can handle this request.
        // Return 1 (true) and set *handle_request = 1 for synchronous handling.
        fn open(
            &self,
            _request: Option<&mut cef::Request>,
            handle_request: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut cef::Callback>,
        ) -> ::std::os::raw::c_int {
            if let Some(h) = handle_request {
                *h = 1;
            }
            1 // true: handled synchronously, no async callback needed
        }

        // response_headers() fills in the HTTP status, MIME type, charset,
        // and an explicit Content-Type header. Setting all three is
        // belt-and-braces — different CEF call paths (CEF's own
        // resource-pipeline checks vs Chromium's downstream sniffing) read
        // different fields, and a missing one can result in plaintext
        // fallback rendering of HTML.
        fn response_headers(
            &self,
            response: Option<&mut cef::Response>,
            response_length: Option<&mut i64>,
            _redirect_url: Option<&mut CefString>,
        ) {
            tracing::debug!(mime = %self.mime, len = self.body.len(), "cef::scheme: response_headers");
            if let Some(r) = response {
                r.set_status(200);
                let status_text = CefString::from("OK");
                r.set_status_text(Some(&status_text));
                let mime_cef = CefString::from(self.mime.as_str());
                r.set_mime_type(Some(&mime_cef));
                let utf8 = CefString::from("utf-8");
                r.set_charset(Some(&utf8));
                let ct_name = CefString::from("Content-Type");
                let ct_value = CefString::from(
                    format!("{}; charset=utf-8", self.mime).as_str(),
                );
                r.set_header_by_name(Some(&ct_name), Some(&ct_value), 1);
            }
            if let Some(len) = response_length {
                *len = self.body.len() as i64;
            }
        }

        // read() copies up to bytes_to_read bytes into data_out.
        // Returns 1 (true) while there is data; 0 (false) on EOF.
        fn read(
            &self,
            data_out: *mut u8,
            bytes_to_read: ::std::os::raw::c_int,
            bytes_read: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut cef::ResourceReadCallback>,
        ) -> ::std::os::raw::c_int {
            let mut pos = self.pos.lock().expect("StringResource pos poisoned");
            let remaining = self.body.len().saturating_sub(*pos);
            if remaining == 0 {
                if let Some(n) = bytes_read {
                    *n = 0;
                }
                return 0; // false — EOF
            }
            let n = remaining.min(bytes_to_read.max(0) as usize);
            // SAFETY: data_out is provided by CEF and points to a writable
            // buffer of at least bytes_to_read bytes. We write exactly n ≤
            // bytes_to_read bytes starting at *pos.
            unsafe {
                std::ptr::copy_nonoverlapping(self.body[*pos..].as_ptr(), data_out, n);
            }
            *pos += n;
            if let Some(out) = bytes_read {
                *out = n as ::std::os::raw::c_int;
            }
            1 // true — still has data (or just finished; CEF re-calls until 0)
        }

        fn cancel(&self) {
            // Nothing to clean up for an in-memory buffer.
        }
    }
}
