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
