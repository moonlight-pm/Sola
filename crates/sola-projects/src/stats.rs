//! A session's figures for the bar under its title (Joshua, 2026-10-31:
//! "a standard bar at the top of the projects window can show some basic
//! stats, turns, compactions, context size"): read from the agent's own
//! files, beside the compaction count ([`crate::status`]).
//!
//! Claude: a turn is a prompt — every line of one carries its `promptId`;
//! the context is the last assistant message's input (fresh, cache read,
//! cache written), and its `model`. Codex: a turn is a `user_message`; the
//! context and the window are the last `token_count`, the model the last
//! `turn_context`. Grok's files say neither: its bar shows compactions.

use std::collections::HashSet;

use serde_json::Value;

/// How much of a transcript's end is read for its last message.
pub const TAIL_BYTES: u64 = 512 * 1024;

/// What the bar shows besides compactions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub turns: u32,
    /// Tokens the last request sent: what the conversation holds now.
    pub context: Option<u64>,
    /// The model's window, when its files say (Codex).
    pub window: Option<u64>,
    pub model: Option<String>,
}

fn line_str(head: &[u8]) -> &str {
    let end = head
        .iter()
        .position(|&b| b == b'\n' || b == b'\r')
        .unwrap_or(head.len());
    std::str::from_utf8(&head[..end]).unwrap_or("")
}

/// The value of `"key":"…"` in a line's head.
fn field<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    let at = s.find(&format!("\"{key}\":\""))? + key.len() + 4;
    let rest = &s[at..];
    rest.find('"').map(|end| &rest[..end])
}

/// Claude's turns: the prompts of the main conversation (a subagent's
/// lines are `isSidechain`). A transcript from before `promptId` counts
/// its typed prompts (string content).
pub fn claude_turns(heads: &[Vec<u8>]) -> u32 {
    let mut prompts: HashSet<&str> = HashSet::new();
    let mut typed = 0u32;
    for head in heads {
        let s = line_str(head);
        if !s.contains(r#""type":"user""#) || s.contains(r#""isSidechain":true"#) {
            continue;
        }
        if let Some(id) = field(s, "promptId") {
            prompts.insert(id);
        } else if s.contains(r#""content":""#)
            && !s.contains(r#""isMeta":true"#)
            && !s.contains(r#""isCompactSummary":true"#)
        {
            typed += 1;
        }
    }
    if prompts.is_empty() { typed } else { prompts.len() as u32 }
}

/// The context and model of the last assistant message in a Claude
/// transcript's tail. A synthetic one (an error the CLI wrote) says
/// nothing.
pub fn claude_tail(tail: &[u8]) -> (Option<u64>, Option<String>) {
    for line in tail.split(|&b| b == b'\n').rev() {
        if !contains(line, br#""type":"assistant""#) || contains(line, br#""isSidechain":true"#) {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        let message = &v["message"];
        let model = message["model"].as_str().filter(|m| !m.starts_with('<'));
        let usage = &message["usage"];
        let Some(model) = model else { continue };
        let tokens = ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"]
            .iter()
            .map(|k| usage[k].as_u64().unwrap_or(0))
            .sum::<u64>();
        return ((tokens > 0).then_some(tokens), Some(model.to_string()));
    }
    (None, None)
}

/// Codex's turns: the prompts the user sent.
pub fn codex_turns(heads: &[Vec<u8>]) -> u32 {
    heads
        .iter()
        .map(|h| line_str(h))
        .filter(|s| s.contains(r#""type":"event_msg""#) && s.contains(r#""type":"user_message""#))
        .count() as u32
}

/// The context, window and model a Codex rollout's tail last recorded.
pub fn codex_tail(tail: &[u8]) -> (Option<u64>, Option<u64>, Option<String>) {
    let (mut context, mut window, mut model) = (None, None, None);
    for line in tail.split(|&b| b == b'\n').rev() {
        if context.is_none() && contains(line, br#""token_count""#) {
            if let Ok(v) = serde_json::from_slice::<Value>(line) {
                let info = &v["payload"]["info"];
                context = info["last_token_usage"]["total_tokens"].as_u64().filter(|n| *n > 0);
                window = info["model_context_window"].as_u64();
            }
        }
        if model.is_none() && contains(line, br#""turn_context""#) {
            if let Ok(v) = serde_json::from_slice::<Value>(line) {
                model = v["payload"]["model"].as_str().map(str::to_string);
            }
        }
        if context.is_some() && model.is_some() {
            break;
        }
    }
    (context, window, model)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// `325k`, `1.2M`: a token count as the bar shows it.
pub fn short_tokens(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{}k", (n + 500) / 1_000),
        _ => {
            let tenths = (n + 50_000) / 100_000;
            if tenths % 10 == 0 {
                format!("{}M", tenths / 10)
            } else {
                format!("{}.{}M", tenths / 10, tenths % 10)
            }
        }
    }
}

/// A model id as a person reads it: `claude-opus-5-5` → `Opus 5.5`,
/// `claude-haiku-4-5-20251001` → `Haiku 4.5`; anything else as it is.
pub fn model_label(model: &str) -> String {
    let Some(rest) = model.strip_prefix("claude-") else {
        return model.to_string();
    };
    let parts: Vec<&str> = rest.split('-').collect();
    let (Some(family), numbers) = (parts.first(), &parts[1..]) else {
        return model.to_string();
    };
    let version: Vec<&str> = numbers
        .iter()
        .copied()
        .take_while(|p| p.len() <= 2 && p.chars().all(|c| c.is_ascii_digit()))
        .collect();
    let mut name = family.to_string();
    if let Some(first) = name.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    if version.is_empty() { name } else { format!("{name} {}", version.join(".")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heads(lines: &[&str]) -> Vec<Vec<u8>> {
        lines.iter().map(|l| l.as_bytes().to_vec()).collect()
    }

    #[test]
    fn a_claude_turn_is_a_prompt_and_its_tool_results_are_not() {
        let h = heads(&[
            r#"{"parentUuid":null,"isSidechain":false,"promptId":"p1","type":"user","message":{"role":"user","content":"go"}}"#,
            r#"{"parentUuid":"a","isSidechain":false,"message":{"model":"claude-opus-5-5"},"type":"assistant"}"#,
            r#"{"parentUuid":"b","isSidechain":false,"promptId":"p1","type":"user","message":{"role":"user","content":[{"tool_use_id":"t","type":"tool_result"}]}}"#,
            r#"{"parentUuid":"c","isSidechain":false,"promptId":"p2","type":"user","message":{"role":"user","content":"next"}}"#,
            r#"{"parentUuid":"d","isSidechain":true,"promptId":"p9","type":"user","message":{"role":"user","content":"sub"}}"#,
        ]);
        assert_eq!(claude_turns(&h), 2);
    }

    #[test]
    fn an_older_transcript_counts_its_typed_prompts() {
        let h = heads(&[
            r#"{"isSidechain":false,"type":"user","message":{"role":"user","content":"one"}}"#,
            r#"{"isSidechain":false,"type":"user","message":{"role":"user","content":[{"type":"tool_result"}]}}"#,
            r#"{"isSidechain":false,"isMeta":true,"type":"user","message":{"role":"user","content":"meta"}}"#,
            r#"{"isSidechain":false,"type":"user","message":{"role":"user","content":"two"}}"#,
        ]);
        assert_eq!(claude_turns(&h), 2);
    }

    #[test]
    fn claude_context_is_the_last_real_messages_input() {
        let tail = concat!(
            r#"{"isSidechain":false,"type":"assistant","message":{"model":"claude-opus-5-5","usage":{"input_tokens":2,"cache_read_input_tokens":300000,"cache_creation_input_tokens":649,"output_tokens":112}}}"#,
            "\n",
            r#"{"isSidechain":false,"type":"assistant","message":{"model":"<synthetic>","usage":{"input_tokens":0}}}"#,
            "\n",
            r#"{"type":"last-prompt","lastPrompt":"x"}"#,
            "\n",
        );
        assert_eq!(
            claude_tail(tail.as_bytes()),
            (Some(300_651), Some("claude-opus-5-5".to_string()))
        );
        // A tail that starts mid-line skips the broken first line.
        let cut = format!("{}\n{tail}", &r#"{"type":"assistant","message":{"model":"claude-x"}}"#[9..]);
        assert_eq!(claude_tail(cut.as_bytes()).0, Some(300_651));
    }

    #[test]
    fn codex_reads_its_last_token_count_and_model() {
        let tail = concat!(
            r#"{"type":"turn_context","payload":{"cwd":"/x","model":"gpt-5-codex"}}"#,
            "\n",
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"hi"}}"#,
            "\n",
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":9000,"total_tokens":9500},"model_context_window":272000}}}"#,
            "\n",
        );
        assert_eq!(
            codex_tail(tail.as_bytes()),
            (Some(9500), Some(272_000), Some("gpt-5-codex".to_string()))
        );
        assert_eq!(codex_turns(&heads(&tail.lines().collect::<Vec<_>>())), 1);
    }

    #[test]
    fn tokens_read_short() {
        assert_eq!(short_tokens(940), "940");
        assert_eq!(short_tokens(324_846), "325k");
        assert_eq!(short_tokens(1_000_000), "1M");
        assert_eq!(short_tokens(1_240_000), "1.2M");
    }

    #[test]
    fn models_read_as_names() {
        assert_eq!(model_label("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(model_label("claude-haiku-4-5-20251001"), "Haiku 4.5");
        assert_eq!(model_label("claude-fable-5-1"), "Fable 5.1");
        assert_eq!(model_label("gpt-5-codex"), "gpt-5-codex");
    }
}
