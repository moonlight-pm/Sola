use crate::AppState;
use std::rc::Rc;

/// Send an event to the JS frontend via the @sola/ipc protocol.
/// Events are delivered as `{ event: "name", ...data }` through `window.__solaRecv`.
pub fn emit_event(webview: &webkit6::WebView, event: &str, data: &serde_json::Value) {
    let mut msg = data.clone();
    if let Some(obj) = msg.as_object_mut() {
        obj.insert("event".to_string(), serde_json::json!(event));
    }
    sola_app::bridge::send_to_js(webview, &msg.to_string());
}

/// Send a command response to the JS frontend via the @sola/ipc protocol.
/// Responses are delivered as `{ id, result }` through `window.__solaRecv`.
fn send_response(webview: &webkit6::WebView, id: u64, result: &serde_json::Value) {
    let msg = serde_json::json!({ "id": id, "result": result });
    sola_app::bridge::send_to_js(webview, &msg.to_string());
}

pub fn setup(manager: &webkit6::UserContentManager, app_state: &Rc<AppState>) {
    manager.register_script_message_handler("sola", None::<&str>);

    let state = app_state.clone();
    manager.connect_script_message_received(Some("sola"), move |_mgr, js_value| {
        let msg_str: String = js_value.to_string().into();

        let msg: serde_json::Value = match serde_json::from_str(&msg_str) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("invalid IPC json: {e}");
                return;
            }
        };

        let id = msg.get("id").and_then(|v| v.as_u64());
        let cmd = msg.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
        let args = msg.get("args").cloned().unwrap_or(serde_json::json!({}));

        let result = handle_command(&state, cmd, &args);

        if let Some(id) = id {
            let response = match &result {
                Ok(val) => val.clone(),
                Err(e) => {
                    tracing::warn!("ipc command '{cmd}' failed: {e}");
                    serde_json::json!({ "error": e })
                }
            };
            send_response(&state.chrome_webview, id, &response);
        }
    });
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
    let tab_id = args["tabId"].as_str().ok_or("missing tabId")?;
    let url = args["url"].as_str();
    let activate = args["activate"].as_bool().unwrap_or(true);
    crate::tabs::create_tab_webview(state, tab_id, url, None);
    if activate {
        crate::tabs::switch_tab(state, tab_id);
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

    Ok(serde_json::json!("ok"))
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
