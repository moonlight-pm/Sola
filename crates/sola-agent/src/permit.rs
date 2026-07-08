//! Permission policy (pure). `static_decision` is the first, no-LLM gate: the
//! engine (Task 27) calls it before any optional classifier pass or user
//! prompt. Still to land: `Risk`, `classify`, `remember` (Tasks 25-26).
//!
//! Task 14 note: `Rule`/`Policy` were forward-declared field-for-field ahead
//! of this task so the engine's `run_turn` could thread a `&mut Policy`
//! through the turn loop before the gate itself existed. This task adds the
//! real logic on top without changing either struct's shape.

use std::path::{Component, Path, PathBuf};

use crate::provider::{InputItem, LlmStream, StreamEvent};

/// One session-policy grant, e.g. `{ tool: "bash", scope: "always" }`.
///
/// `path` scopes a grant to a single resolved filesystem target. It is
/// `Some(resolved_path)` for the path-based tools (`read`/`search`/`write`/
/// `edit`), so clicking "Always allow" on one out-of-root write blesses only
/// that path — not every write for the session. `None` is a tool-wide grant
/// (used by `bash`, a deliberate all-bash trust, and by any manually-created
/// rule that predates path scoping).
#[derive(Debug, Clone)]
pub struct Rule {
    pub tool: String,
    pub scope: String,
    pub path: Option<PathBuf>,
}

/// The active conversation's permission policy. Task 14 only carries this
/// through the loop; `static_decision`/`remember`/`classify` (Tasks 24-26)
/// add the actual gating logic. Whether the classifier pass runs at all is
/// `EngineConfig.classifier` (a per-process setting, not per-policy).
#[derive(Debug, Clone)]
pub struct Policy {
    pub project_root: PathBuf,
    pub always: Vec<Rule>,
}

/// Result of the static (no-LLM) policy pass.
#[derive(Debug)]
pub enum StaticDecision {
    AutoAllow,
    NeedsPrompt { preview: String },
}

/// Decide a tool call from static rules alone — no network, no side effects.
///
/// * a matching `always` rule → `AutoAllow` (a path-scoped rule matches only
///   when this call's resolved target equals the rule's path; a tool-wide rule
///   with `path: None` matches any call of that tool)
/// * path-based tools (`read`/`search`/`write`/`edit`) whose resolved target is
///   inside `project_root` → `AutoAllow`, otherwise `NeedsPrompt`; a missing
///   `path` arg is treated as unsafe and prompts (never auto-allowed)
/// * `bash` → always `NeedsPrompt` (preview = the command)
/// * anything else → `NeedsPrompt` (safe default)
pub fn static_decision(policy: &Policy, tool: &str, args: &serde_json::Value) -> StaticDecision {
    let target = call_target(&policy.project_root, tool, args);
    if policy.always.iter().any(|r| {
        r.tool == tool
            && r.scope == "always"
            && match &r.path {
                // Tool-wide grant (bash / legacy) — matches any call of `tool`.
                None => true,
                // Path-scoped grant — only this exact resolved target.
                Some(p) => target.as_ref() == Some(p),
            }
    }) {
        return StaticDecision::AutoAllow;
    }

    match tool {
        "read" | "search" | "write" | "edit" => {
            // A missing path arg is unsafe: an empty/bare path would fold to the
            // project root and auto-allow, so prompt instead of trusting it.
            let Some(raw) = args.get("path").and_then(|v| v.as_str()) else {
                return StaticDecision::NeedsPrompt {
                    preview: format!("{tool}: missing path argument"),
                };
            };
            if path_inside_root(&policy.project_root, raw) {
                StaticDecision::AutoAllow
            } else {
                StaticDecision::NeedsPrompt {
                    preview: format!("{tool} target outside project root: {raw}"),
                }
            }
        }
        "bash" => {
            let command = args
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            StaticDecision::NeedsPrompt {
                preview: command.to_string(),
            }
        }
        _ => StaticDecision::NeedsPrompt {
            preview: format!("{tool}: {args}"),
        },
    }
}

/// True for the path-based tools whose grants are scoped to a single resolved
/// filesystem target (as opposed to `bash`, which trusts tool-wide).
fn is_path_scoped(tool: &str) -> bool {
    matches!(tool, "read" | "search" | "write" | "edit")
}

/// The resolved, lexically-normalized filesystem target a path-based tool call
/// points at, or `None` for a non-path tool or a missing `path` arg. Used both
/// to match path-scoped `always` rules and to compute the path a `remember`
/// grant is scoped to, so the two always agree on what "the same target" means.
fn call_target(root: &Path, tool: &str, args: &serde_json::Value) -> Option<PathBuf> {
    if !is_path_scoped(tool) {
        return None;
    }
    let raw = args.get("path").and_then(|v| v.as_str())?;
    Some(resolve_target(root, raw))
}

/// True when `raw` (relative to `root`, or absolute) resolves *inside* `root`.
/// Lexical only — the target may not exist yet (writes create it), so we never
/// touch the filesystem and never call `canonicalize`. `..` segments are folded
/// away, so `../escape` lands outside and prompts. Comparison is component-wise,
/// so `/root/project2` does not match `/root/project`.
///
/// This matters because `tools::resolve` (Task 18) does no normalization at
/// all — it just joins relative paths onto `project_root` — so a raw arg like
/// `../../etc/foo` would otherwise pass a naive string-prefix check even
/// though it escapes the root. Folding `..` lexically here closes that gap.
fn path_inside_root(root: &Path, raw: &str) -> bool {
    let target = resolve_target(root, raw);
    let root_norm = normalize_lexically(root);
    target.starts_with(&root_norm)
}

fn resolve_target(root: &Path, raw: &str) -> PathBuf {
    let raw_path = Path::new(raw);
    let joined = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        root.join(raw_path)
    };
    normalize_lexically(&joined)
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Persist an always-allow grant for this call in the session policy. The
/// engine calls this when the user picks "Always allow". For the path-based
/// tools the grant is scoped to the exact resolved target (so "Always" on one
/// out-of-root write does not bless writes elsewhere); `bash` is stored
/// tool-wide (`path: None`), a deliberate all-bash trust. Idempotent per
/// (tool, path).
pub fn remember(policy: &mut Policy, tool: &str, args: &serde_json::Value) {
    // Path-based tools always carry a concrete scope path (a missing arg folds
    // to the project root) so they can never widen into a tool-wide grant.
    let path = is_path_scoped(tool).then(|| {
        let raw = args.get("path").and_then(|v| v.as_str()).unwrap_or_default();
        resolve_target(&policy.project_root, raw)
    });
    let already = policy
        .always
        .iter()
        .any(|r| r.tool == tool && r.scope == "always" && r.path == path);
    if already {
        return;
    }
    policy.always.push(Rule {
        tool: tool.to_string(),
        scope: "always".to_string(),
        path,
    });
}

/// The classifier's three-way verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    Safe,
    Caution,
    Danger,
}

/// One cheap, non-streaming `fugu` call that rates a would-be-prompted action.
/// Builds a tiny prompt, runs it through the provider seam (no tools, sink
/// ignored), and parses a `{"verdict": "safe|caution|danger"}` reply. Anything
/// that fails to parse — or any transport error — defaults to `Caution`, so the
/// classifier can only ever soften a prompt to auto-run on an explicit `safe`;
/// it never escalates a failure into an auto-allow.
pub fn classify(provider: &dyn LlmStream, tool: &str, args: &serde_json::Value) -> Risk {
    let prompt = format!(
        "You are a safety classifier for a coding agent. The agent wants to run \
         the tool `{tool}` with these arguments:\n{args}\n\n\
         Reply with ONLY a JSON object: {{\"verdict\": \"safe\" | \"caution\" | \"danger\"}}. \
         safe = read-only or clearly harmless; caution = could modify local files; \
         danger = destructive, exfiltrating, or system-wide."
    );
    let input = [InputItem::Message {
        role: "user".to_string(),
        text: prompt,
    }];
    let mut sink = |_ev: StreamEvent| {};
    match provider.stream_turn("fugu", "high", &input, &[], &mut sink) {
        Ok(outcome) => parse_verdict(&outcome.assistant_text),
        Err(_) => Risk::Caution,
    }
}

fn parse_verdict(text: &str) -> Risk {
    let verdict = extract_json(text)
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("verdict")
                .and_then(|x| x.as_str())
                .map(str::to_string)
        });
    match verdict.as_deref() {
        Some("safe") => Risk::Safe,
        Some("danger") => Risk::Danger,
        _ => Risk::Caution,
    }
}

/// Slice the first `{ .. }` span out of a reply that may carry prose around it.
fn extract_json(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end < start {
        return None;
    }
    Some(text[start..=end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn policy() -> Policy {
        Policy {
            project_root: std::path::PathBuf::from("/home/agent/project"),
            always: Vec::new(),
        }
    }

    #[test]
    fn read_auto_allows() {
        let p = policy();
        let d = static_decision(&p, "read", &json!({ "path": "src/main.rs" }));
        assert!(matches!(d, StaticDecision::AutoAllow), "got {d:?}");
    }

    #[test]
    fn write_inside_root_auto_allows() {
        let p = policy();
        let d = static_decision(
            &p,
            "write",
            &json!({ "path": "src/new.rs", "content": "x" }),
        );
        assert!(matches!(d, StaticDecision::AutoAllow), "got {d:?}");
    }

    #[test]
    fn write_outside_root_prompts() {
        let p = policy();
        let d = static_decision(
            &p,
            "write",
            &json!({ "path": "/etc/passwd", "content": "x" }),
        );
        match d {
            StaticDecision::NeedsPrompt { preview } => assert!(preview.contains("/etc/passwd")),
            other => panic!("expected prompt, got {other:?}"),
        }
    }

    #[test]
    fn path_escape_prompts() {
        let p = policy();
        let d = static_decision(
            &p,
            "edit",
            &json!({ "path": "../../secret.txt", "old": "a", "new": "b" }),
        );
        assert!(matches!(d, StaticDecision::NeedsPrompt { .. }), "got {d:?}");
    }

    #[test]
    fn bash_prompts() {
        let p = policy();
        let d = static_decision(&p, "bash", &json!({ "command": "rm -rf /tmp/x" }));
        match d {
            StaticDecision::NeedsPrompt { preview } => assert_eq!(preview, "rm -rf /tmp/x"),
            other => panic!("expected prompt, got {other:?}"),
        }
    }

    #[test]
    fn read_inside_root_auto_allows() {
        let p = policy();
        let d = static_decision(&p, "read", &json!({ "path": "src/main.rs" }));
        assert!(matches!(d, StaticDecision::AutoAllow), "got {d:?}");
    }

    #[test]
    fn read_outside_root_prompts() {
        let p = policy();
        let d = static_decision(&p, "read", &json!({ "path": "/home/agent/.ssh/id_rsa" }));
        match d {
            StaticDecision::NeedsPrompt { preview } => assert!(preview.contains("id_rsa"), "{preview}"),
            other => panic!("expected prompt, got {other:?}"),
        }
    }

    #[test]
    fn read_parent_escape_prompts() {
        let p = policy();
        let d = static_decision(&p, "read", &json!({ "path": "../../etc/passwd" }));
        assert!(matches!(d, StaticDecision::NeedsPrompt { .. }), "got {d:?}");
    }

    #[test]
    fn search_outside_root_prompts() {
        let p = policy();
        let d = static_decision(&p, "search", &json!({ "mode": "grep", "path": "/etc", "query": "root" }));
        assert!(matches!(d, StaticDecision::NeedsPrompt { .. }), "got {d:?}");
    }

    #[test]
    fn read_inside_root_via_search_auto_allows() {
        let p = policy();
        let d = static_decision(&p, "search", &json!({ "mode": "ls", "path": "src", "query": null }));
        assert!(matches!(d, StaticDecision::AutoAllow), "got {d:?}");
    }

    #[test]
    fn path_tool_missing_path_prompts() {
        let p = policy();
        let d = static_decision(&p, "read", &json!({}));
        assert!(matches!(d, StaticDecision::NeedsPrompt { .. }), "got {d:?}");
    }

    #[test]
    fn manual_always_rule_auto_allows_bash() {
        let mut p = policy();
        p.always.push(Rule {
            tool: "bash".into(),
            scope: "always".into(),
            path: None,
        });
        let d = static_decision(&p, "bash", &json!({ "command": "ls" }));
        assert!(matches!(d, StaticDecision::AutoAllow), "got {d:?}");
    }

    #[test]
    fn remember_then_bash_auto_allows() {
        let mut p = policy();
        assert!(
            matches!(
                static_decision(&p, "bash", &json!({ "command": "ls" })),
                StaticDecision::NeedsPrompt { .. }
            ),
            "bash should prompt before remember()"
        );

        remember(&mut p, "bash", &json!({ "command": "ls" }));

        // A tool-wide bash grant auto-allows any bash command, not just `ls`.
        assert!(
            matches!(
                static_decision(&p, "bash", &json!({ "command": "rm -rf /tmp/x" })),
                StaticDecision::AutoAllow
            ),
            "bash should auto-allow any command after remember()"
        );

        // idempotent — a second remember() does not duplicate the rule.
        remember(&mut p, "bash", &json!({ "command": "whoami" }));
        assert_eq!(p.always.iter().filter(|r| r.tool == "bash").count(), 1);
    }

    #[test]
    fn remember_write_scopes_grant_to_the_prompted_path() {
        let mut p = policy();
        // Two distinct out-of-root targets: both prompt before any grant.
        let a = json!({ "path": "/etc/hosts", "content": "x" });
        let b = json!({ "path": "/etc/shadow", "content": "y" });
        assert!(matches!(static_decision(&p, "write", &a), StaticDecision::NeedsPrompt { .. }));
        assert!(matches!(static_decision(&p, "write", &b), StaticDecision::NeedsPrompt { .. }));

        // "Always" on the write to A grants only A.
        remember(&mut p, "write", &a);

        assert!(
            matches!(static_decision(&p, "write", &a), StaticDecision::AutoAllow),
            "the remembered path A should now auto-allow"
        );
        assert!(
            matches!(static_decision(&p, "write", &b), StaticDecision::NeedsPrompt { .. }),
            "a different out-of-root path B must still prompt"
        );
        // A per-path grant does not widen into a tool-wide write bless.
        assert!(p.always.iter().all(|r| r.path.is_some()));
    }

    /// Fake provider: a canned assistant reply (or a transport error), no
    /// streaming, no tool calls — enough to exercise `classify` offline.
    struct FakeStream {
        result: Result<String, String>,
    }

    impl crate::provider::LlmStream for FakeStream {
        fn stream_turn(
            &self,
            _model: &str,
            _effort: &str,
            _input: &[crate::provider::InputItem],
            _tools: &[serde_json::Value],
            _sink: &mut dyn FnMut(crate::provider::StreamEvent),
        ) -> Result<crate::provider::TurnOutcome, String> {
            match &self.result {
                Ok(text) => Ok(crate::provider::TurnOutcome {
                    assistant_text: text.clone(),
                    calls: Vec::new(),
                    usage: crate::session::Usage {
                        input_tokens: 0,
                        output_tokens: 0,
                    },
                }),
                Err(e) => Err(e.clone()),
            }
        }
    }

    #[test]
    fn classify_reads_safe_verdict() {
        let fake = FakeStream { result: Ok(r#"{"verdict":"safe"}"#.into()) };
        assert!(matches!(classify(&fake, "bash", &json!({ "command": "ls" })), Risk::Safe));
    }

    #[test]
    fn classify_reads_danger_verdict_with_prose() {
        let fake = FakeStream { result: Ok(r#"Sure: {"verdict":"danger","reason":"rm -rf /"}"#.into()) };
        assert!(matches!(classify(&fake, "bash", &json!({ "command": "rm -rf /" })), Risk::Danger));
    }

    #[test]
    fn classify_garbage_defaults_caution() {
        let fake = FakeStream { result: Ok("I cannot help with that.".into()) };
        assert!(matches!(classify(&fake, "bash", &json!({ "command": "ls" })), Risk::Caution));
    }

    #[test]
    fn classify_error_defaults_caution() {
        let fake = FakeStream { result: Err("network down".into()) };
        assert!(matches!(classify(&fake, "bash", &json!({ "command": "ls" })), Risk::Caution));
    }
}
