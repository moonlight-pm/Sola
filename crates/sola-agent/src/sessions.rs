//! List and rebuild Grok sessions from `~/.grok/sessions`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::overlay;
use crate::protocol::{PlanEntry, SessionSummary, ToolTurn, Turn};

fn grok_home() -> PathBuf {
    if let Ok(h) = std::env::var("GROK_HOME") {
        return PathBuf::from(h);
    }
    dirs_home().join(".grok")
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn sessions_root() -> PathBuf {
    grok_home().join("sessions")
}

fn encode_cwd(cwd: &str) -> String {
    // Grok URL-encodes the absolute path as the group directory name.
    let abs = PathBuf::from(cwd);
    let abs = abs
        .canonicalize()
        .unwrap_or(abs);
    urlencoding::encode(&abs.to_string_lossy()).into_owned()
}

/// Sessions for a project working directory, newest first.
pub fn list_for_cwd(cwd: &str) -> Vec<SessionSummary> {
    let group = sessions_root().join(encode_cwd(cwd));
    let pins = overlay::load();
    let mut out = Vec::new();
    let entries = match fs::read_dir(&group) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = match path.file_name().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let summary_path = path.join("summary.json");
        let (title, updated, cwd_s) = match read_summary(&summary_path) {
            Some(t) => t,
            None => continue,
        };
        let pinned = pins.pinned.contains(&id);
        out.push(SessionSummary {
            id,
            title,
            cwd: cwd_s,
            updated,
            pinned,
        });
    }
    out.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.updated.cmp(&a.updated)));
    out
}

fn read_summary(path: &Path) -> Option<(String, u64, String)> {
    let raw = fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let id_cwd = v
        .pointer("/info/cwd")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let title = v
        .get("generated_title")
        .or_else(|| v.get("session_summary"))
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("(untitled)")
        .to_string();
    let updated = parse_ts(v.get("updated_at").and_then(|t| t.as_str()))
        .or_else(|| parse_ts(v.get("last_active_at").and_then(|t| t.as_str())))
        .unwrap_or(0);
    Some((title, updated, id_cwd))
}

fn parse_ts(s: Option<&str>) -> Option<u64> {
    let s = s?;
    // RFC3339-ish: 2026-07-18T17:33:45.778327572Z — chrono optional; crude parse via file mtime fallback
    // Prefer chrono if we added it
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp() as u64)
        .or_else(|| {
            // truncate fractional to 6 digits for chrono
            let trimmed = if let Some(dot) = s.find('.') {
                let (a, rest) = s.split_at(dot);
                let frac: String = rest
                    .chars()
                    .skip(1)
                    .take_while(|c| c.is_ascii_digit())
                    .take(6)
                    .collect();
                let z = if rest.ends_with('Z') { "Z" } else { "" };
                format!("{a}.{frac}{z}")
            } else {
                s.to_string()
            };
            chrono::DateTime::parse_from_rfc3339(&trimmed)
                .ok()
                .map(|d| d.timestamp() as u64)
        })
}

pub fn title_for(cwd: &str, id: &str) -> Option<String> {
    let path = sessions_root()
        .join(encode_cwd(cwd))
        .join(id)
        .join("summary.json");
    read_summary(&path).map(|(t, _, _)| t)
}

/// Rebuild display turns from Grok `updates.jsonl` for a session.
pub fn turns_from_updates_jsonl(cwd: &str, id: &str) -> Vec<Turn> {
    let path = sessions_root()
        .join(encode_cwd(cwd))
        .join(id)
        .join("updates.jsonl");
    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mut turns: Vec<Turn> = Vec::new();
    let mut tool_index: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for line in raw.lines() {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
        if method != "session/update" && !method.ends_with("session/update") {
            continue;
        }
        let update = v
            .pointer("/params/update")
            .cloned()
            .unwrap_or(Value::Null);
        let kind = update
            .get("sessionUpdate")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        match kind {
            "user_message_chunk" => {
                if let Some(text) = chunk_text(&update) {
                    append_text(&mut turns, TurnKind::User, &text);
                }
            }
            "agent_message_chunk" => {
                if let Some(text) = chunk_text(&update) {
                    append_text(&mut turns, TurnKind::Assistant, &text);
                }
            }
            "agent_thought_chunk" => {
                if let Some(text) = chunk_text(&update) {
                    append_text(&mut turns, TurnKind::Thought, &text);
                }
            }
            "tool_call" => {
                let call_id = update
                    .get("toolCallId")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool = update
                    .get("title")
                    .and_then(|s| s.as_str())
                    .unwrap_or("tool")
                    .to_string();
                let args = update.get("rawInput").cloned().unwrap_or(Value::Null);
                let idx = turns.len();
                turns.push(Turn::Tool(ToolTurn {
                    call_id: call_id.clone(),
                    tool,
                    args,
                    status: "pending".into(),
                    output: String::new(),
                }));
                tool_index.insert(call_id, idx);
            }
            "tool_call_update" => {
                let call_id = update
                    .get("toolCallId")
                    .and_then(|s| s.as_str())
                    .unwrap_or("");
                if let Some(&idx) = tool_index.get(call_id) {
                    if let Turn::Tool(tt) = &mut turns[idx] {
                        if let Some(s) = update.get("status").and_then(|s| s.as_str()) {
                            tt.status = s.to_string();
                        }
                        if let Some(t) = update.get("title").and_then(|s| s.as_str()) {
                            tt.tool = t.to_string();
                        }
                        if let Some(out) = tool_content(&update) {
                            tt.output = out;
                        }
                    }
                }
            }
            "plan" => {
                let entries = update
                    .get("entries")
                    .and_then(|e| e.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|e| {
                                Some(PlanEntry {
                                    content: e.get("content")?.as_str()?.to_string(),
                                    status: e
                                        .get("status")
                                        .and_then(|s| s.as_str())
                                        .unwrap_or("pending")
                                        .to_string(),
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if !entries.is_empty() {
                    turns.push(Turn::Plan(entries));
                }
            }
            _ => {}
        }
    }
    turns
}

enum TurnKind {
    User,
    Assistant,
    Thought,
}

fn append_text(turns: &mut Vec<Turn>, kind: TurnKind, text: &str) {
    match (turns.last_mut(), kind) {
        (Some(Turn::User(s)), TurnKind::User) => s.push_str(text),
        (Some(Turn::Assistant(s)), TurnKind::Assistant) => s.push_str(text),
        (Some(Turn::Thought(s)), TurnKind::Thought) => s.push_str(text),
        (_, TurnKind::User) => turns.push(Turn::User(text.to_string())),
        (_, TurnKind::Assistant) => turns.push(Turn::Assistant(text.to_string())),
        (_, TurnKind::Thought) => turns.push(Turn::Thought(text.to_string())),
    }
}

fn chunk_text(update: &Value) -> Option<String> {
    let content = update.get("content")?;
    if let Some(t) = content.get("text").and_then(|t| t.as_str()) {
        return Some(t.to_string());
    }
    content.as_str().map(|s| s.to_string())
}

fn tool_content(update: &Value) -> Option<String> {
    let content = update.get("content")?;
    if let Some(arr) = content.as_array() {
        let mut parts = Vec::new();
        for item in arr {
            if let Some(t) = item
                .pointer("/content/text")
                .and_then(|t| t.as_str())
                .or_else(|| item.get("text").and_then(|t| t.as_str()))
            {
                parts.push(t);
            }
        }
        if !parts.is_empty() {
            return Some(parts.join("\n"));
        }
    }
    None
}

#[allow(dead_code)]
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn list_and_turns_from_fake_tree() {
        let tmp = tempfile::tempdir().unwrap();
        // point GROK_HOME at tmp
        // SAFETY: test-only env mutation
        unsafe {
            std::env::set_var("GROK_HOME", tmp.path());
        }
        let cwd = "/tmp/proj";
        let enc = urlencoding::encode(cwd);
        let sess = tmp
            .path()
            .join("sessions")
            .join(enc.as_ref())
            .join("abc-123");
        fs::create_dir_all(&sess).unwrap();
        let summary = r#"{
            "info": { "id": "abc-123", "cwd": "/tmp/proj" },
            "generated_title": "Hello",
            "updated_at": "2026-07-23T12:00:00Z"
        }"#;
        fs::write(sess.join("summary.json"), summary).unwrap();
        let mut updates = fs::File::create(sess.join("updates.jsonl")).unwrap();
        writeln!(
            updates,
            r#"{{"method":"session/update","params":{{"update":{{"sessionUpdate":"user_message_chunk","content":{{"type":"text","text":"hi"}}}}}}}}"#
        )
        .unwrap();
        writeln!(
            updates,
            r#"{{"method":"session/update","params":{{"update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"yo"}}}}}}}}"#
        )
        .unwrap();

        let list = list_for_cwd(cwd);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Hello");
        let turns = turns_from_updates_jsonl(cwd, "abc-123");
        assert!(matches!(&turns[0], Turn::User(s) if s == "hi"));
        assert!(matches!(&turns[1], Turn::Assistant(s) if s == "yo"));
        unsafe {
            std::env::remove_var("GROK_HOME");
        }
    }
}
