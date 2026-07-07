//! Permission policy (pure). `static_decision` is the first, no-LLM gate: the
//! engine (Task 27) calls it before any optional classifier pass or user
//! prompt. Still to land: `Risk`, `classify`, `remember` (Tasks 25-26).
//!
//! Task 14 note: `Rule`/`Policy` were forward-declared field-for-field ahead
//! of this task so the engine's `run_turn` could thread a `&mut Policy`
//! through the turn loop before the gate itself existed. This task adds the
//! real logic on top without changing either struct's shape.

use std::path::{Component, Path, PathBuf};

/// One session-policy grant, e.g. `{ tool: "bash", scope: "always" }`.
#[derive(Debug, Clone)]
pub struct Rule {
    pub tool: String,
    pub scope: String,
}

/// The active conversation's permission policy. Task 14 only carries this
/// through the loop; `static_decision`/`remember`/`classify` (Tasks 24-26)
/// add the actual gating logic.
#[derive(Debug, Clone)]
pub struct Policy {
    pub project_root: PathBuf,
    pub always: Vec<Rule>,
    pub classifier: bool,
}

/// Result of the static (no-LLM) policy pass.
#[derive(Debug)]
pub enum StaticDecision {
    AutoAllow,
    NeedsPrompt { preview: String },
}

/// Decide a tool call from static rules alone — no network, no side effects.
///
/// * a matching `always` rule → `AutoAllow`
/// * read-only tools (`read`, `search`) → `AutoAllow`
/// * `write`/`edit` whose resolved target is inside `project_root` → `AutoAllow`,
///   otherwise `NeedsPrompt`
/// * `bash` → always `NeedsPrompt` (preview = the command)
/// * anything else → `NeedsPrompt` (safe default)
pub fn static_decision(policy: &Policy, tool: &str, args: &serde_json::Value) -> StaticDecision {
    if policy
        .always
        .iter()
        .any(|r| r.tool == tool && r.scope == "always")
    {
        return StaticDecision::AutoAllow;
    }

    match tool {
        "read" | "search" => StaticDecision::AutoAllow,
        "write" | "edit" => {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if path_inside_root(&policy.project_root, path) {
                StaticDecision::AutoAllow
            } else {
                StaticDecision::NeedsPrompt {
                    preview: format!("{tool} target outside project root: {path}"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn policy() -> Policy {
        Policy {
            project_root: std::path::PathBuf::from("/home/agent/project"),
            always: Vec::new(),
            classifier: false,
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
    fn manual_always_rule_auto_allows_bash() {
        let mut p = policy();
        p.always.push(Rule {
            tool: "bash".into(),
            scope: "always".into(),
        });
        let d = static_decision(&p, "bash", &json!({ "command": "ls" }));
        assert!(matches!(d, StaticDecision::AutoAllow), "got {d:?}");
    }
}
