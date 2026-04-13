use std::sync::Arc;

use serde_json::{json, Value};

use crate::agent;
use crate::session::SessionManager;

pub struct AgentHandler {
    pub session_mgr: Arc<SessionManager>,
    pub event_tx: std::sync::mpsc::Sender<String>,
}

#[async_trait::async_trait]
impl sola_app::AppHandler for AgentHandler {
    async fn dispatch(&self, cmd: &str, args: &Value) -> Value {
        match cmd {
            "new_session" => self.cmd_new_session(args).await,
            "send_message" => self.cmd_send_message(args).await,
            "cancel" => self.cmd_cancel(args).await,
            "close_session" => self.cmd_close_session(args).await,
            "rename_conversation" => self.cmd_rename(args).await,
            "list_conversations" => self.cmd_list_conversations().await,
            _ => json!({ "error": format!("unknown command: {cmd}") }),
        }
    }
}

impl AgentHandler {
    async fn cmd_new_session(&self, args: &Value) -> Value {
        let Some(working_dir) = args.get("working_dir").and_then(|v| v.as_str()) else {
            return json!({ "error": "working_dir is required" });
        };

        let expanded = if working_dir.starts_with("~/") {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{}/{}", home, &working_dir[2..])
        } else if working_dir == "~" {
            std::env::var("HOME").unwrap_or_default()
        } else {
            working_dir.to_string()
        };
        let dir = std::path::PathBuf::from(&expanded);
        if !dir.is_dir() {
            return json!({ "error": format!("Not a directory: {}", working_dir) });
        }

        let folder_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| expanded.clone());
        let session_id = self.session_mgr.create_session(dir).await;
        self.session_mgr
            .rename_session(&session_id, folder_name.clone())
            .await;
        self.send_event(json!({
            "event": "session_state",
            "session_id": session_id,
            "status": "idle",
            "name": folder_name,
            "working_dir": expanded
        }));

        json!({ "session_id": session_id })
    }

    async fn cmd_send_message(&self, args: &Value) -> Value {
        let Some(session_id) = args.get("session_id").and_then(|v| v.as_str()) else {
            return json!({ "error": "session_id is required" });
        };
        let Some(text) = args.get("text").and_then(|v| v.as_str()) else {
            return json!({ "error": "text is required" });
        };

        let working_dir = {
            let sessions = self.session_mgr.sessions.read().await;
            match sessions.get(session_id) {
                Some(s) => s.working_dir.clone(),
                None => return json!({ "error": "Session not found" }),
            }
        };

        let cancel_token = {
            let mut sessions = self.session_mgr.sessions.write().await;
            let session = sessions.get_mut(session_id).unwrap();
            session.cancel_token = tokio_util::sync::CancellationToken::new();
            session.cancel_token.clone()
        };

        let session_id = session_id.to_string();
        let text = text.to_string();
        let session_mgr = self.session_mgr.clone();
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            agent::run_session_message(
                session_id,
                text,
                working_dir,
                session_mgr,
                event_tx,
                cancel_token,
            )
            .await;
        });

        json!({ "ok": true })
    }

    async fn cmd_cancel(&self, args: &Value) -> Value {
        let Some(session_id) = args.get("session_id").and_then(|v| v.as_str()) else {
            return json!({ "error": "session_id is required" });
        };
        self.session_mgr.cancel_session(session_id).await;
        json!({ "ok": true })
    }

    async fn cmd_close_session(&self, args: &Value) -> Value {
        let Some(session_id) = args.get("session_id").and_then(|v| v.as_str()) else {
            return json!({ "error": "session_id is required" });
        };
        self.session_mgr.close_session(session_id).await;
        json!({ "ok": true })
    }

    async fn cmd_rename(&self, args: &Value) -> Value {
        let Some(session_id) = args.get("session_id").and_then(|v| v.as_str()) else {
            return json!({ "error": "session_id is required" });
        };
        let Some(name) = args.get("name").and_then(|v| v.as_str()) else {
            return json!({ "error": "name is required" });
        };
        self.session_mgr
            .rename_session(session_id, name.to_string())
            .await;
        json!({ "ok": true })
    }

    async fn cmd_list_conversations(&self) -> Value {
        json!({ "conversations": [] })
    }

    fn send_event(&self, value: Value) {
        let _ = self.event_tx.send(value.to_string());
    }
}
