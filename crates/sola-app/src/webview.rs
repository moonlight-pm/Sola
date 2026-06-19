use crate::assets::{AssetBundle, ContentType};
use crate::strip::strip_ts;

/// Set up a WebContext with the `app:///` URI scheme.
/// Serves assets from both the app bundle and platform bundle.
/// TypeScript files are stripped on-demand.
pub fn create_web_context(
    app_assets: &'static AssetBundle,
    platform_assets: &'static AssetBundle,
    html_content: String,
) -> webkit6::WebContext {
    let ctx = webkit6::WebContext::new();

    register_assets_uri_scheme(&ctx);

    ctx.register_uri_scheme("app", move |request| {
        let uri = request.uri().unwrap_or_default().to_string();
        let path = uri
            .strip_prefix("app://")
            .unwrap_or(&uri)
            .split('?')
            .next()
            .unwrap_or("/")
            .split('#')
            .next()
            .unwrap_or("/");
        let path = if path.is_empty() { "/" } else { path };

        // Serve index.html
        if path == "/" || path == "/index.html" {
            serve_string(&request, &html_content, "text/html; charset=utf-8");
            return;
        }

        // Check app assets first, then platform assets
        let asset = app_assets.find(path).or_else(|| platform_assets.find(path));

        match asset {
            Some(asset) => {
                let body = match asset.content_type {
                    ContentType::TypeScript => strip_ts(asset.content),
                    _ => asset.content.to_string(),
                };
                serve_string(&request, &body, asset.content_type.mime());
            }
            None => {
                tracing::warn!("404: {path}");
                serve_string(&request, "Not Found", "text/plain");
            }
        }
    });

    ctx
}

/// Create a UserContentManager for a specific window. UCM messages route
/// through the shared dispatcher slot, which is filled by `run::<A>()` after
/// `A::new` returns.
pub(crate) fn create_ucm_for_window(
    dispatcher_slot: std::rc::Rc<std::cell::RefCell<Option<crate::window::JsDispatcher>>>,
) -> webkit6::UserContentManager {
    let ucm = webkit6::UserContentManager::new();
    ucm.register_script_message_handler("sola", None::<&str>);

    ucm.connect_script_message_received(Some("sola"), move |_ucm, js_value| {
        let msg: String = js_value.to_string().into();
        let parsed: serde_json::Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("invalid JS command JSON: {e}");
                return;
            }
        };
        let id = parsed.get("id").and_then(|v| v.as_u64());
        let cmd = parsed.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
        let args = parsed.get("args").cloned().unwrap_or(serde_json::json!({}));
        if let Some(dispatch) = dispatcher_slot.borrow_mut().as_mut() {
            dispatch(cmd, &args, id);
        } else {
            tracing::warn!(cmd, "JS command received before dispatcher installed");
        }
    });

    ucm
}

fn serve_string(request: &webkit6::URISchemeRequest, body: &str, content_type: &str) {
    let bytes = body.as_bytes();
    let len = bytes.len() as i64;
    let gbytes = glib::Bytes::from(bytes);
    let stream = gio::MemoryInputStream::from_bytes(&gbytes);
    request.finish(&stream, len, Some(content_type));
}

/// Register the `sola-assets://` URI scheme on a WebKit `WebContext`.
///
/// After registration, WebViews using this context can reference assets as
/// `<img src="sola-assets://icons/lucide/terminal.svg">`.
///
/// The scheme is also marked CORS-enabled so cross-origin loads from
/// `app://` documents (e.g. `@font-face` and `fetch`) succeed without
/// hitting the browser's same-origin policy.
///
/// Lives here (not in `sola-assets`) so the asset crate stays pure
/// filesystem — pulling `webkit6`/`gtk4` into `sola-assets` would leak
/// `libwebkitgtk-6.0` into every iced consumer (and shadow the WPE
/// browser's `webkit_web_view_new`).
fn register_assets_uri_scheme(ctx: &webkit6::WebContext) {
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
            serve_bytes(request, b"Not Found".to_vec(), "text/plain");
            return;
        }

        match sola_assets::resolve(path).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
            Some((full, bytes)) => {
                let mime = mime_for(&full);
                serve_bytes(request, bytes, mime);
            }
            None => {
                tracing::warn!(path, "sola-assets: 404");
                serve_bytes(request, b"Not Found".to_vec(), "text/plain");
            }
        }
    });
}

fn mime_for(path: &std::path::Path) -> &'static str {
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
