//! Grok: write `sola-status.json` next to Orca's file (never touch
//! `orca-status.json`). Codex: merge Sola status handlers into
//! `~/.codex/hooks.json` without dropping Impeccable (or other) groups.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

const SCRIPT_NAME: &str = "grok-hook.sh";
const CODEX_SCRIPT_NAME: &str = "codex-hook.sh";
const HOOK_FILE: &str = "sola-status.json";
const CODEX_HOOK_MARK: &str = "codex-hook.sh";
const CODEX_EVENTS: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Stop",
    "Interrupt",
    "PostCompact",
];

pub struct HookPaths {
    pub grok_hooks_dir: PathBuf,
    pub script_path: PathBuf,
    pub socket_path: PathBuf,
    pub codex_hooks_json: PathBuf,
    pub codex_script_path: PathBuf,
}

impl HookPaths {
    pub fn live() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let grok_root = std::env::var_os("GROK_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".grok"));
        let codex_root = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        let cfg = crate::paths::config_dir();
        Self {
            grok_hooks_dir: grok_root.join("hooks"),
            script_path: cfg.join(SCRIPT_NAME),
            socket_path: sola_core::env::runtime_dir().join("sola-ws-hooks.sock"),
            codex_hooks_json: codex_root.join("hooks.json"),
            codex_script_path: cfg.join(CODEX_SCRIPT_NAME),
        }
    }
}

/// Idempotent: rewrite our scripts + hook files. Leave Orca and Impeccable alone.
pub fn install(paths: &HookPaths) -> std::io::Result<()> {
    install_grok(paths)?;
    install_codex(paths)?;
    Ok(())
}

fn install_grok(paths: &HookPaths) -> std::io::Result<()> {
    if let Some(dir) = paths.script_path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::create_dir_all(&paths.grok_hooks_dir)?;
    write_executable(&paths.script_path, &hook_script("grok"))?;
    let hook_json = grok_hook_json(&paths.script_path);
    fs::write(paths.grok_hooks_dir.join(HOOK_FILE), hook_json)?;
    Ok(())
}

fn install_codex(paths: &HookPaths) -> std::io::Result<()> {
    if let Some(dir) = paths.codex_script_path.parent() {
        fs::create_dir_all(dir)?;
    }
    if let Some(dir) = paths.codex_hooks_json.parent() {
        fs::create_dir_all(dir)?;
    }
    write_executable(&paths.codex_script_path, &hook_script("codex"))?;
    let command = hook_command(&paths.codex_script_path);
    let existing = fs::read_to_string(&paths.codex_hooks_json).unwrap_or_default();
    let parsed = if existing.trim().is_empty() {
        json!({"hooks": {}})
    } else {
        match serde_json::from_str::<Value>(&existing) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    path = %paths.codex_hooks_json.display(),
                    "codex hooks.json is not JSON ({e}); leaving it alone"
                );
                return Ok(());
            }
        }
    };
    let merged = merge_codex_hooks(parsed, &command);
    let text = serde_json::to_string_pretty(&merged).unwrap_or_else(|_| existing);
    fs::write(&paths.codex_hooks_json, format!("{text}\n"))?;
    Ok(())
}

fn write_executable(path: &Path, contents: &str) -> std::io::Result<()> {
    fs::write(path, contents)?;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)
}

fn is_ours(command: &str) -> bool {
    command.contains(CODEX_HOOK_MARK)
}

fn our_handler(command: &str) -> Value {
    json!({
        "type": "command",
        "command": command,
        "timeout": 3,
        "async": true,
        "statusMessage": "Sola Workspaces status"
    })
}

pub fn merge_codex_hooks(mut doc: Value, command: &str) -> Value {
    if !doc.is_object() {
        doc = json!({"hooks": {}});
    }
    {
        let hooks = doc
            .as_object_mut()
            .unwrap()
            .entry("hooks")
            .or_insert_with(|| json!({}));
        if !hooks.is_object() {
            *hooks = json!({});
        }
        let map = hooks.as_object_mut().unwrap();
        for event in CODEX_EVENTS {
            let groups = map
                .entry((*event).to_string())
                .or_insert_with(|| json!([]));
            if !groups.is_array() {
                *groups = json!([]);
            }
            let arr = groups.as_array_mut().unwrap();
            let mut found = false;
            for group in arr.iter_mut() {
                let Some(hooks_arr) = group.get_mut("hooks").and_then(|v| v.as_array_mut()) else {
                    continue;
                };
                if let Some(h) = hooks_arr.iter_mut().find(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .is_some_and(is_ours)
                }) {
                    *h = our_handler(command);
                    found = true;
                    break;
                }
            }
            if !found {
                arr.push(json!({ "hooks": [our_handler(command)] }));
            }
        }
    }
    doc
}

#[cfg(test)]
fn orca_hook_path(hooks_dir: &Path) -> PathBuf {
    hooks_dir.join("orca-status.json")
}

fn hook_script(agent: &str) -> String {
    // Drain stdin first (the CLI closes the pipe). Fail-open if the app is down.
    // Codex Stop expects JSON or empty stdout — never leak curl text.
    format!(
        r#"#!/bin/sh
payload=$({{ command -p cat 2>/dev/null || cat; }})
if [ -z "$payload" ]; then
  exit 0
fi
if [ -z "$SOLA_PANE_ID" ]; then
  exit 0
fi
sock="${{SOLA_WS_HOOKS_SOCK:-}}"
if [ -z "$sock" ]; then
  sock="${{XDG_RUNTIME_DIR:-/tmp}}/sola-ws-hooks.sock"
fi
if [ ! -S "$sock" ]; then
  exit 0
fi
printf '%s' "$payload" | curl -sS --unix-socket "$sock" -X POST "http://localhost/hook/{agent}" \
  --connect-timeout 0.5 --max-time 1.5 \
  -H "Content-Type: application/json" \
  -H "X-Sola-Pane-Id: ${{SOLA_PANE_ID}}" \
  --data-binary @- >/dev/null 2>&1 || true
exit 0
"#
    )
}

fn hook_command(script: &Path) -> String {
    format!(
        "if [ -f '{script}' ] && [ -r '{script}' ] && [ -x '{script}' ]; then /bin/sh '{script}'; else {{ command -p cat 2>/dev/null || cat; }} >/dev/null 2>&1 || :; fi",
        script = script.display()
    )
}

fn grok_hook_json(script: &Path) -> String {
    let cmd = hook_command(script);
    let entry = serde_json::json!({
        "hooks": [{ "type": "command", "command": cmd, "timeout": 10 }]
    });
    let tool = serde_json::json!({
        "matcher": ".*",
        "hooks": [{ "type": "command", "command": cmd, "timeout": 10 }]
    });
    let doc = serde_json::json!({
        "hooks": {
            "SessionStart": [entry.clone()],
            "UserPromptSubmit": [entry.clone()],
            "Stop": [entry.clone()],
            "StopFailure": [entry.clone()],
            "StopCancelled": [entry.clone()],
            "SessionEnd": [entry.clone()],
            "PostCompact": [entry.clone()],
            "PreToolUse": [tool.clone()],
            "PostToolUse": [tool.clone()],
            "PostToolUseFailure": [tool],
            "Notification": [entry]
        }
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp_paths() -> (HookPaths, PathBuf) {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("sola-ws-hooks-{n}"));
        let paths = HookPaths {
            grok_hooks_dir: root.join("hooks"),
            script_path: root.join("bin").join(SCRIPT_NAME),
            socket_path: root.join("sola-ws-hooks.sock"),
            codex_hooks_json: root.join("codex").join("hooks.json"),
            codex_script_path: root.join("bin").join(CODEX_SCRIPT_NAME),
        };
        (paths, root)
    }

    #[test]
    fn writes_sola_status_not_orca() {
        let (paths, root) = tmp_paths();
        install(&paths).unwrap();
        assert!(paths.grok_hooks_dir.join(HOOK_FILE).is_file());
        assert!(!orca_hook_path(&paths.grok_hooks_dir).exists());
        let text = fs::read_to_string(paths.grok_hooks_dir.join(HOOK_FILE)).unwrap();
        assert!(text.contains("UserPromptSubmit"));
        assert!(text.contains("StopFailure"));
        assert!(text.contains("StopCancelled"));
        assert!(text.contains("PostCompact"));
        assert!(text.contains("sola-status") || text.contains("grok-hook.sh"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn does_not_overwrite_existing_orca_file() {
        let (paths, root) = tmp_paths();
        fs::create_dir_all(&paths.grok_hooks_dir).unwrap();
        let orca = orca_hook_path(&paths.grok_hooks_dir);
        fs::write(&orca, "{\"keep\":true}").unwrap();
        install(&paths).unwrap();
        assert_eq!(fs::read_to_string(&orca).unwrap(), "{\"keep\":true}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn codex_merge_keeps_impeccable() {
        let (paths, root) = tmp_paths();
        let existing = serde_json::json!({
            "hooks": {
                "PostToolUse": [{
                    "matcher": "Edit|Write|apply_patch",
                    "hooks": [{
                        "type": "command",
                        "command": "impeccable hook",
                        "timeout": 5
                    }]
                }],
                "Stop": [{
                    "hooks": [{
                        "type": "command",
                        "command": "impeccable hook",
                        "timeout": 30
                    }]
                }]
            }
        });
        fs::create_dir_all(paths.codex_hooks_json.parent().unwrap()).unwrap();
        fs::write(
            &paths.codex_hooks_json,
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();
        install(&paths).unwrap();
        let text = fs::read_to_string(&paths.codex_hooks_json).unwrap();
        assert!(text.contains("impeccable hook"));
        assert!(text.contains("codex-hook.sh"));
        assert!(text.contains("PermissionRequest"));
        assert!(text.contains("Sola Workspaces status"));
        assert!(paths.codex_script_path.is_file());
        let again = fs::read_to_string(&paths.codex_hooks_json).unwrap();
        install(&paths).unwrap();
        let twice = fs::read_to_string(&paths.codex_hooks_json).unwrap();
        let count = |s: &str| s.matches("codex-hook.sh").count();
        assert_eq!(count(&again), count(&twice));
        let _ = fs::remove_dir_all(root);
    }
}
