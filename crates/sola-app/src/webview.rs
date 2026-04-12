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

/// Create a UserContentManager with the `sola` message handler.
/// Returns the UCM and a command receiver.
pub fn create_content_manager(
    cmd_tx: tokio::sync::mpsc::UnboundedSender<String>,
) -> webkit6::UserContentManager {
    let ucm = webkit6::UserContentManager::new();
    ucm.register_script_message_handler("sola", None::<&str>);

    ucm.connect_script_message_received(Some("sola"), move |_ucm, js_value| {
        let msg: String = js_value.to_string().into();
        let _ = cmd_tx.send(msg);
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
