//! Session persistence — saves conversation history to disk.
//!
//! Storage layout:
//!   ~/.config/sola/agent/sessions/{session_id}.json

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSession {
    pub session_id: String,
    pub name: Option<String>,
    pub working_dir: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub messages: Vec<Value>,
    /// Last known metrics from the most recent API response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<Value>,
}

fn sessions_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".config/sola/agent/sessions")
}

fn session_path(session_id: &str) -> PathBuf {
    sessions_dir().join(format!("{}.json", session_id))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Save a session with raw JSON messages.
pub fn save_raw(
    session_id: &str,
    name: Option<&str>,
    working_dir: &str,
    messages: &[Value],
) -> Result<()> {
    save_with_metrics(session_id, name, working_dir, messages, None)
}

/// Save a session with raw JSON messages and optional metrics.
pub fn save_with_metrics(
    session_id: &str,
    name: Option<&str>,
    working_dir: &str,
    messages: &[Value],
    metrics: Option<Value>,
) -> Result<()> {
    let dir = sessions_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create {}", dir.display()))?;

    let path = session_path(session_id);

    // Preserve created_at and metrics from existing file
    let existing = load(session_id).ok();
    let created_at = existing.as_ref().map(|e| e.created_at).unwrap_or_else(now_ms);
    let metrics = metrics.or_else(|| existing.and_then(|e| e.metrics));

    let session = SavedSession {
        session_id: session_id.to_string(),
        name: name.map(String::from),
        working_dir: working_dir.to_string(),
        created_at,
        updated_at: now_ms(),
        messages: messages.to_vec(),
        metrics,
    };

    let json = serde_json::to_string_pretty(&session)?;
    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write {}", path.display()))?;

    tracing::debug!(session_id, "Session saved ({} messages)", messages.len());
    Ok(())
}

/// Load a session from disk.
pub fn load(session_id: &str) -> Result<SavedSession> {
    let path = session_path(session_id);
    let json = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let session: SavedSession = serde_json::from_str(&json)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(session)
}

/// List all saved sessions, sorted by most recently updated.
pub fn list_all() -> Vec<SavedSession> {
    let dir = sessions_dir();
    let mut sessions: Vec<SavedSession> = Vec::new();

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return sessions,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(json) = std::fs::read_to_string(&path) {
            if let Ok(session) = serde_json::from_str::<SavedSession>(&json) {
                sessions.push(session);
            }
        }
    }

    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    sessions
}
