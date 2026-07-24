//! List and rebuild Grok sessions from `~/.grok/sessions`.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::overlay;
use crate::protocol::{PlanEntry, SessionSummary, ToolTurn, Turn};

/// Default tail window when loading a transcript for display.
/// Tool-heavy jsonl burns bytes fast; chain-load fills the pane (see App).
pub const HISTORY_TAIL_BYTES: u64 = 512 * 1024;

/// How many **display items** (after collapsing contiguous tools) we try to
/// have on first open before stopping auto-prepend of older chunks.
pub const HISTORY_INITIAL_ITEMS: usize = 48;

/// Cap auto-chain loads so a huge session cannot flood the UI thread.
pub const HISTORY_AUTO_CHUNKS_MAX: u32 = 6;

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
    let abs = PathBuf::from(cwd);
    let abs = abs.canonicalize().unwrap_or(abs);
    urlencoding::encode(&abs.to_string_lossy()).into_owned()
}

/// All sessions across every project group under `~/.grok/sessions`.
///
/// Sorted: pinned first, then live TUI sessions, then most recent activity.
pub fn list_all() -> Vec<SessionSummary> {
    let pins = overlay::load();
    let live = active_terminal_sessions();
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    let groups = match fs::read_dir(sessions_root()) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for group in groups.flatten() {
        let group_path = group.path();
        if !group_path.is_dir() {
            continue;
        }
        let group_cwd = group
            .file_name()
            .to_str()
            .and_then(|n| urlencoding::decode(n).ok())
            .map(|s| s.into_owned())
            .unwrap_or_default();
        collect_group(&group_path, &group_cwd, &pins, &live, &mut seen, &mut out);
    }

    out.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then(b.live.cmp(&a.live))
            .then(b.updated.cmp(&a.updated))
    });
    out
}

/// Back-compat: list for a cwd (+ git root). Prefer [`list_all`] in the UI.
pub fn list_for_cwd(cwd: &str) -> Vec<SessionSummary> {
    let pins = overlay::load();
    let live = active_terminal_sessions();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for root in session_roots(cwd) {
        let group = sessions_root().join(encode_cwd(&root));
        collect_group(&group, &root, &pins, &live, &mut seen, &mut out);
    }
    out.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then(b.live.cmp(&a.live))
            .then(b.updated.cmp(&a.updated))
    });
    out
}

fn session_roots(cwd: &str) -> Vec<String> {
    let mut roots = vec![cwd.to_string()];
    if let Some(git) = find_git_root(Path::new(cwd)) {
        let g = git.to_string_lossy().into_owned();
        if g != cwd {
            roots.push(g);
        }
    }
    roots
        .into_iter()
        .map(|r| {
            PathBuf::from(&r)
                .canonicalize()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or(r)
        })
        .collect()
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    if let Ok(c) = cur.canonicalize() {
        cur = c;
    }
    loop {
        if cur.join(".git").exists() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

fn collect_group(
    group_path: &Path,
    group_cwd: &str,
    pins: &overlay::Overlay,
    live: &HashSet<String>,
    seen: &mut HashSet<String>,
    out: &mut Vec<SessionSummary>,
) {
    let entries = match fs::read_dir(group_path) {
        Ok(e) => e,
        Err(_) => return,
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
        if !seen.insert(id.clone()) {
            continue;
        }
        let summary_path = path.join("summary.json");
        let (disk_title, summary_updated, cwd_s) = match read_summary(&summary_path) {
            Some(t) => t,
            None => continue,
        };
        // Activity = last turn on disk, not "last opened" (Grok bumps
        // summary.json on session/load, which made ages jump to "just now").
        let updated = activity_secs(&path).unwrap_or(summary_updated);
        let pinned = pins.pinned.contains(&id);
        let title = resolve_title(&id, &disk_title, pins);
        let cwd_display = if cwd_s.is_empty() {
            group_cwd.to_string()
        } else {
            cwd_s
        };
        out.push(SessionSummary {
            id: id.clone(),
            title,
            cwd: cwd_display,
            updated,
            pinned,
            live: live.contains(&id),
        });
    }
}

fn resolve_title(id: &str, disk_title: &str, pins: &overlay::Overlay) -> String {
    if let Some(t) = pins.title_overrides.get(id) {
        if !t.trim().is_empty() {
            return t.clone();
        }
    }
    if let Some(t) = pins.auto_titles.get(id) {
        if !t.trim().is_empty() {
            return t.clone();
        }
    }
    let t = disk_title.trim();
    if !t.is_empty() && t != "(untitled)" {
        return t.to_string();
    }
    "(untitled)".into()
}

/// Last conversational activity: mtime of `updates.jsonl` (preferred) or
/// `chat_history.jsonl`. Never uses `summary.json` mtime — Grok rewrites
/// that on mere open/load.
fn activity_secs(session_dir: &Path) -> Option<u64> {
    let candidates = ["updates.jsonl", "chat_history.jsonl"];
    let mut best: Option<u64> = None;
    for name in candidates {
        let p = session_dir.join(name);
        if let Ok(meta) = fs::metadata(&p) {
            if let Ok(modified) = meta.modified() {
                if let Ok(d) = modified.duration_since(UNIX_EPOCH) {
                    let secs = d.as_secs();
                    best = Some(best.map_or(secs, |b| b.max(secs)));
                }
            }
        }
    }
    best
}

/// Session ids currently held open by a live Grok TUI process.
pub fn active_terminal_sessions() -> HashSet<String> {
    let path = grok_home().join("active_sessions.json");
    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return HashSet::new(),
    };
    let entries: Vec<Value> = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return HashSet::new(),
    };
    let mut out = HashSet::new();
    for e in entries {
        let id = e
            .get("session_id")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        if id.is_empty() {
            continue;
        }
        let pid = e.get("pid").and_then(|p| p.as_u64()).unwrap_or(0);
        if pid > 0 && process_alive(pid as u32) {
            out.insert(id.to_string());
        }
    }
    out
}

fn process_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// Recent project roots for the new-session picker.
pub fn recent_project_cwds() -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let o = overlay::load();
    for c in o.recent_cwds.iter().chain(o.last_cwd.iter()) {
        if seen.insert(c.clone()) {
            out.push(c.clone());
        }
    }
    if let Ok(entries) = fs::read_dir(sessions_root()) {
        for e in entries.flatten() {
            let name = match e.file_name().into_string() {
                Ok(s) => s,
                Err(_) => continue,
            };
            if let Ok(decoded) = urlencoding::decode(&name) {
                let path = decoded.into_owned();
                if !path.is_empty() && seen.insert(path.clone()) {
                    out.push(path);
                }
            }
        }
    }
    out.truncate(24);
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
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp() as u64)
        .or_else(|| {
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
    let pins = overlay::load();
    if let Some(t) = pins.title_overrides.get(id) {
        return Some(t.clone());
    }
    if let Some(t) = pins.auto_titles.get(id) {
        return Some(t.clone());
    }
    let path = sessions_root()
        .join(encode_cwd(cwd))
        .join(id)
        .join("summary.json");
    read_summary(&path).map(|(t, _, _)| t)
}

/// Derive a short, human session title from **user + assistant** turns only.
/// Ignores tools, thoughts, plans, and errors.
pub fn derive_title_from_turns(turns: &[Turn]) -> Option<String> {
    let mut users: Vec<&str> = Vec::new();
    let mut assistants: Vec<&str> = Vec::new();
    for t in turns {
        match t {
            Turn::User(s) => {
                let s = s.trim();
                if !s.is_empty() {
                    users.push(s);
                }
            }
            Turn::Assistant(s) => {
                let s = s.trim();
                if !s.is_empty() {
                    assistants.push(s);
                }
            }
            _ => {}
        }
    }
    if users.is_empty() && assistants.is_empty() {
        return None;
    }

    // Evolving title: early sessions use the first ask; longer ones prefer
    // the latest user intent so the label tracks what the work became.
    let seed = if users.len() >= 3 {
        users.last().copied()
    } else {
        users.first().copied()
    }
    .or_else(|| assistants.first().copied())?;

    let mut title = clean_title_seed(seed);
    if title.is_empty() {
        return None;
    }

    // If we only have a short user ask, optionally append a hint from the
    // first assistant sentence (still user/assistant only).
    if users.len() == 1 && title.chars().count() < 28 {
        if let Some(a) = assistants.first() {
            let hint = clean_title_seed(a);
            if !hint.is_empty() && !title.eq_ignore_ascii_case(&hint) {
                let combined = format!("{title} · {hint}");
                title = ellipsize(&combined, 72);
            }
        }
    }

    Some(ellipsize(&title, 72))
}

fn clean_title_seed(s: &str) -> String {
    // First non-empty line, strip markdown heading markers / list bullets.
    let line = s
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .trim_start_matches('#')
        .trim_start_matches(['-', '*', '>', ' '])
        .trim();
    // Collapse whitespace.
    let collapsed: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
}

fn ellipsize(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let take = max_chars.saturating_sub(1);
    let t: String = s.chars().take(take).collect();
    format!("{t}…")
}

/// If the user has not manually renamed this session, store a derived title.
pub fn maybe_update_auto_title(id: &str, turns: &[Turn]) {
    if overlay::title_override(id).is_some() {
        return;
    }
    if let Some(title) = derive_title_from_turns(turns) {
        overlay::set_auto_title(id, &title);
    }
}

/// Result of a lazy history window over `updates.jsonl`.
#[derive(Debug, Clone)]
pub struct HistorySlice {
    pub turns: Vec<Turn>,
    /// Absolute file byte where the first complete line of this slice began.
    pub start_byte: u64,
    pub has_older: bool,
}

/// Rebuild display turns from Grok `updates.jsonl` for a session (full file).
pub fn turns_from_updates_jsonl(cwd: &str, id: &str) -> Vec<Turn> {
    history_tail(cwd, id).turns
}

/// Load the **tail** of a session transcript for first paint.
pub fn history_tail(cwd: &str, id: &str) -> HistorySlice {
    history_window(cwd, id, None, HISTORY_TAIL_BYTES)
}

/// Load a window of history ending at `before_byte` (exclusive).
pub fn history_before(cwd: &str, id: &str, before_byte: u64) -> HistorySlice {
    history_window(cwd, id, Some(before_byte), HISTORY_TAIL_BYTES)
}

fn updates_path(cwd: &str, id: &str) -> PathBuf {
    sessions_root()
        .join(encode_cwd(cwd))
        .join(id)
        .join("updates.jsonl")
}

fn history_window(
    cwd: &str,
    id: &str,
    end_exclusive: Option<u64>,
    max_bytes: u64,
) -> HistorySlice {
    let path = updates_path(cwd, id);
    let mut file = match File::open(&path) {
        Ok(f) => f,
        Err(_) => {
            return HistorySlice {
                turns: Vec::new(),
                start_byte: 0,
                has_older: false,
            };
        }
    };
    let file_len = file.seek(SeekFrom::End(0)).unwrap_or(0);
    let end = end_exclusive.unwrap_or(file_len).min(file_len);
    if end == 0 {
        return HistorySlice {
            turns: Vec::new(),
            start_byte: 0,
            has_older: false,
        };
    }
    let start_read = end.saturating_sub(max_bytes);
    if file.seek(SeekFrom::Start(start_read)).is_err() {
        return HistorySlice {
            turns: Vec::new(),
            start_byte: 0,
            has_older: false,
        };
    }
    let mut buf = vec![0u8; (end - start_read) as usize];
    let n = file.read(&mut buf).unwrap_or(0);
    buf.truncate(n);

    let (start_byte, text) = if start_read > 0 {
        match buf.iter().position(|&b| b == b'\n') {
            Some(i) if i + 1 < buf.len() => {
                let start = start_read + i as u64 + 1;
                (start, String::from_utf8_lossy(&buf[i + 1..]).into_owned())
            }
            _ => {
                return HistorySlice {
                    turns: Vec::new(),
                    start_byte: end,
                    has_older: start_read > 0,
                };
            }
        }
    } else {
        (0u64, String::from_utf8_lossy(&buf).into_owned())
    };

    let mut turns = parse_updates_text(&text);
    // History is past: never leave tools stuck on pending/in_progress/running
    // (window edges and backgrounded tasks often omit a terminal update).
    finalize_tool_statuses(&mut turns);
    HistorySlice {
        turns,
        start_byte,
        has_older: start_byte > 0,
    }
}

/// Count UI rows after collapsing contiguous tool uses (matches bubble layout).
pub fn display_item_count(turns: &[Turn]) -> usize {
    let mut n = 0usize;
    let mut i = 0;
    while i < turns.len() {
        if matches!(&turns[i], Turn::Tool(_)) {
            while i < turns.len() && matches!(&turns[i], Turn::Tool(_)) {
                i += 1;
            }
            n += 1;
        } else {
            n += 1;
            i += 1;
        }
    }
    n
}

/// Mark non-terminal tools as completed (history / end-of-turn).
pub fn finalize_tool_statuses(turns: &mut [Turn]) {
    for t in turns.iter_mut() {
        if let Turn::Tool(tt) = t {
            if !is_terminal_tool_status(&tt.status) {
                tt.status = "completed".into();
            }
        }
    }
}

pub fn is_terminal_tool_status(status: &str) -> bool {
    let s = status.to_ascii_lowercase();
    s.contains("complet")
        || s == "success"
        || s == "ok"
        || s.contains("fail")
        || s.contains("error")
        || s.contains("cancel")
}

fn parse_updates_text(raw: &str) -> Vec<Turn> {
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
                // Metadata only — transcript collapses tools; skip rawInput/output.
                let idx = turns.len();
                turns.push(Turn::Tool(ToolTurn {
                    call_id: call_id.clone(),
                    tool,
                    status: "pending".into(),
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
        let all = list_all();
        assert_eq!(all.len(), 1);
        let title = derive_title_from_turns(&turns).unwrap();
        assert!(title.to_lowercase().contains("hi") || title.contains("yo"));
        unsafe {
            std::env::remove_var("GROK_HOME");
        }
    }

    #[test]
    fn derive_ignores_tools() {
        let turns = vec![
            Turn::User("Fix the login bug".into()),
            Turn::Tool(ToolTurn {
                call_id: "1".into(),
                tool: "bash".into(),
                status: "ok".into(),
            }),
            Turn::Assistant("Patched auth middleware.".into()),
        ];
        let t = derive_title_from_turns(&turns).unwrap();
        assert!(!t.contains("bash"));
        assert!(!t.contains("noise"));
        assert!(t.to_lowercase().contains("login") || t.to_lowercase().contains("fix"));
    }

    #[test]
    fn finalize_tools_marks_stuck_running_done() {
        let mut turns = vec![
            Turn::Tool(ToolTurn {
                call_id: "a".into(),
                tool: "read".into(),
                status: "in_progress".into(),
            }),
            Turn::Tool(ToolTurn {
                call_id: "b".into(),
                tool: "bash".into(),
                status: "Pending".into(),
            }),
            Turn::Tool(ToolTurn {
                call_id: "c".into(),
                tool: "x".into(),
                status: "failed".into(),
            }),
        ];
        finalize_tool_statuses(&mut turns);
        assert_eq!(
            matches!(&turns[0], Turn::Tool(t) if t.status == "completed"),
            true
        );
        assert_eq!(
            matches!(&turns[1], Turn::Tool(t) if t.status == "completed"),
            true
        );
        assert_eq!(
            matches!(&turns[2], Turn::Tool(t) if t.status == "failed"),
            true
        );
    }

    #[test]
    fn display_items_collapse_tools() {
        let turns = vec![
            Turn::User("hi".into()),
            Turn::Tool(ToolTurn {
                call_id: "1".into(),
                tool: "a".into(),
                status: "completed".into(),
            }),
            Turn::Tool(ToolTurn {
                call_id: "2".into(),
                tool: "b".into(),
                status: "completed".into(),
            }),
            Turn::Assistant("ok".into()),
        ];
        assert_eq!(display_item_count(&turns), 3);
    }
}
