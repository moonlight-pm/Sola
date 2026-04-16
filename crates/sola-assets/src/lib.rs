//! Shared on-disk assets (icons, fonts, ...) for Sola.
//!
//! Serves files from `<assets_dir>/<path>` via a `sola-assets://<path>` WebKit
//! URI scheme. Resolves to `/opt/sola/share/` when present (deployed), else
//! `<workspace>/crates/sola-assets/assets/` (dev mode).
//!
//! Nothing is compiled into consumer binaries — all data is read from disk.

use std::path::{Path, PathBuf};

pub mod icons;

/// Deployed location of shared assets on canto.
pub const DEPLOYED_DIR: &str = "/opt/sola/share";

/// Dev-mode location (relative to this crate's manifest).
const DEV_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");

/// Returns the active assets root — `DEPLOYED_DIR` if it exists, else `DEV_DIR`.
///
/// Logs a warning and returns `DEV_DIR` (even if missing) when neither exists.
pub fn assets_dir() -> PathBuf {
    let deployed = Path::new(DEPLOYED_DIR);
    if deployed.is_dir() {
        return deployed.to_path_buf();
    }
    let dev = PathBuf::from(DEV_DIR);
    if !dev.is_dir() {
        tracing::warn!(
            deployed = DEPLOYED_DIR,
            dev = DEV_DIR,
            "sola-assets: neither deployed nor dev asset dir exists"
        );
    }
    dev
}

/// Register the `sola-assets://` URI scheme on a WebKit `WebContext`.
///
/// After registration, WebViews using this context can reference assets as
/// `<img src="sola-assets://icons/lucide/terminal.svg">`.
pub fn register_uri_scheme(ctx: &webkit6::WebContext) {
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

        let full = assets_dir().join(path);
        match std::fs::read(&full) {
            Ok(bytes) => {
                let mime = mime_for(&full);
                serve_bytes(request, bytes, mime);
            }
            Err(_) => {
                tracing::warn!(path = %full.display(), "sola-assets: 404");
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
