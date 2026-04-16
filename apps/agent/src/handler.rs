use std::sync::Arc;

use serde_json::{json, Value};

use crate::agent;
use crate::session::SessionManager;
use crate::storage;

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
            "resume_session" => self.cmd_resume_session(args).await,
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

        // Persist immediately so the session survives app restart
        if let Err(e) = storage::save_meta(&session_id, Some(&folder_name), &expanded, None) {
            tracing::warn!("Failed to save new session: {:#}", e);
        }

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

        // If subprocess is already running, inject as follow-up
        if self.session_mgr.is_running(session_id).await {
            match agent::send_followup(session_id, text, &self.session_mgr).await {
                Ok(()) => return json!({ "ok": true, "followup": true }),
                Err(e) => {
                    tracing::warn!("Follow-up injection failed: {:#}", e);
                    // Fall through to spawn new subprocess
                }
            }
        }

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

        // Update saved metadata with new name
        if let Ok(meta) = storage::load_meta(session_id) {
            let _ = storage::save_meta(session_id, Some(name), &meta.working_dir, meta.metrics);
        }

        json!({ "ok": true })
    }

    async fn cmd_list_conversations(&self) -> Value {
        let metas = storage::list_all();
        tracing::info!(count = metas.len(), "list_conversations");
        let conversations: Vec<Value> = metas
            .iter()
            .map(|m| {
                // Peek at first user message from JSONL for the preview
                let first_prompt = storage::load_history(&m.session_id).ok()
                    .and_then(|msgs| {
                        msgs.iter()
                            .find(|msg| msg.get("role").and_then(|v| v.as_str()) == Some("user"))
                            .and_then(|msg| extract_text_content(msg).into())
                    })
                    .unwrap_or_default();
                json!({
                    "session_id": &m.session_id,
                    "name": &m.name,
                    "first_prompt": first_prompt,
                    "working_dir": &m.working_dir,
                    "updated_at": m.updated_at,
                    "metrics": &m.metrics,
                })
            })
            .collect();
        // Return the conversations directly in the invoke reply; the frontend
        // reads them from the promise result rather than going through a
        // separate event channel.
        json!({ "conversations": conversations })
    }

    async fn cmd_resume_session(&self, args: &Value) -> Value {
        let Some(session_id) = args.get("session_id").and_then(|v| v.as_str()) else {
            return json!({ "error": "session_id is required" });
        };

        let meta = match storage::load_meta(session_id) {
            Ok(m) => m,
            Err(e) => return json!({ "error": format!("Failed to load session: {:#}", e) }),
        };

        let dir = std::path::PathBuf::from(&meta.working_dir);
        self.session_mgr.sessions.write().await.insert(
            session_id.to_string(),
            crate::session::Session::new(dir),
        );
        if let Some(ref name) = meta.name {
            self.session_mgr.rename_session(session_id, name.clone()).await;
        }

        self.send_event(json!({
            "event": "session_state",
            "session_id": session_id,
            "status": "idle",
            "name": meta.name,
            "working_dir": meta.working_dir,
        }));

        // Load and forward conversation history
        let history = storage::load_history(session_id).unwrap_or_default();
        let messages: Vec<Value> = history.iter().map(|m: &Value| {
            let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = extract_text_content(m);
            json!({ "role": role, "content": content })
        }).collect();

        self.send_event(json!({
            "event": "session_loaded",
            "session_id": session_id,
            "messages": messages,
        }));

        json!({ "ok": true })
    }

    fn send_event(&self, value: Value) {
        let _ = self.event_tx.send(value.to_string());
    }
}

fn extract_text_content(msg: &Value) -> String {
    if let Some(content) = msg.get("content") {
        if let Some(text) = content.as_str() {
            return text.to_string();
        }
        if let Some(blocks) = content.as_array() {
            return blocks.iter()
                .filter_map(|b| {
                    if b.get("type").and_then(|v| v.as_str()) == Some("text") {
                        b.get("text").and_then(|v| v.as_str()).map(String::from)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
    String::new()
}
