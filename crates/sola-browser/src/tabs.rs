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

    // Dark background so about:blank isn't a white flash.
    webview.set_background_color(&gdk4::RGBA::new(0.039, 0.043, 0.051, 1.0));

    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&webview) {
        settings.set_enable_developer_extras(true);
        settings.set_media_playback_requires_user_gesture(false);
        settings.set_user_agent(Some(USER_AGENT));
    }

    if let Some(ref b64) = cfg.session_state_b64 {
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
            let gbytes = glib::Bytes::from(&bytes);
            let session = webkit6::WebViewSessionState::new(&gbytes);
            webview.restore_session_state(&session);
        }
    }

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
                app.history.record_visit(&url_str, &title);
                ctx.emit(Topic::BrowserHistory(app.history.clone()));
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
