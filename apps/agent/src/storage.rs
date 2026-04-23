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
    /// CLI JSONL mtime (ms since epoch) at last view-model rebuild.
    /// Zero means "never synced"; triggers rebuild.
    #[serde(default)]
    pub cli_synced_at: u64,
    /// Bumped when sync.rs's aggregation logic changes so old metrics
    /// can be detected and rebuilt. Zero = pre-versioning (per-turn
    /// snapshot metrics) → force rebuild.
    #[serde(default)]
    pub metrics_schema: u8,
}

fn default_model() -> String {
    "opus".into()
}
fn default_effort() -> String {
    "high".into()
}

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

/// Save a full SessionMeta struct directly.
pub fn save_meta_full(meta: &SessionMeta) -> Result<()> {
    let dir = sessions_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    let json = serde_json::to_string_pretty(meta)?;
    std::fs::write(meta_path(&meta.session_id), json)?;
    Ok(())
}

/// Overwrite the session's JSONL history with the given messages.
/// Used by sync when rebuilding a view model from a CLI JSONL.
pub fn write_history(session_id: &str, messages: &[Value]) -> Result<()> {
    let dir = sessions_dir();
    std::fs::create_dir_all(&dir)?;
    let path = history_path(session_id);
    let mut file = std::fs::File::create(&path)
        .with_context(|| format!("Failed to create {}", path.display()))?;
    for msg in messages {
        let line = serde_json::to_string(msg)?;
        writeln!(file, "{}", line)?;
    }
    Ok(())
}

/// Append a message to the session's JSONL history.
/// Callers must bump `meta.updated_at` via MetaStore — this function
/// never touches the meta file.
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
    Ok(())
}

/// Load conversation history from JSONL.
pub fn load_history(session_id: &str) -> Result<Vec<Value>> {
    let path = history_path(session_id);
    let file =
        std::fs::File::open(&path).with_context(|| format!("Failed to open {}", path.display()))?;
    let reader = std::io::BufReader::new(file);
    let mut messages = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let msg: Value = serde_json::from_str(&line)?;
        messages.push(msg);
    }
    Ok(messages)
}

/// Delete session metadata and history files from disk.
pub fn delete_session(session_id: &str) -> Result<()> {
    for path in [
        meta_path(session_id),
        history_path(session_id),
        raw_path(session_id),
    ] {
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
