use crate::ipc;
use crate::AppState;
use base64::Engine;
use std::rc::Rc;
use webkit6::prelude::*;

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

pub fn create_tab_webview(
    state: &Rc<AppState>,
    tab_id: &str,
    url: Option<&str>,
    session_state_b64: Option<&str>,
) {
    let manager = webkit6::UserContentManager::new();

    // Inject emacs keybindings
    let emacs = webkit6::UserScript::new(
        EMACS_SCRIPT,
        webkit6::UserContentInjectedFrames::AllFrames,
        webkit6::UserScriptInjectionTime::End,
        &[],
        &[],
    );
    manager.add_script(&emacs);

    let webview = webkit6::WebView::builder()
        .web_context(&state.web_context)
        .network_session(&state.network_session)
        .user_content_manager(&manager)
        .build();

    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&webview) {
        settings.set_enable_developer_extras(true);
        settings.set_media_playback_requires_user_gesture(false);
        settings.set_user_agent(Some(USER_AGENT));
    }

    // Restore session state (back/forward history)
    if let Some(b64) = session_state_b64 {
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
            let gbytes = glib::Bytes::from(&bytes);
            let session = webkit6::WebViewSessionState::new(&gbytes);
            webview.restore_session_state(&session);
        }
    }

    // Load URL
    let load_url = url.unwrap_or("about:blank");
    webview.load_uri(load_url);

    // Position in content area
    let area = crate::chrome::content_area(
        state.chrome_webview.width(),
        state.chrome_webview.height(),
    );
    state
        .container
        .put(&webview, area.x as f64, area.y as f64);
    webview.set_size_request(area.width, area.height);
    webview.set_visible(false); // Hidden until switched to

    // Track title changes
    let chrome_wv = state.chrome_webview.clone();
    let tid = tab_id.to_string();
    webview.connect_notify_local(Some("title"), move |wv, _| {
        if let Some(title) = wv.title() {
            let data = serde_json::json!({ "tabId": tid, "title": title.to_string() });
            ipc::emit_event_json(&chrome_wv, "tab_title_changed", &data);
        }
    });

    // Track URL changes
    let chrome_wv = state.chrome_webview.clone();
    let tid = tab_id.to_string();
    let state_ref = state.clone();
    webview.connect_notify_local(Some("uri"), move |wv, _| {
        if let Some(uri) = wv.uri() {
            let url_str = uri.to_string();
            let data = serde_json::json!({ "tabId": tid, "url": url_str });
            ipc::emit_event_json(&chrome_wv, "tab_url_changed", &data);

            // Record in history
            let title = wv.title().map(|t| t.to_string()).unwrap_or_default();
            state_ref
                .history
                .borrow_mut()
                .record_visit(&url_str, &title);
            state_ref.persist_history();
        }
    });

    // Track load state
    let chrome_wv = state.chrome_webview.clone();
    let tid = tab_id.to_string();
    webview.connect_notify_local(Some("is-loading"), move |wv, _| {
        let loading = wv.is_loading();
        let data = serde_json::json!({ "tabId": tid, "loading": loading });
        ipc::emit_event_json(&chrome_wv, "tab_load_changed", &data);
    });

    // Handle target="_blank" -- open as new tab
    let state_ref = state.clone();
    webview.connect_decide_policy(move |_wv, decision, decision_type| {
        if decision_type == webkit6::PolicyDecisionType::NewWindowAction {
            if let Some(nav) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>() {
                if let Some(mut action) = nav.navigation_action() {
                    if let Some(request) = action.request() {
                        if let Some(uri) = request.uri() {
                            let url = uri.to_string();
                            let tab_id = uuid::Uuid::new_v4().to_string();
                            create_tab_webview(&state_ref, &tab_id, Some(&url), None);
                            switch_tab(&state_ref, &tab_id);
                            let data = serde_json::json!({
                                "tabId": tab_id,
                                "url": url,
                                "activate": true,
                            });
                            ipc::emit_event_json(
                                &state_ref.chrome_webview,
                                "bus_new_tab",
                                &data,
                            );
                        }
                    }
                }
            }
            decision.ignore();
            return true;
        }
        false
    });

    state.tabs.borrow_mut().push(crate::Tab {
        id: tab_id.to_string(),
        webview,
    });
}

pub fn switch_tab(state: &Rc<AppState>, tab_id: &str) {
    let tabs = state.tabs.borrow();

    // Hide current
    if let Some(current_id) = state.active_tab_id.borrow().as_ref() {
        if let Some(tab) = tabs.iter().find(|t| t.id == *current_id) {
            tab.webview.set_visible(false);
        }
    }

    // Show new
    if let Some(tab) = tabs.iter().find(|t| t.id == tab_id) {
        tab.webview.set_visible(true);
        tab.webview.grab_focus();
    }

    drop(tabs);
    *state.active_tab_id.borrow_mut() = Some(tab_id.to_string());

    // Persist
    state.tab_store.borrow_mut().active_tab_id = Some(tab_id.to_string());
    state.persist_tabs();
}

pub fn close_tab(state: &Rc<AppState>, tab_id: &str) {
    let mut tabs = state.tabs.borrow_mut();
    if let Some(pos) = tabs.iter().position(|t| t.id == tab_id) {
        let tab = tabs.remove(pos);
        drop(tabs);
        tab.webview.unparent();

        // Update persisted store
        let mut store = state.tab_store.borrow_mut();
        if pos < store.tabs.len() {
            store.tabs.remove(pos);
        }
        drop(store);
        state.persist_tabs();
    }
}

pub fn navigate_active(state: &Rc<AppState>, url: &str) {
    let active_id = state.active_tab_id.borrow().clone();
    if let Some(id) = active_id {
        let tabs = state.tabs.borrow();
        if let Some(tab) = tabs.iter().find(|t| t.id == id) {
            tab.webview.load_uri(url);
        }
    }
}

pub fn go_back(state: &Rc<AppState>) {
    let active_id = state.active_tab_id.borrow().clone();
    if let Some(id) = active_id {
        let tabs = state.tabs.borrow();
        if let Some(tab) = tabs.iter().find(|t| t.id == id) {
            tab.webview.go_back();
        }
    }
}

pub fn go_forward(state: &Rc<AppState>) {
    let active_id = state.active_tab_id.borrow().clone();
    if let Some(id) = active_id {
        let tabs = state.tabs.borrow();
        if let Some(tab) = tabs.iter().find(|t| t.id == id) {
            tab.webview.go_forward();
        }
    }
}

pub fn reload(state: &Rc<AppState>) {
    let active_id = state.active_tab_id.borrow().clone();
    if let Some(id) = active_id {
        let tabs = state.tabs.borrow();
        if let Some(tab) = tabs.iter().find(|t| t.id == id) {
            tab.webview.reload();
        }
    }
}

pub fn capture_session_state(state: &Rc<AppState>) {
    let tabs = state.tabs.borrow();
    let mut store = state.tab_store.borrow_mut();

    for (i, tab) in tabs.iter().enumerate() {
        if i < store.tabs.len() {
            if let Some(uri) = tab.webview.uri() {
                store.tabs[i].url = uri.to_string();
            }
            if let Some(title) = tab.webview.title() {
                store.tabs[i].title = title.to_string();
            }
            if let Some(session) = tab.webview.session_state() {
                if let Some(bytes) = session.serialize() {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes.as_ref());
                    store.tabs[i].session_state = Some(b64);
                }
            }
        }
    }

    drop(store);
    drop(tabs);
    state.persist_tabs();
}
