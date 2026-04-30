use std::cell::RefCell;
use std::rc::Weak;

use base64::Engine;
use serde_json::json;
use webkit6::prelude::*;

use sola_app::{AppRuntime, WindowHandle};
use sola_bus::topics::{BrowserTab, Topic};

use crate::app::BrowserApp;
use crate::state::HistoryOps;

const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";

const EMACS_SCRIPT: &str = r#"
(function() {
    if (window.__sola_emacs) return;
    window.__sola_emacs = true;

    document.addEventListener('keydown', function(e) {
        if (!e.ctrlKey) return;

        var el = document.activeElement;
        if (!el) return;
        if (el.closest && el.closest('.cm-editor')) return;

        var isText = el.tagName === 'TEXTAREA'
            || (el.tagName === 'INPUT' && /^(text|search|url|email|password|tel|number)$/i.test(el.type || 'text'))
            || el.isContentEditable;
        if (!isText) return;

        var handled = true;
        switch (e.key) {
            case 'f': move('forward', 'character'); break;
            case 'b': move('backward', 'character'); break;
            case 'n': move('forward', 'line'); break;
            case 'p': move('backward', 'line'); break;
            case 'a': move('backward', 'lineboundary'); break;
            case 'e': move('forward', 'lineboundary'); break;
            case 'd': document.execCommand('forwardDelete'); break;
            case 'h': document.execCommand('delete'); break;
            case 'k': {
                var sel = window.getSelection();
                sel.modify('extend', 'forward', 'lineboundary');
                if (!sel.isCollapsed) document.execCommand('delete');
                break;
            }
            default: handled = false;
        }
        if (handled) { e.preventDefault(); e.stopPropagation(); }

        function move(dir, gran) {
            var sel = window.getSelection();
            if (sel) sel.modify('move', dir, gran);
        }
    }, true);
})();
"#;

pub struct Tab {
    pub id: String,
    pub webview: webkit6::WebView,
}

pub struct TabConfig {
    pub url: Option<String>,
    pub session_state_b64: Option<String>,
}

/// Build a per-tab WebView: Safari user agent, emacs keybinding UserScript,
/// session-state restore, URL load. Signal handlers are wired separately by
/// `wire_signals` after the caller has positioned the WebView in the
/// chrome's content area.
pub fn build_web_page_view(
    web_context: &webkit6::WebContext,
    network_session: &webkit6::NetworkSession,
    cfg: &TabConfig,
) -> webkit6::WebView {
    let manager = webkit6::UserContentManager::new();

    let emacs = webkit6::UserScript::new(
        EMACS_SCRIPT,
        webkit6::UserContentInjectedFrames::AllFrames,
        webkit6::UserScriptInjectionTime::End,
        &[],
        &[],
    );
    manager.add_script(&emacs);

    let webview = webkit6::WebView::builder()
        .web_context(web_context)
        .network_session(network_session)
        .user_content_manager(&manager)
        .build();

    // White background — matches mainstream browser defaults. Pages
    // without an explicit `body` background inherit it (rather than our
    // chrome's dark color), so default-black text stays readable.
    webview.set_background_color(&gdk4::RGBA::new(1.0, 1.0, 1.0, 1.0));

    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&webview) {
        settings.set_enable_developer_extras(true);
        // Match mainstream browser defaults: video/audio with sound only
        // plays after a user gesture. Pages with multiple autoplay video
        // embeds (e.g. knowyourmeme) otherwise spin up several
        // GStreamer pipelines simultaneously, which wedges JSC on
        // WebKitGTK far more readily than on Blink.
        settings.set_media_playback_requires_user_gesture(true);
        settings.set_user_agent(Some(USER_AGENT));
    }

    if let Some(ref b64) = cfg.session_state_b64 {
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
            let gbytes = glib::Bytes::from(&bytes);
            let session = webkit6::WebViewSessionState::new(&gbytes);
            webview.restore_session_state(&session);
        }
    }

    // Mirror sola-app's chrome-WebView DPR compensation: WebKit honors
    // the compositor-assigned surface scale, so on a HiDPI surface 1 CSS
    // px renders to 2 device px and content looks 2× zoomed.
    //
    // dpr is linear in zoom_level for a fixed surface scale, so the
    // zoom that makes css-px == device-px is `current_zoom / dpr`. The
    // earlier `1.0 / dpr` formula was correct only when starting from
    // zoom_level == 1.0, and feedback-looped on reload (zoom 0.5 ⇒ dpr
    // 1.0 ⇒ "fix" sets zoom 1.0 ⇒ next reload reads dpr 2.0 ⇒ flip).
    webview.connect_load_changed(|wv, event| {
        if event != webkit6::LoadEvent::Finished {
            return;
        }
        let wv_for_zoom = wv.clone();
        wv.evaluate_javascript(
            "window.devicePixelRatio",
            None,
            None,
            None::<&gio::Cancellable>,
            move |result| {
                let Ok(jsv) = result else { return };
                let dpr = jsv.to_double();
                if dpr <= 0.001 {
                    return;
                }
                let current = wv_for_zoom.zoom_level();
                let target_zoom = current / dpr;
                if (current - target_zoom).abs() < 0.005 {
                    return;
                }
                wv_for_zoom.set_zoom_level(target_zoom);
            },
        );
    });

    let load_url = cfg.url.as_deref().unwrap_or("about:blank");
    webview.load_uri(load_url);

    webview
}

/// Attach the four per-tab signal handlers. Called by `BrowserApp::create_tab`
/// after the WebView is added to the Overlay.
pub fn wire_signals(
    webview: &webkit6::WebView,
    tab_id: &str,
    chrome: WindowHandle,
    runtime: Weak<RefCell<AppRuntime<BrowserApp>>>,
) {
    // notify::title
    {
        let chrome = chrome.clone();
        let tid = tab_id.to_string();
        webview.connect_notify_local(Some("title"), move |wv, _| {
            if let Some(title) = wv.title() {
                chrome.send_to_js(&json!({
                    "event": "tab_title_changed",
                    "tabId": tid,
                    "title": title.to_string(),
                }));
            }
        });
    }

    // notify::uri: history + session snapshot + write-through.
    //
    // WebKit fires this signal synchronously from inside `load_uri`, which
    // we call from `on_js_command` handlers while the runtime RefCell is
    // already borrowed. Defer the runtime-touching work to the GTK idle
    // loop so it runs after the triggering borrow releases.
    {
        let chrome = chrome.clone();
        let tid = tab_id.to_string();
        let runtime = runtime.clone();
        webview.connect_notify_local(Some("uri"), move |wv, _| {
            let Some(uri) = wv.uri() else { return };
            let url_str = uri.to_string();
            let title = wv.title().map(|t| t.to_string()).unwrap_or_default();

            chrome.send_to_js(&json!({
                "event": "tab_url_changed",
                "tabId": tid,
                "url": url_str,
            }));

            let runtime = runtime.clone();
            let tid = tid.clone();
            glib::idle_add_local_once(move || {
                let Some(runtime) = runtime.upgrade() else {
                    return;
                };
                let mut rt = runtime.borrow_mut();
                let AppRuntime { app, ctx } = &mut *rt;
                if !is_blank_url(&url_str) {
                    app.history.record_visit(&url_str, &title);
                    ctx.emit(Topic::BrowserHistory(app.history.clone()));
                }
                app.capture_tab_session_state(&tid, ctx);
            });
        });
    }

    // notify::is-loading
    {
        let chrome = chrome.clone();
        let tid = tab_id.to_string();
        webview.connect_notify_local(Some("is-loading"), move |wv, _| {
            chrome.send_to_js(&json!({
                "event": "tab_load_changed",
                "tabId": tid,
                "loading": wv.is_loading(),
            }));
        });
    }

    // load-failed: surface WebKit load errors. Default return false lets
    // WebKit show its own error page; we just log here so the failure is
    // diagnosable from /opt/sola/log/sola.log.
    {
        let tid = tab_id.to_string();
        webview.connect_load_failed(move |_wv, _event, failing_uri, error| {
            tracing::warn!(
                tab_id = %tid,
                uri = %failing_uri,
                error = %error.message(),
                "load-failed"
            );
            false
        });
    }

    // load-failed-with-tls-errors: surface TLS-specific failures.
    {
        let tid = tab_id.to_string();
        webview.connect_load_failed_with_tls_errors(move |_wv, failing_uri, _cert, errors| {
            tracing::warn!(
                tab_id = %tid,
                uri = %failing_uri,
                tls_errors = ?errors,
                "load-failed-with-tls-errors"
            );
            false
        });
    }

    // web-process-terminated: WebKit2 runs each tab's content in its own
    // WebContent process. When that process dies, the WebView keeps
    // existing as a placeholder (URI becomes about:blank, JS context is
    // gone) but no load-failed fires — so without this signal the crash
    // is silent. Auto-reload the last known URL to recover; the user
    // sees a brief blank then the page returns. (Reproduced today on
    // noagendashow.net link clicks — the audio prefetch on the new page
    // tickles a media-stack bug that takes the WebContent down.)
    {
        let tid = tab_id.to_string();
        webview.connect_web_process_terminated(move |wv, reason| {
            let uri = wv.uri().map(|u| u.to_string()).unwrap_or_default();
            tracing::warn!(
                tab_id = %tid,
                uri = %uri,
                reason = ?reason,
                "web-process-terminated; reloading"
            );
            if !uri.is_empty() && uri != "about:blank" {
                wv.load_uri(&uri);
            }
        });
    }

    // notify::is-web-process-responsive: WebKit's hang monitor pings the
    // WebContent process; when it stops responding (busy JS, blocked
    // syscall, etc.) this flips false. The process is still alive — no
    // termination signal — so without surfacing this the user has no way
    // to tell a "frozen" tab from one that simply happens to not be
    // doing anything. Send to the strip so it can show an indicator and
    // expose a force-reload affordance.
    {
        let chrome = chrome.clone();
        let tid = tab_id.to_string();
        webview.connect_notify_local(Some("is-web-process-responsive"), move |wv, _| {
            let responsive = wv.is_web_process_responsive();
            tracing::warn!(
                tab_id = %tid,
                responsive,
                "web-process responsiveness changed"
            );
            chrome.send_to_js(&json!({
                "event": "tab_responsive_changed",
                "tabId": tid,
                "responsive": responsive,
            }));
        });
    }

    // decide-policy: target="_blank" → new tab.
    {
        let runtime = runtime.clone();
        webview.connect_decide_policy(move |_wv, decision, decision_type| {
            if decision_type != webkit6::PolicyDecisionType::NewWindowAction {
                return false;
            }
            let Some(nav) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>() else {
                return false;
            };
            let Some(mut action) = nav.navigation_action() else {
                return false;
            };
            let Some(request) = action.request() else {
                return false;
            };
            let Some(uri) = request.uri() else {
                return false;
            };
            let url = uri.to_string();
            let new_tab_id = uuid::Uuid::new_v4().to_string();

            // decide-policy may fire from inside a WebKit call made while
            // the runtime is already borrowed. Defer tab creation to the
            // GTK idle loop to avoid reentrant borrow_mut.
            let runtime = runtime.clone();
            glib::idle_add_local_once(move || {
                let Some(runtime) = runtime.upgrade() else {
                    return;
                };
                let mut rt = runtime.borrow_mut();
                let AppRuntime { app, ctx } = &mut *rt;
                let ordinal = app
                    .tabs_by_id
                    .values()
                    .map(|t| t.ordinal)
                    .max()
                    .map(|m| m + 1)
                    .unwrap_or(0);
                let tab = BrowserTab {
                    id: new_tab_id.clone(),
                    url: url.clone(),
                    title: String::new(),
                    ordinal,
                    session_state: None,
                };
                app.tabs_by_id.insert(new_tab_id.clone(), tab.clone());
                app.create_webview_for_tab(&tab);
                ctx.emit(Topic::BrowserTab(tab));
                app.set_active_tab(Some(new_tab_id.clone()), ctx);
                app.emit_to_chrome(
                    "bus_new_tab",
                    json!({
                        "tabId": new_tab_id,
                        "url": url,
                        "activate": true,
                    }),
                );
            });
            decision.ignore();
            true
        });
    }
}

/// True if `url` is a blank/internal placeholder that shouldn't appear
/// in the user's visit history.
fn is_blank_url(url: &str) -> bool {
    matches!(url, "" | "about:blank" | "about:srcdoc")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_urls_are_ignored() {
        assert!(is_blank_url(""));
        assert!(is_blank_url("about:blank"));
        assert!(is_blank_url("about:srcdoc"));
        assert!(!is_blank_url("https://example.com"));
        assert!(!is_blank_url("about:config"));
    }
}
