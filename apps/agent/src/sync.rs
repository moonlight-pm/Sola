//! Reconcile our display sessions with Claude CLI's session storage.
//!
//! Scans `~/.claude/projects/` for session JSONL files. For each one,
//! rebuilds our view-model meta + history if the CLI JSONL has been
//! modified since our last sync (tracked via `cli_synced_at`).
//!
//! Emits sync progress events so the frontend can show an indicator.

use crate::meta::MetaStore;
use crate::storage::{self, SessionMeta};
use serde_json::{json, Value};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{debug, info, warn};

/// A discovered Claude CLI session.
struct CliSession {
    session_id: String,
    jsonl_path: PathBuf,
    cwd: Option<String>,
    first_prompt: Option<String>,
}

/// Run a full sync pass. Intended to be called from a background thread
/// on startup. Emits `sync_start` / `session_updated` / `sync_complete`
/// events over `event_tx`.
pub fn run_sync(event_tx: &Sender<String>, meta_store: &Arc<MetaStore>) {
    let cli_sessions = scan_cli_sessions();
    info!(count = cli_sessions.len(), "discovered CLI sessions");

    let to_rebuild: Vec<&CliSession> = cli_sessions
        .iter()
        .filter(|cli| needs_rebuild(cli, meta_store))
        .collect();

    let total = to_rebuild.len();
    info!(total, "sync: sessions needing rebuild");

    if total == 0 {
        send_event(event_tx, json!({"event": "sync_complete"}));
        return;
    }

    send_event(event_tx, json!({"event": "sync_start", "total": total}));

    for (idx, cli) in to_rebuild.iter().enumerate() {
        match rebuild(cli, meta_store) {
            Ok(meta) => {
                let first_prompt = first_user_text(&meta.session_id).unwrap_or_default();
                send_event(event_tx, json!({
                    "event": "session_updated",
                    "session_id": meta.session_id,
                    "name": meta.name,
                    "first_prompt": first_prompt,
                    "working_dir": meta.working_dir,
                    "updated_at": meta.updated_at,
                    "metrics": meta.metrics,
                    "model": meta.model,
                    "effort": meta.effort,
                    "current": idx + 1,
                    "total": total,
                }));
            }
            Err(e) => {
                warn!(session_id = %cli.session_id, "rebuild failed: {:#}", e);
            }
        }
    }

    send_event(event_tx, json!({"event": "sync_complete"}));
}

/// Check if a terminal CLI JSONL exists for a given session — used by
/// `agent.rs` to decide between `--resume` and `--session-id`.
pub fn cli_session_exists(session_id: &str) -> bool {
    find_cli_jsonl(&projects_root(), session_id).is_some()
}

/// Remove every Claude CLI artifact tied to a session id. Best-effort:
/// logs per-path failures but always continues. Only touches paths whose
/// final component is a literal match for the session UUID (or the
/// exact `security_warnings_state_<uuid>.json` / `<uuid>-agent-<uuid>.json`
/// form). Refuses to run unless `session_id` parses as a UUID — guards
/// against path traversal via a malformed id.
pub fn cli_delete_session(session_id: &str) {
    if !is_uuid(session_id) {
        warn!(session_id, "cli_delete_session: not a UUID, refusing");
        return;
    }

    let Ok(home) = std::env::var("HOME") else { return };
    let claude = PathBuf::from(&home).join(".claude");

    // Single-file artifacts.
    let files = [
        claude.join(format!("security_warnings_state_{session_id}.json")),
        claude.join("todos").join(format!("{session_id}-agent-{session_id}.json")),
    ];
    for path in &files {
        remove_file_if_present(path);
    }

    // Single-directory artifacts.
    let dirs = [
        claude.join("session-env").join(session_id),
        claude.join("tasks").join(session_id),
        claude.join("file-history").join(session_id),
    ];
    for path in &dirs {
        remove_dir_if_present(path);
    }

    // projects/<any>/<id>.jsonl + projects/<any>/<id>/ — scan each project dir.
    if let Ok(entries) = std::fs::read_dir(claude.join("projects")) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() { continue; }
            remove_file_if_present(&p.join(format!("{session_id}.jsonl")));
            remove_dir_if_present(&p.join(session_id));
        }
    }
}

fn is_uuid(s: &str) -> bool {
    if s.len() != 36 { return false; }
    let b = s.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => if c != b'-' { return false; },
            _ => if !c.is_ascii_hexdigit() { return false; },
        }
    }
    true
}

fn remove_file_if_present(path: &Path) {
    if !path.exists() { return; }
    if let Err(e) = std::fs::remove_file(path) {
        warn!(path = %path.display(), "cli delete failed: {e}");
    } else {
        debug!(path = %path.display(), "cli file deleted");
    }
}

fn remove_dir_if_present(path: &Path) {
    if !path.exists() { return; }
    if let Err(e) = std::fs::remove_dir_all(path) {
        warn!(path = %path.display(), "cli delete failed: {e}");
    } else {
        debug!(path = %path.display(), "cli dir deleted");
    }
}

// ── Internals ──────────────────────────────────────────────────────────────

fn projects_root() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".claude/projects")
}

fn scan_cli_sessions() -> Vec<CliSession> {
    let root = projects_root();
    let mut sessions = Vec::new();

    let dirs = match std::fs::read_dir(&root) {
        Ok(rd) => rd,
        Err(_) => return sessions,
    };

    for dir_entry in dirs.flatten() {
        let dir = dir_entry.path();
        if !dir.is_dir() { continue; }

        let files = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };

        for entry in files.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") { continue; }
            if path.to_string_lossy().contains("/subagents/") { continue; }

            let session_id = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };

            let (cwd, first_prompt) = parse_cli_head(&path);

            sessions.push(CliSession {
                session_id,
                jsonl_path: path,
                cwd,
                first_prompt,
            });
        }
    }

    sessions
}

fn parse_cli_head(path: &Path) -> (Option<String>, Option<String>) {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (None, None),
    };
    let reader = std::io::BufReader::new(file);
    let mut cwd = None;
    let mut first_prompt = None;

    for line in reader.lines().take(30) {
        let line = match line { Ok(l) => l, Err(_) => continue };
        let obj: Value = match serde_json::from_str(&line) { Ok(v) => v, Err(_) => continue };

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

/// CLI JSONL mtime in ms since epoch, or 0 if unavailable.
fn cli_mtime_ms(path: &Path) -> u64 {
    path.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Bump whenever aggregation in `aggregate_metrics` changes output so
/// stale per-turn-snapshot metrics get refreshed.
const METRICS_SCHEMA: u8 = 2;

fn needs_rebuild(cli: &CliSession, meta_store: &MetaStore) -> bool {
    let cli_mtime = cli_mtime_ms(&cli.jsonl_path);
    match meta_store.get(&cli.session_id) {
        Some(m) => m.cli_synced_at < cli_mtime || m.metrics_schema < METRICS_SCHEMA,
        None => true,
    }
}

fn find_cli_jsonl(projects_root: &Path, session_id: &str) -> Option<PathBuf> {
    let dirs = std::fs::read_dir(projects_root).ok()?;
    for entry in dirs.flatten() {
        let path = entry.path().join(format!("{session_id}.jsonl"));
        if path.exists() { return Some(path); }
    }
    None
}

/// Rebuild the view model for a CLI session: rewrite history JSONL from
/// the CLI JSONL (respecting compact boundaries), then hand merged
/// metadata to the store. User-editable fields (name, model, effort) are
/// preserved by `MetaStore::apply_cli_rebuild`.
fn rebuild(cli: &CliSession, meta_store: &MetaStore) -> anyhow::Result<SessionMeta> {
    let messages = extract_display_messages(&cli.jsonl_path);
    storage::write_history(&cli.session_id, &messages)?;

    let existing = meta_store.get(&cli.session_id);
    let working_dir = cli.cwd.clone().unwrap_or_else(|| {
        existing.as_ref().map(|e| e.working_dir.clone()).unwrap_or_else(|| ".".into())
    });

    let cli_synced_at = cli_mtime_ms(&cli.jsonl_path);

    // Recompute cumulative metrics from the CLI JSONL for exact totals.
    // Cost isn't in the JSONL — preserve whatever we had.
    let metrics = aggregate_metrics(
        &cli.jsonl_path,
        existing.as_ref().and_then(|e| e.metrics.clone()),
    );

    let meta = meta_store.apply_cli_rebuild(
        &cli.session_id,
        working_dir,
        cli.first_prompt.clone(),
        cli_synced_at,
        METRICS_SCHEMA,
        metrics,
    )?;

    debug!(session_id = %cli.session_id, cli_synced_at, "rebuilt view model");
    Ok(meta)
}

/// Walk the CLI JSONL and compute exact cumulative metrics:
/// - input/output/cache_read/cache_creation tokens: sum of message.usage
///   fields across non-sidechain assistant records.
/// - duration_ms: sum of system.turn_duration.durationMs records.
/// - num_turns: count of non-sidechain assistant records (1 LLM iteration each).
/// - model: latest non-sidechain assistant record's model field.
/// - context_window: derived from model name ("[1m]" suffix → 1_000_000).
/// - context_used_pct: latest assistant record's token sum / context_window.
/// - total_cost_usd: preserved from `existing` (not in JSONL).
fn aggregate_metrics(path: &Path, existing: Option<Value>) -> Option<Value> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return existing,
    };
    let reader = std::io::BufReader::new(file);

    let mut input = 0u64;
    let mut output = 0u64;
    let mut cache_read = 0u64;
    let mut cache_creation = 0u64;
    let mut duration = 0u64;
    let mut iterations = 0u64;
    let mut model: Option<String> = None;
    let mut last_turn_total: u64 = 0;

    for line in reader.lines() {
        let line = match line { Ok(l) => l, Err(_) => continue };
        let obj: Value = match serde_json::from_str(&line) { Ok(v) => v, Err(_) => continue };

        let t = obj["type"].as_str().unwrap_or("");
        if t == "system" && obj["subtype"].as_str() == Some("turn_duration") {
            duration += obj["durationMs"].as_u64().unwrap_or(0);
            continue;
        }
        if t != "assistant" { continue; }
        if obj["isSidechain"].as_bool() == Some(true) { continue; }

        let usage = &obj["message"]["usage"];
        let ti = usage["input_tokens"].as_u64().unwrap_or(0);
        let to = usage["output_tokens"].as_u64().unwrap_or(0);
        let tcr = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
        let tcc = usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
        input += ti;
        output += to;
        cache_read += tcr;
        cache_creation += tcc;
        iterations += 1;
        last_turn_total = ti + to + tcr + tcc;

        if let Some(m) = obj["message"]["model"].as_str() {
            model = Some(m.to_string());
        }
    }

    // Model: prefer whatever JSONL said last; fall back to existing meta
    // (which may carry a "[1m]" variant the JSONL never records).
    let model_from_meta = existing
        .as_ref()
        .and_then(|m| m.get("model"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let model_str = model.or(model_from_meta).unwrap_or_else(|| "unknown".to_string());

    // Context window: JSONL records don't include the "[1m]" qualifier,
    // so never downgrade from what we already knew. Keep the prior meta
    // value when it was set; only derive from model name as a fallback.
    let prev_window = existing
        .as_ref()
        .and_then(|m| m.get("context_window"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let derived_window = if model_str.contains("[1m]") { 1_000_000 } else { 200_000 };
    let context_window = if prev_window > 0 { prev_window } else { derived_window };

    // Last-turn fill as % of window. Clamp to 100 — sum of a single turn's
    // cache_read + cache_creation + input + output can briefly exceed the
    // window on tool-use inner iterations, which would render as a
    // nonsense > 100% reading.
    let context_used_pct = if context_window > 0 {
        ((last_turn_total as f64 / context_window as f64 * 100.0).round() as u64).min(100)
    } else { 0 };

    // Duration and cost aren't in the JSONL — preserve what we had.
    // (system.turn_duration records exist in < 10% of sessions.)
    let total_cost_usd = existing
        .as_ref()
        .and_then(|m| m.get("total_cost_usd"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let preserved_duration = existing
        .as_ref()
        .and_then(|m| m.get("duration_ms"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let duration_final = duration.max(preserved_duration);

    Some(json!({
        "input_tokens": input,
        "output_tokens": output,
        "cache_read_tokens": cache_read,
        "cache_creation_tokens": cache_creation,
        "duration_ms": duration_final,
        "num_turns": iterations,
        "model": model_str,
        "context_window": context_window,
        "context_used_pct": context_used_pct,
        "total_cost_usd": total_cost_usd,
    }))
}

/// Walk the CLI JSONL and produce our display message list.
/// - Skips meta/sidechain, tool-result-only user messages, thinking blocks.
/// - Respects `isCompactSummary: true` by clearing accumulated messages at
///   the boundary; the summary itself becomes the first message.
fn extract_display_messages(path: &Path) -> Vec<Value> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let reader = std::io::BufReader::new(file);
    let mut messages = Vec::new();

    for line in reader.lines() {
        let line = match line { Ok(l) => l, Err(_) => continue };
        let obj: Value = match serde_json::from_str(&line) { Ok(v) => v, Err(_) => continue };

        let is_compact = obj["isCompactSummary"].as_bool() == Some(true);
        if is_compact {
            messages.clear();
        }

        let msg_type = obj["type"].as_str().unwrap_or("");
        match msg_type {
            "user" => {
                if !is_compact && obj["isMeta"].as_bool() == Some(true) { continue; }
                if obj["isSidechain"].as_bool() == Some(true) { continue; }

                let content = &obj["message"]["content"];
                // Compact summary content is a string; normalize to a text-block array.
                let content = if let Some(s) = content.as_str() {
                    json!([{"type": "text", "text": s}])
                } else if let Some(arr) = content.as_array() {
                    if !is_compact && arr.iter().all(|b| b["type"].as_str() == Some("tool_result")) {
                        continue;
                    }
                    Value::Array(arr.clone())
                } else {
                    continue;
                };

                messages.push(json!({"role": "user", "content": content}));
            }
            "assistant" => {
                if obj["isSidechain"].as_bool() == Some(true) { continue; }
                let content = match obj["message"]["content"].as_array() {
                    Some(c) => c,
                    None => continue,
                };
                let display: Vec<Value> = content.iter()
                    .filter(|b| b["type"].as_str() != Some("thinking"))
                    .cloned()
                    .collect();
                if display.is_empty() { continue; }
                messages.push(json!({"role": "assistant", "content": display}));
            }
            _ => {}
        }
    }

    messages
}

fn first_user_text(session_id: &str) -> Option<String> {
    let msgs = storage::load_history(session_id).ok()?;
    for m in &msgs {
        if m["role"].as_str() != Some("user") { continue; }
        let content = &m["content"];
        if let Some(arr) = content.as_array() {
            for b in arr {
                if b["type"].as_str() == Some("text") {
                    if let Some(t) = b["text"].as_str() {
                        let trimmed = if t.len() > 200 { &t[..200] } else { t };
                        return Some(trimmed.to_string());
                    }
                }
            }
        } else if let Some(s) = content.as_str() {
            let trimmed = if s.len() > 200 { &s[..200] } else { s };
            return Some(trimmed.to_string());
        }
    }
    None
}

fn send_event(tx: &Sender<String>, value: Value) {
    let _ = tx.send(value.to_string());
}
