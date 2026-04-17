//! Reconcile our display sessions with Claude CLI's session storage.
//!
//! On startup, scan ~/.claude/projects/ for JSONL session files.
//! For each one, ensure we have a matching view-model entry in our
//! ~/.config/sola/agent/sessions/ directory. If not, fast-forward
//! through the CLI JSONL to build one.

use crate::storage;
use serde_json::{json, Value};
use std::io::BufRead;
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// A discovered Claude CLI session.
struct CliSession {
    session_id: String,
    cwd: Option<String>,
    first_prompt: Option<String>,
}

/// Scan all Claude CLI sessions and sync our view models.
/// Returns the merged list of session metadata for the frontend.
pub fn sync_sessions() -> Vec<storage::SessionMeta> {
    let cli_sessions = scan_cli_sessions();
    info!(count = cli_sessions.len(), "discovered CLI sessions");

    for cli in &cli_sessions {
        // Check if we already have a view model for this session.
        if storage::load_meta(&cli.session_id).is_ok() {
            continue;
        }

        // Build a view model by fast-forwarding through the CLI JSONL.
        info!(session_id = %cli.session_id, "syncing new CLI session");
        build_view_model(cli);
    }

    storage::list_all()
}

/// Scan ~/.claude/projects/ for session JSONL files.
fn scan_cli_sessions() -> Vec<CliSession> {
    let home = std::env::var("HOME").unwrap_or_default();
    let projects_root = PathBuf::from(&home).join(".claude/projects");
    let mut sessions = Vec::new();

    let dirs: Vec<PathBuf> = match std::fs::read_dir(&projects_root) {
        Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect(),
        Err(_) => return sessions,
    };

    for dir in &dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            // Skip subagent directories
            if path.to_string_lossy().contains("/subagents/") {
                continue;
            }

            let session_id = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };

            // Quick parse: read first 30 lines for metadata.
            let (cwd, first_prompt) = parse_cli_head(&path);

            sessions.push(CliSession {
                session_id,
                cwd,
                first_prompt,
            });
        }
    }

    sessions
}

/// Read the head of a CLI JSONL file for cwd and first user prompt.
fn parse_cli_head(path: &PathBuf) -> (Option<String>, Option<String>) {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (None, None),
    };
    let reader = std::io::BufReader::new(file);
    let mut cwd = None;
    let mut first_prompt = None;

    for line in reader.lines().take(30) {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let obj: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if cwd.is_none() {
            cwd = obj["cwd"].as_str().filter(|s| !s.is_empty()).map(String::from);
        }

        if first_prompt.is_none()
            && obj["type"].as_str() == Some("user")
            && obj["isMeta"].as_bool() != Some(true)
        {
            if let Some(content) = obj["message"]["content"].as_array() {
                for block in content {
                    if block["type"].as_str() == Some("text") {
                        if let Some(text) = block["text"].as_str() {
                            if !text.starts_with('<') {
                                let truncated = if text.len() > 200 { &text[..200] } else { text };
                                first_prompt = Some(truncated.to_string());
                                break;
                            }
                        }
                    }
                }
            }
        }

        if cwd.is_some() && first_prompt.is_some() { break; }
    }

    (cwd, first_prompt)
}

/// Build our display view model for a CLI session by fast-forwarding
/// through its JSONL and extracting user/assistant messages.
fn build_view_model(cli: &CliSession) {
    let home = std::env::var("HOME").unwrap_or_default();
    let projects_root = PathBuf::from(&home).join(".claude/projects");

    // Find the JSONL file for this session.
    let jsonl_path = find_cli_jsonl(&projects_root, &cli.session_id);
    let jsonl_path = match jsonl_path {
        Some(p) => p,
        None => return,
    };

    let file = match std::fs::File::open(&jsonl_path) {
        Ok(f) => f,
        Err(e) => {
            warn!(session_id = %cli.session_id, "failed to open CLI JSONL: {e}");
            return;
        }
    };

    let reader = std::io::BufReader::new(file);
    let working_dir = cli.cwd.as_deref().unwrap_or(".");

    // Create the meta file first.
    let _ = storage::save_meta(
        &cli.session_id,
        cli.first_prompt.as_deref(), // use first prompt as name
        working_dir,
        None,
    );

    // Fast-forward: extract user and assistant messages for display.
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let obj: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let msg_type = obj["type"].as_str().unwrap_or("");

        match msg_type {
            "user" => {
                // Skip tool_result-only messages and meta messages.
                if obj["isMeta"].as_bool() == Some(true) { continue; }
                let content = match obj["message"]["content"].as_array() {
                    Some(c) => c,
                    None => continue,
                };
                let all_tool_result = content.iter().all(|b| b["type"].as_str() == Some("tool_result"));
                if all_tool_result { continue; }

                let display_msg = json!({"role": "user", "content": content});
                let _ = storage::append_message(&cli.session_id, &display_msg);
            }

            "assistant" => {
                let content = match obj["message"]["content"].as_array() {
                    Some(c) => c.clone(),
                    None => continue,
                };
                // Filter thinking blocks for display.
                let display: Vec<Value> = content.iter()
                    .filter(|b| b["type"].as_str() != Some("thinking"))
                    .cloned()
                    .collect();
                if display.is_empty() { continue; }

                let display_msg = json!({"role": "assistant", "content": display});
                let _ = storage::append_message(&cli.session_id, &display_msg);
            }

            _ => {}
        }
    }

    debug!(session_id = %cli.session_id, "built view model");
}

/// Find the CLI JSONL file for a session ID across all project directories.
fn find_cli_jsonl(projects_root: &PathBuf, session_id: &str) -> Option<PathBuf> {
    let dirs = std::fs::read_dir(projects_root).ok()?;
    for entry in dirs.flatten() {
        let path = entry.path().join(format!("{session_id}.jsonl"));
        if path.exists() { return Some(path); }
    }
    None
}

/// Check if a CLI session JSONL exists (used by agent.rs).
pub fn cli_session_exists(session_id: &str) -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    let projects_root = PathBuf::from(&home).join(".claude/projects");
    find_cli_jsonl(&projects_root, session_id).is_some()
}
