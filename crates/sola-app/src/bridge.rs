use std::sync::mpsc;
use std::time::Duration;

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

/// Set up the glib→JS bridge: polls the event channel every 2ms and
/// forwards messages to the WebView via evaluate_javascript.
pub fn setup_event_poller(
    webview: webkit6::WebView,
    event_rx: mpsc::Receiver<String>,
) {
    glib::timeout_add_local(Duration::from_millis(2), move || {
        while let Ok(msg) = event_rx.try_recv() {
            send_to_js(&webview, &msg);
        }
        glib::ControlFlow::Continue
    });
}
