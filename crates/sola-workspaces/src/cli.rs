//! Call-plane helpers: payloads, targeting, prompt, wait status.
//!
//! Kept off the iced `App` so the contract is unit-testable. Freeze:
//! `docs/specs/2026-08-18-workspaces-cli-design.md`.

use crate::calls::WAIT_DEFAULT_SECS;
use crate::status::AgentStatus;
use crate::workspace::{Kind, Project, Workspace};

pub fn project_json(p: &Project) -> serde_json::Value {
    serde_json::json!({
        "id": p.id,
        "name": p.name,
        "root": p.root,
        "startup": !p.startup.is_empty(),
    })
}

pub fn workspace_json(w: &Workspace, selected: Option<&str>) -> serde_json::Value {
    let title = display_name(w);
    let mut v = serde_json::json!({
        "id": w.id,
        "name": title,
        "title": w.title,
        "path": w.path,
        "kind": kind_str(w.kind),
        "parent": w.parent,
        "status": status_str(w.status),
        "agent": w.agent,
        "project": w.project_id,
    });
    if let Some(sel) = selected {
        v["selected"] = serde_json::json!(w.id == sel);
    }
    v
}

pub fn pane_json(id: &str, status: AgentStatus, agent: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "status": status_str(status),
        "agent": agent,
    })
}

pub fn spawn_json(w: &Workspace, selected: bool) -> serde_json::Value {
    serde_json::json!({
        "id": w.id,
        "name": w.name,
        "title": w.title,
        "path": w.path,
        "kind": kind_str(w.kind),
        "parent": w.parent,
        "project": w.project_id,
        "selected": selected,
    })
}

pub fn display_name(w: &Workspace) -> &str {
    if w.kind == Kind::Main {
        "root"
    } else {
        w.name.as_str()
    }
}

/// Rail label: `root`, `sc-1234`, or `sc-1234 · short title`.
pub fn rail_label(w: &Workspace) -> String {
    if w.kind == Kind::Main {
        return "root".into();
    }
    match w.title.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) => format!("{} · {t}", w.name),
        None => w.name.clone(),
    }
}

pub fn kind_str(kind: Kind) -> &'static str {
    match kind {
        Kind::Main => "main",
        Kind::Worktree => "worktree",
        Kind::Folder => "folder",
    }
}

pub fn status_str(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Working => "working",
        AgentStatus::Waiting => "waiting",
        AgentStatus::Done => "done",
        AgentStatus::Idle => "idle",
    }
}

pub fn parse_status(raw: &str) -> Result<AgentStatus, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "working" => Ok(AgentStatus::Working),
        "waiting" => Ok(AgentStatus::Waiting),
        "done" => Ok(AgentStatus::Done),
        "idle" => Ok(AgentStatus::Idle),
        other => Err(format!("unknown status '{other}'")),
    }
}

pub fn wait_timeout_secs(raw: Option<u64>) -> u64 {
    match raw {
        Some(0) | None => WAIT_DEFAULT_SECS,
        Some(n) => n.min(3_600),
    }
}

/// Explicit pane id wins. A workspace leaf list prefers Grok, else `active`.
pub fn prefer_grok_pane(
    leaves: &[String],
    agents: &[(String, Option<String>)],
    active: &str,
    explicit: Option<&str>,
) -> String {
    if let Some(id) = explicit {
        if leaves.iter().any(|p| p == id) {
            return id.to_string();
        }
    }
    if let Some((id, _)) = agents.iter().find(|(_, a)| {
        a.as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case("grok"))
    }) {
        if leaves.iter().any(|p| p == id) {
            return id.clone();
        }
    }
    if leaves.iter().any(|p| p == active) {
        return active.to_string();
    }
    leaves
        .first()
        .cloned()
        .unwrap_or_else(|| active.to_string())
}

pub fn read_prompt(
    prompt: Option<&str>,
    prompt_file: Option<&str>,
) -> Result<Option<String>, String> {
    match (prompt, prompt_file) {
        (Some(_), Some(_)) => Err("pass --prompt or --prompt-file, not both".into()),
        (Some(p), None) => {
            let t = p.trim();
            if t.is_empty() {
                Ok(None)
            } else {
                Ok(Some(t.to_string()))
            }
        }
        (None, Some(path)) => {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("prompt-file {path}: {e}"))?;
            let t = text.trim_end();
            if t.is_empty() {
                Ok(None)
            } else {
                Ok(Some(t.to_string()))
            }
        }
        (None, None) => Ok(None),
    }
}

pub fn grok_argv(prompt: Option<&str>) -> Vec<String> {
    let mut args = vec!["grok".to_string()];
    if let Some(p) = prompt {
        let p = p.trim();
        if !p.is_empty() {
            args.push(p.to_string());
        }
    }
    args
}

pub fn grok_shell_line(prompt: Option<&str>) -> String {
    match prompt {
        Some(p) if !p.trim().is_empty() => format!("grok {}", shell_single_quote(p)),
        _ => "grok".into(),
    }
}

pub fn shell_single_quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

pub fn only_grok(agent: Option<&str>) -> Result<Option<&str>, String> {
    match agent {
        None => Ok(None),
        Some("grok") => Ok(Some("grok")),
        Some(other) => Err(format!(
            "only grok is first-class; other agents are presence-only (got {other})"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ws(id: &str, name: &str, kind: Kind) -> Workspace {
        Workspace {
            id: id.into(),
            project_id: "proj".into(),
            name: name.into(),
            title: None,
            path: PathBuf::from("/r"),
            kind,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        }
    }

    #[test]
    fn rail_label_joins_title() {
        let mut w = ws("ws-kid", "sc-1234", Kind::Worktree);
        assert_eq!(rail_label(&w), "sc-1234");
        w.title = Some("fix login".into());
        assert_eq!(rail_label(&w), "sc-1234 · fix login");
    }

    #[test]
    fn main_display_name_is_root() {
        let w = ws("ws-main", "Sola", Kind::Main);
        assert_eq!(display_name(&w), "root");
        assert_eq!(workspace_json(&w, Some("ws-main"))["name"], "root");
        assert_eq!(workspace_json(&w, Some("ws-main"))["selected"], true);
        assert_eq!(workspace_json(&w, Some("ws-main"))["kind"], "main");
    }

    #[test]
    fn prefer_explicit_pane_over_grok() {
        let leaves = vec!["a".into(), "b".into()];
        let agents = vec![
            ("a".into(), Some("shell".into())),
            ("b".into(), Some("grok".into())),
        ];
        assert_eq!(prefer_grok_pane(&leaves, &agents, "a", Some("a")), "a");
        assert_eq!(prefer_grok_pane(&leaves, &agents, "a", None), "b");
    }

    #[test]
    fn prefer_active_when_no_grok() {
        let leaves = vec!["a".into(), "b".into()];
        let agents = vec![("a".into(), Some("shell".into())), ("b".into(), None)];
        assert_eq!(prefer_grok_pane(&leaves, &agents, "b", None), "b");
    }

    #[test]
    fn prompt_xor_file() {
        assert!(read_prompt(Some("hi"), Some("/tmp/x")).is_err());
        assert_eq!(
            read_prompt(Some("  hi  "), None).unwrap(),
            Some("hi".into())
        );
        assert_eq!(read_prompt(Some("   "), None).unwrap(), None);
    }

    #[test]
    fn prompt_file_contents() {
        let dir = std::env::temp_dir().join(format!("sola-ws-prompt-{}", std::process::id()));
        std::fs::write(&dir, "brief me\n").unwrap();
        let got = read_prompt(None, Some(dir.to_str().unwrap())).unwrap();
        assert_eq!(got.as_deref(), Some("brief me"));
        std::fs::remove_file(&dir).ok();
    }

    #[test]
    fn grok_quote_embeds_apostrophe() {
        assert_eq!(grok_shell_line(None), "grok");
        assert_eq!(
            grok_shell_line(Some("it's a ticket")),
            "grok 'it'\\''s a ticket'"
        );
        assert_eq!(grok_argv(Some("go")), vec!["grok", "go"]);
    }

    #[test]
    fn wait_status_and_timeout() {
        assert_eq!(parse_status("Done").unwrap(), AgentStatus::Done);
        assert!(parse_status("nope").is_err());
        assert_eq!(wait_timeout_secs(None), WAIT_DEFAULT_SECS);
        assert_eq!(wait_timeout_secs(Some(0)), WAIT_DEFAULT_SECS);
        assert_eq!(wait_timeout_secs(Some(12)), 12);
        assert_eq!(wait_timeout_secs(Some(9_999)), 3_600);
    }

    #[test]
    fn spawn_json_reports_selected() {
        let w = ws("ws-kid", "bg-test", Kind::Worktree);
        assert_eq!(spawn_json(&w, false)["selected"], false);
        assert_eq!(spawn_json(&w, true)["selected"], true);
        assert_eq!(spawn_json(&w, false)["name"], "bg-test");
    }

    #[test]
    fn only_grok_rejects_claude() {
        assert!(only_grok(Some("claude")).is_err());
        assert_eq!(only_grok(Some("grok")).unwrap(), Some("grok"));
        assert_eq!(only_grok(None).unwrap(), None);
    }
}
