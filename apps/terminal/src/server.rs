use std::sync::Arc;

use base64::Engine;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tracing::warn;

use crate::pty::PtyEvent;
use crate::state::{TabEntry, TerminalState};

/// Process commands from the frontend via WebKit message handler.
/// Runs on the tokio runtime. Responses and events are sent back
/// through `event_tx`, which bridges to the glib main loop.
pub async fn command_loop(
    state: Arc<TerminalState>,
    mut cmd_rx: mpsc::UnboundedReceiver<String>,
    event_tx: std::sync::mpsc::Sender<String>,
) {
    while let Some(msg) = cmd_rx.recv().await {
        let parsed: Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(e) => {
                warn!("Invalid command JSON: {e}");
                continue;
            }
        };

        let id = parsed.get("id").and_then(|v| v.as_u64());
        let cmd = parsed.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
        let args = parsed.get("args").cloned().unwrap_or(json!({}));

        let result = dispatch(&state, cmd, &args, &event_tx).await;

        if let Some(id) = id {
            let response = json!({ "id": id, "result": result });
            let _ = event_tx.send(response.to_string());
        }
    }
}

async fn dispatch(
    state: &Arc<TerminalState>,
    cmd: &str,
    args: &Value,
    event_tx: &std::sync::mpsc::Sender<String>,
) -> Value {
    match cmd {
        "spawn_pty" => cmd_spawn_pty(state, args, event_tx).await,
        "write_pty" => cmd_write_pty(state, args).await,
        "resize_pty" => cmd_resize_pty(state, args).await,
        "close_pty" => cmd_close_pty(state, args).await,
        "reconnect_pty" => cmd_reconnect_pty(state, args).await,
        "rename_tab" => cmd_rename_tab(state, args).await,
        "update_cwd" => cmd_update_cwd(state, args).await,
        "reorder_tabs" => cmd_reorder_tabs(state, args).await,
        _ => json!({ "error": format!("unknown command: {cmd}") }),
    }
}

async fn cmd_spawn_pty(
    state: &Arc<TerminalState>,
    args: &Value,
    event_tx: &std::sync::mpsc::Sender<String>,
) -> Value {
    let cols = args.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as u16;
    let rows = args.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as u16;
    let tmux_session = args
        .get("tmuxSession")
        .and_then(|v| v.as_str())
        .map(String::from);
    let cwd = args
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(String::from);

    let pty_id = uuid::Uuid::new_v4().to_string();
    let (pty_event_tx, pty_event_rx) = mpsc::unbounded_channel::<PtyEvent>();

    let tmux_session_name = {
        let mut mgr = state.pty_manager.lock().await;
        match mgr.spawn_pty(
            pty_id.clone(),
            cols,
            rows,
            tmux_session,
            cwd.clone(),
            pty_event_tx,
        ) {
            Ok(name) => name,
            Err(e) => return json!({ "error": e }),
        }
    };

    // Look up custom title if reattaching (keyed by tmux session name)
    let title = state
        .custom_titles
        .read()
        .await
        .get(&tmux_session_name)
        .cloned();

    // Add tab entry
    {
        let mut tabs = state.tabs.write().await;
        tabs.push(TabEntry {
            pty_id: pty_id.clone(),
            tmux_session: tmux_session_name.clone(),
            custom_title: title.clone(),
            cwd,
        });
    }

    // Forward PTY events to the glib main loop
    let tx = event_tx.clone();
    tokio::spawn(forward_pty_events(pty_id.clone(), pty_event_rx, tx));

    state.persist_to_disk().await;

    json!({
        "pty_id": pty_id,
        "tmux_session": tmux_session_name,
        "title": title,
    })
}

async fn cmd_write_pty(state: &Arc<TerminalState>, args: &Value) -> Value {
    let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({ "error": "missing pty_id" }),
    };
    let data_b64 = match args.get("data").and_then(|v| v.as_str()) {
        Some(d) => d,
        None => return json!({ "error": "missing data" }),
    };
    let data = match base64::engine::general_purpose::STANDARD.decode(data_b64) {
        Ok(d) => d,
        Err(e) => return json!({ "error": format!("base64 decode failed: {e}") }),
    };

    let mgr = state.pty_manager.lock().await;
    match mgr.write_pty(pty_id, &data) {
        Ok(()) => json!("ok"),
        Err(e) => json!({ "error": e }),
    }
}

async fn cmd_resize_pty(state: &Arc<TerminalState>, args: &Value) -> Value {
    let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({ "error": "missing pty_id" }),
    };
    let cols = match args.get("cols").and_then(|v| v.as_u64()) {
        Some(c) => c as u16,
        None => return json!({ "error": "missing cols" }),
    };
    let rows = match args.get("rows").and_then(|v| v.as_u64()) {
        Some(r) => r as u16,
        None => return json!({ "error": "missing rows" }),
    };

    let mgr = state.pty_manager.lock().await;
    match mgr.resize_pty(pty_id, cols, rows) {
        Ok(()) => {}
        Err(e) => return json!({ "error": e }),
    }
    match mgr.sigwinch_pty(pty_id) {
        Ok(()) => json!("ok"),
        Err(e) => json!({ "error": e }),
    }
}

async fn cmd_close_pty(state: &Arc<TerminalState>, args: &Value) -> Value {
    let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({ "error": "missing pty_id" }),
    };

    {
        let mut mgr = state.pty_manager.lock().await;
        if let Err(e) = mgr.close_pty(pty_id) {
            return json!({ "error": e });
        }
    }

    {
        let mut tabs = state.tabs.write().await;
        tabs.retain(|t| t.pty_id != pty_id);
    }

    state.persist_to_disk().await;

    json!("ok")
}

async fn cmd_reconnect_pty(state: &Arc<TerminalState>, args: &Value) -> Value {
    let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({ "error": "missing pty_id" }),
    };

    let mgr = state.pty_manager.lock().await;
    match mgr.reconnect_pty(pty_id) {
        Ok(scrollback) => json!({ "scrollback": scrollback }),
        Err(e) => json!({ "error": e }),
    }
}

async fn cmd_rename_tab(state: &Arc<TerminalState>, args: &Value) -> Value {
    let tmux_session = match args.get("tmux_session").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return json!({ "error": "missing tmux_session" }),
    };
    let title = match args.get("title").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return json!({ "error": "missing title" }),
    };

    // Verify the session exists
    {
        let tabs = state.tabs.read().await;
        if !tabs.iter().any(|t| t.tmux_session == tmux_session) {
            return json!({ "error": format!("no tab for session: {tmux_session}") });
        }
    }

    state
        .custom_titles
        .write()
        .await
        .insert(tmux_session.to_string(), title);

    state.persist_to_disk().await;

    json!("ok")
}

async fn cmd_update_cwd(state: &Arc<TerminalState>, args: &Value) -> Value {
    let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({ "error": "missing pty_id" }),
    };
    let cwd = match args.get("cwd").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return json!({ "error": "missing cwd" }),
    };

    {
        let mut tabs = state.tabs.write().await;
        if let Some(tab) = tabs.iter_mut().find(|t| t.pty_id == pty_id) {
            tab.cwd = Some(cwd);
        }
    }

    state.persist_to_disk().await;

    json!("ok")
}

async fn cmd_reorder_tabs(state: &Arc<TerminalState>, args: &Value) -> Value {
    let pty_ids: Vec<String> = match args.get("pty_ids").and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        None => return json!({ "error": "missing pty_ids" }),
    };

    {
        let mut tabs = state.tabs.write().await;
        let mut reordered = Vec::with_capacity(pty_ids.len());
        for id in &pty_ids {
            if let Some(tab) = tabs.iter().find(|t| &t.pty_id == id).cloned() {
                reordered.push(tab);
            }
        }
        *tabs = reordered;
    }

    state.persist_to_disk().await;

    json!("ok")
}

async fn forward_pty_events(
    _pty_id: String,
    mut event_rx: mpsc::UnboundedReceiver<PtyEvent>,
    tx: std::sync::mpsc::Sender<String>,
) {
    let b64 = base64::engine::general_purpose::STANDARD;

    while let Some(event) = event_rx.recv().await {
        let msg = match event {
            PtyEvent::Data { pty_id, data } => {
                json!({
                    "event": "pty:data",
                    "pty_id": pty_id,
                    "data": b64.encode(&data),
                })
            }
            PtyEvent::Scrollback { pty_id, data } => {
                json!({
                    "event": "pty:scrollback",
                    "pty_id": pty_id,
                    "data": b64.encode(&data),
                })
            }
            PtyEvent::Exit { pty_id } => {
                let msg = json!({
                    "event": "pty:exit",
                    "pty_id": pty_id,
                });
                let _ = tx.send(msg.to_string());
                break;
            }
        };
        if tx.send(msg.to_string()).is_err() {
            break;
        }
    }
}
