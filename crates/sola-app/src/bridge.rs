use webkit6::prelude::*;

/// Send a JSON message to the JS frontend via evaluate_javascript.
pub fn send_to_js(webview: &webkit6::WebView, msg: &str) {
    let js_str = serde_json::to_string(msg).unwrap_or_default();
    let script = format!("window.__solaRecv({js_str})");
    webview.evaluate_javascript(
        &script,
        None::<&str>,
        None::<&str>,
        None::<&gio::Cancellable>,
        |result| {
            if let Err(e) = result {
                tracing::debug!("JS eval error: {e}");
            }
        },
    );
}
