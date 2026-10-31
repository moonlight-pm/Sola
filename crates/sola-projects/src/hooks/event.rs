//! A hook event: what an agent's hook said, read off its computer's hook
//! log ([`super::log`]), and which session it lands on.

use std::path::Path;

use serde_json::Value;

use super::map::{self, MappedHook};

#[derive(Debug, Clone)]
pub struct Incoming {
    /// The computer it came from (whose hook log it was read off);
    /// `None`: this one.
    pub host: Option<String>,
    pub pane_id: String,
    pub agent: String,
    pub mapped: MappedHook,
    /// Session cwd from the hook payload, when the CLI sent one.
    /// Codex's app-server daemon inherits whichever pane started it, so
    /// its `SOLA_PANE_ID` can name the wrong session. Callers retarget
    /// when this path disagrees with that pane.
    pub cwd: Option<String>,
    /// What the hook says the session asks (its island's title and body).
    pub ask: Option<crate::islands::Ask>,
    /// Claude's transcript (a finished turn's last message).
    pub transcript: Option<String>,
}

/// The event in hook payload `body`: `pane_header` is the hook's
/// `SOLA_PANE_ID` (else the payload's `paneId`), `path_agent` the script's
/// agent. `Ok(None)`: nothing to place or map.
pub fn from_json(
    pane_header: Option<String>,
    path_agent: Option<String>,
    body: &[u8],
) -> std::io::Result<Option<Incoming>> {
    if body.is_empty() {
        return Ok(None);
    }
    let value: Value = serde_json::from_slice(body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let pane_id = pane_header
        .or_else(|| {
            value
                .get("paneId")
                .or_else(|| value.get("pane_id"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    let event = if value.get("hookEventName").is_some() || value.get("hook_event_name").is_some() {
        value
    } else {
        value.get("payload").cloned().unwrap_or(value)
    };
    let cwd = string_cwd(&event);
    if pane_id.is_empty() && cwd.is_none() {
        return Ok(None);
    }
    let agent = path_agent
        .or_else(|| {
            event
                .get("agent")
                .or_else(|| event.get("agentType"))
                .or_else(|| event.get("agent_type"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_ascii_lowercase())
        })
        .filter(|s| crate::cli::is_first_class(s))
        .unwrap_or_else(|| "grok".into());
    let Some(mapped) = map::map_agent(&agent, &event) else {
        return Ok(None);
    };
    Ok(Some(Incoming {
        host: None,
        pane_id,
        agent,
        mapped,
        cwd,
        ask: crate::islands::ask_from_hook(&event),
        transcript: crate::islands::transcript_of(&event),
    }))
}

fn string_cwd(event: &Value) -> Option<String> {
    event
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Where a hook should land.
///
/// Keep the header pane when its tab path is the event cwd. Otherwise
/// pick the leaf on that path, preferring the one already named `agent` or
/// already bound to the event session.
pub fn resolve_hook_target(
    header_pane: Option<&str>,
    header_path: Option<&Path>,
    event_cwd: Option<&Path>,
    event_session: Option<&str>,
    agent: &str,
    leaves: &[HookLeaf<'_>],
) -> Option<String> {
    let event_cwd = event_cwd.filter(|p| !p.as_os_str().is_empty());
    let header_matches = header_pane.filter(|id| !id.is_empty()).and_then(|id| {
        let path = header_path?;
        let cwd = event_cwd?;
        paths_same(path, cwd).then(|| id.to_string())
    });
    if header_matches.is_some() {
        return header_matches;
    }
    let Some(cwd) = event_cwd else {
        return header_pane.filter(|id| !id.is_empty()).map(str::to_string);
    };
    let matches: Vec<&HookLeaf<'_>> = leaves
        .iter()
        .filter(|leaf| paths_same(leaf.path, cwd))
        .collect();
    if matches.is_empty() {
        return header_pane.filter(|id| !id.is_empty()).map(str::to_string);
    }
    if let Some(sid) = event_session.filter(|s| !s.is_empty()) {
        if let Some(leaf) = matches.iter().find(|leaf| leaf.session_id == Some(sid)) {
            return Some(leaf.id.to_string());
        }
    }
    if let Some(leaf) = matches.iter().find(|leaf| leaf.agent == Some(agent)) {
        return Some(leaf.id.to_string());
    }
    if matches.len() == 1 {
        return Some(matches[0].id.to_string());
    }
    header_pane
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .or_else(|| Some(matches[0].id.to_string()))
}

fn paths_same(a: &Path, b: &Path) -> bool {
    a.components().eq(b.components())
}

/// One tab leaf the hook resolver can see.
pub struct HookLeaf<'a> {
    pub id: &'a str,
    pub path: &'a Path,
    pub agent: Option<&'a str>,
    pub session_id: Option<&'a str>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_grok_event() {
        let body = br#"{"hookEventName":"UserPromptSubmit","prompt":"hi"}"#;
        let got = from_json(Some("pane-1".into()), Some("grok".into()), body)
            .unwrap()
            .unwrap();
        assert_eq!(got.pane_id, "pane-1");
        assert_eq!(got.agent, "grok");
        assert_eq!(got.mapped.status, Some(crate::status::AgentStatus::Working));
        assert_eq!(got.mapped.prompt.as_deref(), Some("hi"));
    }

    #[test]
    fn parses_a_claude_event() {
        let body = br#"{"hook_event_name":"Stop","session_id":"sid","cwd":"/src/app"}"#;
        let got = from_json(Some("pane-k".into()), Some("claude".into()), body)
            .unwrap()
            .unwrap();
        assert_eq!(got.pane_id, "pane-k");
        assert_eq!(got.agent, "claude");
        assert_eq!(got.mapped.status, Some(crate::status::AgentStatus::Done));
        assert_eq!(got.cwd.as_deref(), Some("/src/app"));
    }

    #[test]
    fn parses_a_codex_event() {
        let body = br#"{"hook_event_name":"PermissionRequest"}"#;
        let got = from_json(Some("pane-c".into()), Some("codex".into()), body)
            .unwrap()
            .unwrap();
        assert_eq!(got.pane_id, "pane-c");
        assert_eq!(got.agent, "codex");
        assert_eq!(got.mapped.status, Some(crate::status::AgentStatus::Waiting));
    }

    #[test]
    fn parses_codex_cwd_without_pane_header() {
        let body =
            br#"{"hookEventName":"UserPromptSubmit","cwd":"/src/rimworld","sessionId":"s1"}"#;
        let got = from_json(None, Some("codex".into()), body).unwrap().unwrap();
        assert!(got.pane_id.is_empty());
        assert_eq!(got.cwd.as_deref(), Some("/src/rimworld"));
        assert_eq!(got.agent, "codex");
    }

    #[test]
    fn retargets_codex_hook_when_header_cwd_disagrees() {
        let harness = std::path::Path::new("/src/harness");
        let rim = std::path::Path::new("/src/rimworld");
        let leaves = [
            HookLeaf {
                id: "ws-harness",
                path: harness,
                agent: Some("grok"),
                session_id: Some("g-sid"),
            },
            HookLeaf {
                id: "ws-main-11",
                path: rim,
                agent: Some("codex"),
                session_id: None,
            },
            HookLeaf {
                id: "ws-main-11-p",
                path: rim,
                agent: None,
                session_id: None,
            },
        ];
        let got = resolve_hook_target(
            Some("ws-harness"),
            Some(harness),
            Some(rim),
            Some("s1"),
            "codex",
            &leaves,
        );
        assert_eq!(got.as_deref(), Some("ws-main-11"));
    }

    #[test]
    fn keeps_header_when_cwd_matches() {
        let rim = std::path::Path::new("/src/rimworld");
        let leaves = [HookLeaf {
            id: "ws-main-11",
            path: rim,
            agent: Some("codex"),
            session_id: None,
        }];
        let got = resolve_hook_target(
            Some("ws-main-11"),
            Some(rim),
            Some(rim),
            None,
            "codex",
            &leaves,
        );
        assert_eq!(got.as_deref(), Some("ws-main-11"));
    }

    #[test]
    fn parses_bare_json_envelope() {
        let raw = br#"{"paneId":"p2","hookEventName":"Stop"}"#;
        let got = from_json(None, None, raw).unwrap().unwrap();
        assert_eq!(got.pane_id, "p2");
        assert_eq!(got.agent, "grok");
        assert_eq!(got.mapped.status, Some(crate::status::AgentStatus::Done));
    }
}
