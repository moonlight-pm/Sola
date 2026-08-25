//! Grok hook events → status. Grok is the first-class mapping.

use serde_json::Value;

use crate::status::AgentStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedHook {
    pub status: Option<AgentStatus>,
    pub clear_turn: bool,
    /// Lead events that may take the pane from a previous session
    /// (`session_start`, `user_prompt_submit`). Grok does not fire
    /// those for a subagent's own session.
    pub claim: bool,
    pub session_end: bool,
    pub compacted: bool,
    pub prompt: Option<String>,
    pub tool: Option<String>,
    pub session_id: Option<String>,
}

/// Map a Grok stdin JSON envelope. Unknown / child-only events return `None`.
pub fn map_grok(payload: &Value) -> Option<MappedHook> {
    let event = event_name(payload);
    if event.is_empty() {
        return None;
    }
    // Child CLIs inherit SOLA_PANE_ID; their stop must not mark the parent.
    if matches!(
        event.as_str(),
        "subagent_start" | "subagent_stop" | "subagent_end"
    ) {
        return None;
    }
    // SessionEnd / Stop / tool events from a child session carry this.
    if string_field(payload, &["subagentType", "subagent_type"]).is_some() {
        return None;
    }

    let session_id = string_field(payload, &["sessionId", "session_id"]);
    let prompt = string_field(payload, &["prompt", "userPrompt", "user_prompt"]);
    let tool = string_field(payload, &["toolName", "tool_name", "name"]);

    if event == "session_start" {
        return Some(MappedHook {
            status: None,
            clear_turn: true,
            claim: true,
            session_end: false,
            compacted: false,
            prompt: None,
            tool: None,
            session_id,
        });
    }
    if event == "post_compact" {
        return Some(MappedHook {
            status: None,
            clear_turn: false,
            claim: false,
            session_end: false,
            compacted: true,
            prompt: None,
            tool: None,
            session_id,
        });
    }

    let status = if matches!(
        event.as_str(),
        "user_prompt_submit" | "post_tool_use" | "post_tool_use_failure"
    ) {
        Some(AgentStatus::Working)
    } else if event == "pre_tool_use" {
        if is_ask_user(tool.as_deref()) {
            Some(AgentStatus::Waiting)
        } else {
            Some(AgentStatus::Working)
        }
    } else if matches!(
        event.as_str(),
        "stop" | "session_end" | "stop_failure" | "stop_cancelled"
    ) {
        Some(AgentStatus::Done)
    } else if event == "notification" {
        map_notification(payload)
    } else {
        None
    };

    status.map(|status| MappedHook {
        status: Some(status),
        clear_turn: false,
        claim: event == "user_prompt_submit",
        session_end: event == "session_end",
        compacted: false,
        prompt: if event == "notification" {
            None
        } else {
            prompt
        },
        tool,
        session_id,
    })
}

fn map_notification(payload: &Value) -> Option<AgentStatus> {
    let ntype = string_field(payload, &["notificationType", "notification_type", "type"])
        .unwrap_or_default();
    let message = string_field(payload, &["message"]).unwrap_or_default();
    let level = string_field(payload, &["level"]).unwrap_or_default();
    let ntype_n = normalize_event(&ntype);
    let msg = message.to_ascii_lowercase();

    if ntype_n == "permission_prompt"
        && msg.trim() == "tool permission requested"
        && (level.is_empty() || level.eq_ignore_ascii_case("info"))
    {
        return None;
    }
    if ntype_n == "idle_prompt" || msg.contains("type your message") || msg.contains("enter send") {
        return Some(AgentStatus::Done);
    }
    if ntype_n == "permission_prompt"
        || msg.contains("permission")
        || msg.contains("approval")
        || msg.contains("needs your")
        || msg.contains("question")
    {
        return Some(AgentStatus::Waiting);
    }
    None
}

pub fn event_name(payload: &Value) -> String {
    string_field(payload, &["hookEventName", "hook_event_name"])
        .map(|s| normalize_event(&s))
        .unwrap_or_default()
}

pub fn normalize_event(name: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = name.trim().chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if (*c == '-' || c.is_whitespace()) && !out.ends_with('_') {
            out.push('_');
            continue;
        }
        if c.is_uppercase() && i > 0 && chars[i - 1].is_lowercase() && !out.ends_with('_') {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

fn is_ask_user(tool: Option<&str>) -> bool {
    let Some(name) = tool else {
        return false;
    };
    let norm: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    norm == "askuserquestion" || norm == "requestuserinput"
}

fn string_field(payload: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(s) = payload.get(*key).and_then(|v| v.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(v: Value) -> Option<MappedHook> {
        map_grok(&v)
    }

    #[test]
    fn grok_events_map_first() {
        assert!(
            map(json!({"hookEventName": "PostCompact"}))
                .unwrap()
                .compacted
        );
        assert_eq!(
            map(json!({"hookEventName": "UserPromptSubmit"}))
                .unwrap()
                .status,
            Some(AgentStatus::Working)
        );
        assert_eq!(
            map(json!({"hookEventName": "PreToolUse", "toolName": "read_file"}))
                .unwrap()
                .status,
            Some(AgentStatus::Working)
        );
        assert_eq!(
            map(json!({"hookEventName": "PreToolUse", "toolName": "ask_user_question"}))
                .unwrap()
                .status,
            Some(AgentStatus::Waiting)
        );
        assert_eq!(
            map(json!({"hookEventName": "Stop"})).unwrap().status,
            Some(AgentStatus::Done)
        );
        assert_eq!(
            map(json!({"hookEventName": "StopFailure"})).unwrap().status,
            Some(AgentStatus::Done)
        );
        assert!(
            map(json!({"hookEventName": "SessionStart"}))
                .unwrap()
                .clear_turn
        );
        assert!(map(json!({"hookEventName": "SessionStart"})).unwrap().claim);
        assert!(
            map(json!({"hookEventName": "SessionStart"}))
                .unwrap()
                .status
                .is_none()
        );
        assert!(
            map(json!({"hookEventName": "UserPromptSubmit"}))
                .unwrap()
                .claim
        );
        assert_eq!(
            map(json!({"hookEventName": "StopCancelled"}))
                .unwrap()
                .status,
            Some(AgentStatus::Done)
        );
    }

    #[test]
    fn child_subagent_does_not_map() {
        assert!(map(json!({"hookEventName": "SubagentStop"})).is_none());
        assert!(map(json!({"hookEventName": "subagent_start"})).is_none());
        assert!(
            map(json!({
                "hookEventName": "SessionEnd",
                "sessionId": "child",
                "subagentType": "explore"
            }))
            .is_none()
        );
        assert!(
            map(json!({
                "hookEventName": "Stop",
                "sessionId": "child",
                "subagentType": "explore"
            }))
            .is_none()
        );
    }

    #[test]
    fn routine_permission_prompt_ignored() {
        assert!(
            map(json!({
                "hookEventName": "Notification",
                "notificationType": "permission_prompt",
                "message": "Tool permission requested",
                "level": "info"
            }))
            .is_none()
        );
    }

    #[test]
    fn idle_notification_is_done() {
        assert_eq!(
            map(json!({
                "hookEventName": "Notification",
                "notificationType": "idle_prompt",
                "message": "Type your message"
            }))
            .unwrap()
            .status,
            Some(AgentStatus::Done)
        );
    }

    #[test]
    fn snake_and_camel_event_names() {
        assert_eq!(normalize_event("StopFailure"), "stop_failure");
        assert_eq!(normalize_event("pre_tool_use"), "pre_tool_use");
        assert_eq!(normalize_event("UserPromptSubmit"), "user_prompt_submit");
    }
}
