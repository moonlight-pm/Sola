//! Shared on-disk assets (icons, fonts, ...) for Sola.
//!
//! Every third-party asset lives at `/opt/sola/share/<category>/<pack>/...`
//! and is populated by `cargo make assets sync`. Nothing is committed to
//! the repo; nothing is rsynced by `install`. A clean clone auto-syncs
//! the first time `cargo make install` runs and is good for every
//! subsequent build.
//!
//! Nothing is compiled into consumer binaries — all data is read from disk.

use std::path::{Path, PathBuf};

pub mod icons;

pub const ASSETS_DIR: &str = "/opt/sola/share";

/// Resolve a relative asset path to a real file under `ASSETS_DIR`.
/// Returns `None` when the file is missing — callers handle that via
/// 404 paths (URI scheme) or `Option`-returning helpers (icons).
pub fn resolve(path: &str) -> Option<PathBuf> {
    let candidate = Path::new(ASSETS_DIR).join(path);
    candidate.is_file().then_some(candidate)
}

/// Register the `sola-assets://` URI scheme on a WebKit `WebContext`.
///
/// After registration, WebViews using this context can reference assets as
/// `<img src="sola-assets://icons/lucide/terminal.svg">`.
///
/// The scheme is also marked CORS-enabled so cross-origin loads from
/// `app://` documents (e.g. `@font-face` and `fetch`) succeed without
/// hitting the browser's same-origin policy.
pub fn register_uri_scheme(ctx: &webkit6::WebContext) {
    if let Some(sm) = ctx.security_manager() {
        sm.register_uri_scheme_as_cors_enabled("sola-assets");
    }

    ctx.register_uri_scheme("sola-assets", |request| {
        let uri = request.uri().unwrap_or_default().to_string();
        let path = uri
            .strip_prefix("sola-assets://")
            .unwrap_or(&uri)
            .split('?')
            .next()
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("");

        if path.is_empty() || path.contains("..") {
            tracing::warn!(uri, "sola-assets: invalid path");
            serve_not_found(request);
            return;
        }

        match resolve(path).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
            Some((full, bytes)) => {
                let mime = mime_for(&full);
                serve_bytes(request, bytes, mime);
            }
            None => {
                tracing::warn!(path, "sola-assets: 404");
                serve_not_found(request);
            }
        }
    });
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

fn serve_bytes(request: &webkit6::URISchemeRequest, body: Vec<u8>, content_type: &str) {
    let len = body.len() as i64;
    let gbytes = glib::Bytes::from_owned(body);
    let stream = gio::MemoryInputStream::from_bytes(&gbytes);
    request.finish(&stream, len, Some(content_type));
}

fn serve_not_found(request: &webkit6::URISchemeRequest) {
    let body = b"Not Found".to_vec();
    serve_bytes(request, body, "text/plain");
}
