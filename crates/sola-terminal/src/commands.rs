use std::sync::Arc;

use base64::Engine;
use serde_json::{Value, json};
use sola_bus::topics::{TerminalSession, Topic};
use tokio::sync::mpsc;

use crate::BusOp;
use crate::pty::PtyEvent;
use crate::state::{TabEntry, TerminalState};

/// Terminal command handler implementing the sola-app AppHandler trait.
/// Emits/retracts bus topics by sending `BusOp`s through `bus_tx`; the
/// main thread drains and calls `ctx.emit`/`ctx.retract`.
pub struct TerminalHandler {
    pub state: Arc<TerminalState>,
    pub event_tx: std::sync::mpsc::Sender<String>,
    pub bus_tx: std::sync::mpsc::Sender<BusOp>,
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
        let provided_pty_id = args
            .get("pty_id")
            .and_then(|v| v.as_str())
            .map(String::from);

        // Restore path: JS passed the persisted pty_id of a tab the bus
        // already replayed into our mirror. Spawn the PTY (which attaches
        // to the existing tmux session) but skip mirror push + bus emit
        // — both already exist.
        let is_restore = match &provided_pty_id {
            Some(id) => {
                let tabs = self.state.tabs.read().await;
                tabs.iter().any(|t| &t.pty_id == id)
            }
            None => false,
        };

        let pty_id = provided_pty_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
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

        let tx = self.event_tx.clone();
        tokio::spawn(forward_pty_events(pty_id.clone(), pty_event_rx, tx));

        if !is_restore {
            let new_session = {
                let mut tabs = self.state.tabs.write().await;
                let ordinal = tabs.iter().map(|t| t.ordinal).max().map_or(0, |m| m + 1);
                let entry = TabEntry {
                    pty_id: pty_id.clone(),
                    tmux_session: tmux_session_name.clone(),
                    cwd,
                    ordinal,
                };
                tabs.push(entry.clone());
                TerminalSession {
                    id: entry.pty_id,
                    tmux_session: entry.tmux_session,
                    cwd: entry.cwd,
                    ordinal: entry.ordinal,
                }
            };
            self.emit_session(new_session).await;
            self.emit_menu().await;
        }

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

        let removed = {
            let mut tabs = self.state.tabs.write().await;
            let pos = tabs.iter().position(|t| t.pty_id == pty_id);
            pos.map(|i| tabs.remove(i))
        };

        if let Some(entry) = removed {
            self.retract_session(entry).await;
            self.emit_menu().await;
        }

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

        let updated = {
            let mut tabs = self.state.tabs.write().await;
            tabs.iter_mut()
                .find(|t| t.pty_id == pty_id)
                .and_then(|tab| {
                    if tab.cwd.as_deref() == Some(cwd.as_str()) {
                        None
                    } else {
                        tab.cwd = Some(cwd);
                        Some(TerminalSession {
                            id: tab.pty_id.clone(),
                            tmux_session: tab.tmux_session.clone(),
                            cwd: tab.cwd.clone(),
                            ordinal: tab.ordinal,
                        })
                    }
                })
        };

        if let Some(session) = updated {
            self.emit_session(session).await;
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

        // Renumber by the requested order. Any tabs absent from the
        // request keep their previous ordinal — they'll trail the
        // reordered set since their ordinal is unchanged.
        let changed: Vec<TerminalSession> = {
            let mut tabs = self.state.tabs.write().await;
            let mut out = Vec::new();
            for (new_ord, id) in pty_ids.iter().enumerate() {
                let new_ord = new_ord as u32;
                if let Some(tab) = tabs.iter_mut().find(|t| &t.pty_id == id) {
                    if tab.ordinal != new_ord {
                        tab.ordinal = new_ord;
                        out.push(TerminalSession {
                            id: tab.pty_id.clone(),
                            tmux_session: tab.tmux_session.clone(),
                            cwd: tab.cwd.clone(),
                            ordinal: tab.ordinal,
                        });
                    }
                }
            }
            tabs.sort_by_key(|t| t.ordinal);
            out
        };

        for session in changed {
            self.emit_session(session).await;
        }

        json!("ok")
    }

    async fn emit_session(&self, session: TerminalSession) {
        if self
            .bus_tx
            .send(BusOp::Emit(Topic::TerminalSession(session)))
            .is_err()
        {
            tracing::warn!("bus channel closed; TerminalSession emit dropped");
        }
    }

    async fn retract_session(&self, entry: TabEntry) {
        let session = TerminalSession {
            id: entry.pty_id,
            tmux_session: entry.tmux_session,
            cwd: entry.cwd,
            ordinal: entry.ordinal,
        };
        if self
            .bus_tx
            .send(BusOp::Retract(Topic::TerminalSession(session)))
            .is_err()
        {
            tracing::warn!("bus channel closed; TerminalSession retract dropped");
        }
    }

    async fn emit_menu(&self) {
        let count = self.state.tabs.read().await.len();
        let menu = crate::menu::terminal_menu(count);
        if self
            .bus_tx
            .send(BusOp::Emit(Topic::SetAppMenu(menu)))
            .is_err()
        {
            tracing::warn!("bus channel closed; SetAppMenu emit dropped");
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
