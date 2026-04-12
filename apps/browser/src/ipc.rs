use crate::AppState;
use std::rc::Rc;
use webkit6::prelude::*;

const INIT_SCRIPT: &str = r#"
(function() {
    window.sola = {
        _handlers: {},
        _nextId: 0,

        invoke(command, args = {}) {
            const callbackId = String(this._nextId++);
            return window.webkit.messageHandlers.sola
                .postMessage(JSON.stringify({ command, args, callbackId }))
                .then(raw => {
                    try { return JSON.parse(raw); } catch { return raw; }
                });
        },

        on(event, callback) {
            if (!this._handlers[event]) this._handlers[event] = new Set();
            this._handlers[event].add(callback);
            return () => this._handlers[event]?.delete(callback);
        },

        _emit(event, data) {
            const handlers = this._handlers[event];
            if (handlers) {
                for (const cb of handlers) {
                    try { cb(typeof data === 'string' ? JSON.parse(data) : data); }
                    catch (e) { console.error('sola event error:', e); }
                }
            }
        }
    };
})();
"#;

pub fn inject_init_script(manager: &webkit6::UserContentManager) {
    let script = webkit6::UserScript::new(
        INIT_SCRIPT,
        webkit6::UserContentInjectedFrames::AllFrames,
        webkit6::UserScriptInjectionTime::Start,
        &[],
        &[],
    );
    manager.add_script(&script);
}

pub fn emit_event_json(webview: &webkit6::WebView, event: &str, data: &serde_json::Value) {
    let json_str = serde_json::to_string(data).unwrap_or_default();
    // Escape for JS string literal
    let escaped = json_str.replace('\\', "\\\\").replace('\'', "\\'");
    let js = format!("window.sola?._emit('{event}', '{escaped}')");
    webview.evaluate_javascript(&js, None, None, None::<&gio::Cancellable>, |_| {});
}

pub fn setup(manager: &webkit6::UserContentManager, app_state: &Rc<AppState>) {
    inject_init_script(manager);

    manager.register_script_message_handler_with_reply("sola", None);

    let state = app_state.clone();
    manager.connect_script_message_with_reply_received(
        Some("sola"),
        move |_mgr, js_value, reply| {
            let msg_str = js_value.to_str().to_string();

            let msg: serde_json::Value = match serde_json::from_str(&msg_str) {
                Ok(v) => v,
                Err(_) => {
                    reply.return_error_message("invalid json");
                    return true;
                }
            };

            let command = msg["command"].as_str().unwrap_or("");
            let args = &msg["args"];

            let result = handle_command(&state, command, args);

            let response_str = match &result {
                Ok(val) => serde_json::to_string(val).unwrap_or_else(|_| "null".into()),
                Err(e) => {
                    tracing::warn!("ipc command '{command}' failed: {e}");
                    format!(r#"{{"error":"{e}"}}"#)
                }
            };

            let ctx = js_value.context().expect("js value must have context");
            let js_result =
                webkit6::javascriptcore::Value::new_string(&ctx, Some(&response_str));
            reply.return_value(&js_result);
            true
        },
    );
}

fn handle_command(
    state: &Rc<AppState>,
    command: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    match command {
        "ready" => cmd_ready(state),
        "create_tab" => cmd_create_tab(state, args),
        "close_tab" => cmd_close_tab(state, args),
        "switch_tab" => cmd_switch_tab(state, args),
        "navigate" => cmd_navigate(state, args),
        "go_back" => cmd_go_back(state),
        "go_forward" => cmd_go_forward(state),
        "reload" => cmd_reload(state),
        "history_search" => cmd_history_search(state, args),
        _ => Err(format!("unknown command: {command}")),
    }
}

fn cmd_ready(state: &Rc<AppState>) -> Result<serde_json::Value, String> {
    let store = state.tab_store.borrow();
    let tabs: Vec<serde_json::Value> = store
        .tabs
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let tab_id = format!("restored-{i}");
            serde_json::json!({
                "id": tab_id,
                "url": t.url,
                "title": t.title,
            })
        })
        .collect();
    let active = store.active_tab_id.clone();
    drop(store);

    // Create actual WebViews for restored tabs
    for (i, persisted) in state.tab_store.borrow().tabs.iter().enumerate() {
        let tab_id = format!("restored-{i}");
        crate::tabs::create_tab_webview(
            state,
            &tab_id,
            Some(&persisted.url),
            persisted.session_state.as_deref(),
        );
    }

    // Activate first restored tab
    let active_id = active.unwrap_or_else(|| {
        if !tabs.is_empty() {
            tabs[0]["id"].as_str().unwrap_or("").to_string()
        } else {
            String::new()
        }
    });
    if !active_id.is_empty() {
        crate::tabs::switch_tab(state, &active_id);
    }

    Ok(serde_json::json!({
        "tabs": tabs,
        "activeTabId": active_id,
    }))
}

fn cmd_create_tab(
    state: &Rc<AppState>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let url = args["url"].as_str();
    let activate = args["activate"].as_bool().unwrap_or(true);
    let tab_id = uuid::Uuid::new_v4().to_string();
    crate::tabs::create_tab_webview(state, &tab_id, url, None);
    if activate {
        crate::tabs::switch_tab(state, &tab_id);
    }

    // Persist
    let mut store = state.tab_store.borrow_mut();
    store.tabs.push(crate::state::PersistedTab {
        url: url.unwrap_or("").to_string(),
        title: String::new(),
        session_state: None,
    });
    drop(store);
    state.persist_tabs();

    Ok(serde_json::json!({ "tabId": tab_id }))
}

fn cmd_close_tab(
    state: &Rc<AppState>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let tab_id = args["tabId"].as_str().ok_or("missing tabId")?;
    crate::tabs::close_tab(state, tab_id);
    Ok(serde_json::json!("ok"))
}

fn cmd_switch_tab(
    state: &Rc<AppState>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let tab_id = args["tabId"].as_str().ok_or("missing tabId")?;
    crate::tabs::switch_tab(state, tab_id);
    Ok(serde_json::json!("ok"))
}

fn cmd_navigate(
    state: &Rc<AppState>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let url = args["url"].as_str().ok_or("missing url")?;
    crate::tabs::navigate_active(state, url);
    Ok(serde_json::json!("ok"))
}

fn cmd_go_back(state: &Rc<AppState>) -> Result<serde_json::Value, String> {
    crate::tabs::go_back(state);
    Ok(serde_json::json!("ok"))
}

fn cmd_go_forward(state: &Rc<AppState>) -> Result<serde_json::Value, String> {
    crate::tabs::go_forward(state);
    Ok(serde_json::json!("ok"))
}

fn cmd_reload(state: &Rc<AppState>) -> Result<serde_json::Value, String> {
    crate::tabs::reload(state);
    Ok(serde_json::json!("ok"))
}

fn cmd_history_search(
    state: &Rc<AppState>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let query = args["query"].as_str().ok_or("missing query")?;
    let history = state.history.borrow();
    let results: Vec<serde_json::Value> = history
        .search(query, 10)
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "url": e.url,
                "title": e.title,
                "visits": e.visits,
            })
        })
        .collect();
    Ok(serde_json::json!(results))
}
