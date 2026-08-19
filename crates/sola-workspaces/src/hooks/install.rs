//! Write `sola-status.json` next to Orca's hook file. Never touch
//! `orca-status.json`. Grok is the only installer in this slice.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const SCRIPT_NAME: &str = "grok-hook.sh";
const HOOK_FILE: &str = "sola-status.json";

pub struct HookPaths {
    pub grok_hooks_dir: PathBuf,
    pub script_path: PathBuf,
    pub socket_path: PathBuf,
}

impl HookPaths {
    pub fn live() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let grok_root = std::env::var_os("GROK_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".grok"));
        Self {
            grok_hooks_dir: grok_root.join("hooks"),
            script_path: crate::paths::config_dir().join(SCRIPT_NAME),
            socket_path: sola_core::env::runtime_dir().join("sola-ws-hooks.sock"),
        }
    }
}

/// Idempotent: rewrite our script + hook file. Leave Orca's file alone.
pub fn install(paths: &HookPaths) -> std::io::Result<()> {
    if let Some(dir) = paths.script_path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::create_dir_all(&paths.grok_hooks_dir)?;
    fs::write(&paths.script_path, hook_script())?;
    let mut perms = fs::metadata(&paths.script_path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&paths.script_path, perms)?;

    let hook_json = hook_json(&paths.script_path);
    fs::write(paths.grok_hooks_dir.join(HOOK_FILE), hook_json)?;
    Ok(())
}

#[cfg(test)]
fn orca_hook_path(hooks_dir: &Path) -> PathBuf {
    hooks_dir.join("orca-status.json")
}

fn hook_script() -> String {
    // Drain stdin first (Grok closes the pipe). Fail-open if the app is down.
    r#"#!/bin/sh
payload=$({ command -p cat 2>/dev/null || cat; })
if [ -z "$payload" ]; then
  exit 0
fi
if [ -z "$SOLA_PANE_ID" ]; then
  exit 0
fi
sock="${SOLA_WS_HOOKS_SOCK:-}"
if [ -z "$sock" ]; then
  sock="${XDG_RUNTIME_DIR:-/tmp}/sola-ws-hooks.sock"
fi
if [ ! -S "$sock" ]; then
  exit 0
fi
printf '%s' "$payload" | curl -sS --unix-socket "$sock" -X POST "http://localhost/hook/grok" \
  --connect-timeout 0.5 --max-time 1.5 \
  -H "Content-Type: application/json" \
  -H "X-Sola-Pane-Id: ${SOLA_PANE_ID}" \
  --data-binary @- >/dev/null 2>&1 || true
exit 0
"#
    .to_string()
}

fn hook_json(script: &Path) -> String {
    let cmd = format!(
        "if [ -f '{script}' ] && [ -r '{script}' ] && [ -x '{script}' ]; then /bin/sh '{script}'; else {{ command -p cat 2>/dev/null || cat; }} >/dev/null 2>&1 || :; fi",
        script = script.display()
    );
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
}
