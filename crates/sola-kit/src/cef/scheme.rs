//! `app://` scheme handler. Bridges CEF's resource model to our
//! AssetBundle + swc TS+JSX transform.
//!
//! Multi-window apps (sola-shell, eventually sola-browser) need
//! per-window dispatch — each `add_window` registers its own bundle
//! and pre-built HTML, and the browser navigates to a window-scoped
//! URL like `app://win3/index.html`. The scheme handler keys
//! registrations by the URL host segment, so concurrent windows
//! never trample each other.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

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

static REGISTRATIONS: Mutex<Option<HashMap<String, RegisteredWindow>>> = Mutex::new(None);
static NEXT_HOST_ID: AtomicU64 = AtomicU64::new(0);

/// Register a window's bundle + pre-built HTML under a freshly
/// allocated URL host. Returns the host string; the caller is
/// expected to navigate the browser to `app://<host>/index.html`.
///
/// Each window gets its own host slot, so the scheme handler can
/// dispatch by URL host instead of trampling on a single global.
pub fn register_window(app_assets: &'static AssetBundle, html: String) -> String {
    let id = NEXT_HOST_ID.fetch_add(1, Ordering::Relaxed);
    let host = format!("win{id}");
    let mut guard = REGISTRATIONS.lock().expect("scheme REGISTRATIONS poisoned");
    guard
        .get_or_insert_with(HashMap::new)
        .insert(host.clone(), RegisteredWindow { app_assets, html });
    host
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

            // Split "app://<host>/<path>[?query][#frag]" into host + path.
            // Host is the registration key (allocated by `register_window`);
            // path is what we look up in the bundle.
            let after_scheme = url_str.strip_prefix("app://").unwrap_or(&url_str);
            let (host, path_and_rest) = match after_scheme.find('/') {
                Some(i) => (&after_scheme[..i], &after_scheme[i..]),
                None => (after_scheme, "/"),
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

            let guard = REGISTRATIONS.lock().expect("scheme REGISTRATIONS poisoned");
            let reg = match guard.as_ref().and_then(|m| m.get(host)) {
                Some(r) => r,
                None => {
                    tracing::warn!(
                        host,
                        path,
                        "cef::scheme: no registration for host"
                    );
                    return Some(make_resource(
                        b"No window registered".to_vec(),
                        "text/plain",
                    ));
                }
            };

            tracing::debug!(url = %url_str, path, "cef::scheme: incoming request");

            // index.html — return the pre-built HTML (bootstrap + import map
            // + initial state already injected by ctx::add_window).
            if path == "/" || path == "/index.html" {
                return Some(make_resource(reg.html.clone().into_bytes(), "text/html"));
            }

            // /sola-assets/<rel-path> — disk-backed assets under
            // `/opt/sola/share/...`. Routed to `sola_assets::resolve` so
            // kit apps can reference fonts, icons, and other shared media
            // via `app://<host>/sola-assets/...` URLs. Returns 404 if the
            // file is missing or path-traversal is attempted.
            if let Some(rel) = path.strip_prefix("/sola-assets/") {
                if rel.contains("..") {
                    tracing::warn!(path, "cef::scheme: rejected /sola-assets path traversal");
                    return Some(make_resource(b"Not Found".to_vec(), "text/plain"));
                }
                match sola_assets::resolve(rel).and_then(|p| std::fs::read(&p).ok()) {
                    Some(bytes) => {
                        let mime = mime_for_path(rel);
                        return Some(make_resource(bytes, mime));
                    }
                    None => {
                        tracing::warn!(path, "cef::scheme: sola-assets file not found");
                        return Some(make_resource(b"Not Found".to_vec(), "text/plain"));
                    }
                }
            }

            // Asset lookup: app bundle first, platform assets as fallback.
            let platform = crate::assets::platform_assets();
            let asset = reg.app_assets.find(path).or_else(|| platform.find(path));

            match asset {
                Some(asset) => {
                    let body: Vec<u8> = if asset.content_type.has_jsx()
                        || asset.content_type.has_types()
                    {
                        // swc operates on UTF-8 source; our embedded TS/JSX
                        // is UTF-8 by construction (we control the inputs).
                        // A non-UTF-8 byte here would mean a corrupted asset
                        // bundle, which is a bug — panic loudly.
                        let src = std::str::from_utf8(asset.content)
                            .expect("non-UTF-8 source in TS/JSX asset");
                        transform(
                            src,
                            asset.content_type.has_jsx(),
                            asset.content_type.has_types(),
                        )
                        .into_bytes()
                    } else {
                        asset.content.to_vec()
                    };
                    let mime = asset.content_type.mime().to_string();
                    Some(make_resource(body, mime))
                }
                None => {
                    tracing::warn!(path, "cef::scheme: 404");
                    Some(make_resource(b"Not Found".to_vec(), "text/plain"))
                }
            }
        }
    }
}

// ── ResourceHandler — bytes-backed ───────────────────────────────────────────

/// Construct a `ResourceHandler` that serves `body` as `mime`.
///
/// The `wrap_resource_handler!` macro's generated `StringResource::new()`
/// takes all three fields including `pos`. This helper hides the
/// `Arc<Mutex<usize>>` boilerplate from the factory.
/// Pick a sensible Content-Type for a disk-served `/sola-assets/`
/// file. Limited to the formats the kit's bundled assets actually
/// expose (fonts, SVG icons, common bitmaps). Unknown extensions get
/// `application/octet-stream`, which is fine for non-decoded payloads.
fn mime_for_path(path: &str) -> &'static str {
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

fn make_resource(body: Vec<u8>, mime: impl Into<String>) -> ResourceHandler {
    StringResource::new(
        body,
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
