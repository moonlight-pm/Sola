//! Session persistence.
//!
//! Storage layout:
//!   ~/.config/sola/agent/sessions/{session_id}.json   — metadata
//!   ~/.config/sola/agent/sessions/{session_id}.jsonl  — conversation history

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub session_id: String,
    pub name: Option<String>,
    pub working_dir: String,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<Value>,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_effort")]
    pub effort: String,
}

fn default_model() -> String { "opus".into() }
fn default_effort() -> String { "high".into() }

fn sessions_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".config/sola/agent/sessions")
}

fn meta_path(session_id: &str) -> PathBuf {
    sessions_dir().join(format!("{}.json", session_id))
}

fn history_path(session_id: &str) -> PathBuf {
    sessions_dir().join(format!("{}.jsonl", session_id))
}

/// Raw NDJSON lines file — stores full stream-json messages for replay.
fn raw_path(session_id: &str) -> PathBuf {
    sessions_dir().join(format!("{}.ndjson", session_id))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Save or update session metadata (preserves model/effort from existing).
pub fn save_meta(
    session_id: &str,
    name: Option<&str>,
    working_dir: &str,
    metrics: Option<Value>,
) -> Result<()> {
    let dir = sessions_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create {}", dir.display()))?;

    let existing = load_meta(session_id).ok();
    let created_at = existing.as_ref().map(|e| e.created_at).unwrap_or_else(now_ms);
    let metrics = metrics.or_else(|| existing.as_ref().and_then(|e| e.metrics.clone()));
    let model = existing.as_ref().map(|e| e.model.clone()).unwrap_or_else(default_model);
    let effort = existing.as_ref().map(|e| e.effort.clone()).unwrap_or_else(default_effort);

    let meta = SessionMeta {
        session_id: session_id.to_string(),
        name: name.map(String::from),
        working_dir: working_dir.to_string(),
        created_at,
        updated_at: now_ms(),
        metrics,
        model,
        effort,
    };

    let json = serde_json::to_string_pretty(&meta)?;
    std::fs::write(meta_path(session_id), json)?;
    Ok(())
}

/// Save a full SessionMeta struct directly.
pub fn save_meta_full(meta: &SessionMeta) -> Result<()> {
    let dir = sessions_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create {}", dir.display()))?;
    let json = serde_json::to_string_pretty(meta)?;
    std::fs::write(meta_path(&meta.session_id), json)?;
    Ok(())
}

/// Append a message to the session's JSONL history.
pub fn append_message(session_id: &str, message: &Value) -> Result<()> {
    let dir = sessions_dir();
    std::fs::create_dir_all(&dir)?;

    let path = history_path(session_id);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("Failed to open {}", path.display()))?;

    let line = serde_json::to_string(message)?;
    writeln!(file, "{}", line)?;

    // Update metadata timestamp
    if let Ok(mut meta) = load_meta(session_id) {
        meta.updated_at = now_ms();
        let json = serde_json::to_string_pretty(&meta)?;
        std::fs::write(meta_path(session_id), json)?;
    }

    Ok(())
}

/// Load session metadata.
pub fn load_meta(session_id: &str) -> Result<SessionMeta> {
    let path = meta_path(session_id);
    let json = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    serde_json::from_str(&json).with_context(|| format!("Failed to parse {}", path.display()))
}

/// Load conversation history from JSONL.
pub fn load_history(session_id: &str) -> Result<Vec<Value>> {
    let path = history_path(session_id);
    let file = std::fs::File::open(&path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let reader = std::io::BufReader::new(file);
    let mut messages = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.is_empty() { continue; }
        let msg: Value = serde_json::from_str(&line)?;
        messages.push(msg);
    }
    Ok(messages)
}

/// Delete session metadata and history files from disk.
pub fn delete_session(session_id: &str) -> Result<()> {
    for path in [meta_path(session_id), history_path(session_id), raw_path(session_id)] {
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
        }
    }
    Ok(())
}

/// List all saved sessions, sorted by most recently updated.
pub fn list_all() -> Vec<SessionMeta> {
    let dir = sessions_dir();
    let mut sessions: Vec<SessionMeta> = Vec::new();

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return sessions,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        // Only look at .json metadata files (not .jsonl)
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(json) = std::fs::read_to_string(&path) {
            if let Ok(meta) = serde_json::from_str::<SessionMeta>(&json) {
                sessions.push(meta);
            }
        }
    }

    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    sessions
}

