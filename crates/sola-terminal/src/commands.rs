use std::sync::Arc;

use base64::Engine;
use serde_json::{Value, json};
use sola_bus::topics::{TerminalSessions, TerminalTab, Topic};
use tokio::sync::mpsc;

use crate::pty::PtyEvent;
use crate::state::{TabEntry, TerminalState};

/// Terminal command handler implementing the sola-app AppHandler trait.
/// Emits bus topics by sending them through `emit_tx`; the main thread
/// drains and calls `ctx.emit`.
pub struct TerminalHandler {
    pub state: Arc<TerminalState>,
    pub event_tx: std::sync::mpsc::Sender<String>,
    pub emit_tx: std::sync::mpsc::Sender<Topic>,
}

#[async_trait::async_trait]
impl sola_app::AppHandler for TerminalHandler {
    async fn dispatch(&self, cmd: &str, args: &Value) -> Value {
        match cmd {
            "spawn_pty" => self.cmd_spawn_pty(args).await,
            "write_pty" => self.cmd_write_pty(args).await,
            "resize_pty" => self.cmd_resize_pty(args).await,
            "close_pty" => self.cmd_close_pty(args).await,
            "reconnect_pty" => self.cmd_reconnect_pty(args).await,
            "update_cwd" => self.cmd_update_cwd(args).await,
            "reorder_tabs" => self.cmd_reorder_tabs(args).await,
            _ => json!({ "error": format!("unknown command: {cmd}") }),
        }
    }
}

impl TerminalHandler {
    async fn cmd_spawn_pty(&self, args: &Value) -> Value {
        let cols = args.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as u16;
        let rows = args.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as u16;
        let tmux_session = args
            .get("tmuxSession")
            .and_then(|v| v.as_str())
            .map(String::from);
        let cwd = args.get("cwd").and_then(|v| v.as_str()).map(String::from);

        let pty_id = uuid::Uuid::new_v4().to_string();
        let (pty_event_tx, pty_event_rx) = mpsc::unbounded_channel::<PtyEvent>();

        let tmux_session_name = {
            let mut mgr = self.state.pty_manager.lock().await;
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

        {
            let mut tabs = self.state.tabs.write().await;
            tabs.push(TabEntry {
                pty_id: pty_id.clone(),
                tmux_session: tmux_session_name.clone(),
                cwd,
            });
        }

        let tx = self.event_tx.clone();
        tokio::spawn(forward_pty_events(pty_id.clone(), pty_event_rx, tx));

        self.emit_sessions().await;

        json!({
            "pty_id": pty_id,
            "tmux_session": tmux_session_name,
        })
    }

    async fn cmd_write_pty(&self, args: &Value) -> Value {
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

        let mgr = self.state.pty_manager.lock().await;
        match mgr.write_pty(pty_id, &data) {
            Ok(()) => json!("ok"),
            Err(e) => json!({ "error": e }),
        }
    }

    async fn cmd_resize_pty(&self, args: &Value) -> Value {
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

        let mgr = self.state.pty_manager.lock().await;
        match mgr.resize_pty(pty_id, cols, rows) {
            Ok(()) => {}
            Err(e) => return json!({ "error": e }),
        }
        match mgr.sigwinch_pty(pty_id) {
            Ok(()) => json!("ok"),
            Err(e) => json!({ "error": e }),
        }
    }

    async fn cmd_close_pty(&self, args: &Value) -> Value {
        let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({ "error": "missing pty_id" }),
        };

        {
            let mut mgr = self.state.pty_manager.lock().await;
            if let Err(e) = mgr.close_pty(pty_id) {
                return json!({ "error": e });
            }
        }

        {
            let mut tabs = self.state.tabs.write().await;
            tabs.retain(|t| t.pty_id != pty_id);
        }

        self.emit_sessions().await;

        json!("ok")
    }

    async fn cmd_reconnect_pty(&self, args: &Value) -> Value {
        let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({ "error": "missing pty_id" }),
        };

        let mgr = self.state.pty_manager.lock().await;
        match mgr.reconnect_pty(pty_id) {
            Ok(scrollback) => json!({ "scrollback": scrollback }),
            Err(e) => json!({ "error": e }),
        }
    }

    async fn cmd_update_cwd(&self, args: &Value) -> Value {
        let pty_id = match args.get("pty_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({ "error": "missing pty_id" }),
        };
        let cwd = match args.get("cwd").and_then(|v| v.as_str()) {
            Some(c) => c.to_string(),
            None => return json!({ "error": "missing cwd" }),
        };

        let changed = {
            let mut tabs = self.state.tabs.write().await;
            if let Some(tab) = tabs.iter_mut().find(|t| t.pty_id == pty_id) {
                if tab.cwd.as_deref() == Some(cwd.as_str()) {
                    false
                } else {
                    tab.cwd = Some(cwd);
                    true
                }
            } else {
                false
            }
        };

        if changed {
            self.emit_sessions().await;
        }

        json!("ok")
    }

    async fn cmd_reorder_tabs(&self, args: &Value) -> Value {
        let pty_ids: Vec<String> = match args.get("pty_ids").and_then(|v| v.as_array()) {
            Some(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            None => return json!({ "error": "missing pty_ids" }),
        };

        {
            let mut tabs = self.state.tabs.write().await;
            let mut reordered = Vec::with_capacity(pty_ids.len());
            for id in &pty_ids {
                if let Some(tab) = tabs.iter().find(|t| &t.pty_id == id).cloned() {
                    reordered.push(tab);
                }
            }
            *tabs = reordered;
        }

        self.emit_sessions().await;

        json!("ok")
    }

    async fn emit_sessions(&self) {
        let tabs = self.state.tabs.read().await;
        let count = tabs.len();
        let payload = TerminalSessions {
            tabs: tabs
                .iter()
                .map(|t| TerminalTab {
                    id: t.pty_id.clone(),
                    tmux_session: t.tmux_session.clone(),
                    cwd: t.cwd.clone(),
                })
                .collect(),
        };
        drop(tabs);
        if self.emit_tx.send(Topic::TerminalSessions(payload)).is_err() {
            tracing::warn!("emit channel closed; sessions topic dropped");
            return;
        }
        let menu = crate::menu::terminal_menu(count);
        if self.emit_tx.send(Topic::SetAppMenu(menu)).is_err() {
            tracing::warn!("emit channel closed; menu topic dropped");
        }
    }
}

async fn forward_pty_events(
    _pty_id: String,
    mut event_rx: mpsc::UnboundedReceiver<PtyEvent>,
    tx: std::sync::mpsc::Sender<String>,
) {
    let b64 = base64::engine::general_purpose::STANDARD;

    while let Some(event) = event_rx.recv().await {
        let msg = match event {
            PtyEvent::Data { pty_id, data } => json!({
                "event": "pty:data",
                "pty_id": pty_id,
                "data": b64.encode(&data),
            }),
            PtyEvent::Scrollback { pty_id, data } => json!({
                "event": "pty:scrollback",
                "pty_id": pty_id,
                "data": b64.encode(&data),
            }),
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
