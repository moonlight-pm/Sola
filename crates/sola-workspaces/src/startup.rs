//! Per-project script run after a sibling worktree is created.
//!
//! Empty `Project.startup` is a no-op. The script is `/bin/sh -c` in the
//! new worktree. See [`VARS`].

use std::process::Command;

use crate::workspace::{Project, Workspace};

pub const PROJECT: &str = "PROJECT";
pub const WORKTREE: &str = "WORKTREE";
pub const NAME: &str = "NAME";

/// Shown in the editor. Keep this list the source of truth for copy.
pub struct Var {
    pub name: &'static str,
    pub help: &'static str,
}

pub const VARS: &[Var] = &[
    Var {
        name: PROJECT,
        help: "Project folder on disk (the root checkout)",
    },
    Var {
        name: WORKTREE,
        help: "This tab's folder — <project>/.worktrees/<name>",
    },
    Var {
        name: NAME,
        help: "Tab name",
    },
];

/// Run `project.startup` in `ws.path`. `Ok` when empty or the shell exits 0.
pub fn run(project: &Project, ws: &Workspace) -> Result<(), String> {
    let script = project.startup.trim();
    if script.is_empty() {
        return Ok(());
    }
    let output = Command::new("sh")
        .arg("-c")
        .arg(script)
        .current_dir(&ws.path)
        .env(PROJECT, &project.root)
        .env(WORKTREE, &ws.path)
        .env(NAME, &ws.name)
        .output()
        .map_err(|e| format!("startup: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if !stderr.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };
    if detail.is_empty() {
        Err(format!("startup exited {status}", status = output.status))
    } else {
        Err(format!("startup: {detail}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::status::AgentStatus;
    use crate::workspace::Kind;

    fn project(root: PathBuf, script: &str) -> Project {
        Project {
            id: "proj".into(),
            name: "Illuno".into(),
            collapsed: false,
            root,
            startup: script.into(),
        }
    }

    fn ws(path: PathBuf) -> Workspace {
        Workspace {
            id: "ws-kid".into(),
            project_id: "proj".into(),
            name: "kid".into(),
            title: None,
            path,
            kind: Kind::Worktree,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        }
    }

    #[test]
    fn empty_is_ok() {
        let p = project(PathBuf::from("/tmp"), "");
        let w = ws(PathBuf::from("/tmp"));
        assert!(run(&p, &w).is_ok());
    }

    #[test]
    fn writes_using_env() {
        let root = std::env::temp_dir().join(format!("sola-ws-startup-root-{}", std::process::id()));
        let dest = std::env::temp_dir().join(format!("sola-ws-startup-ws-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(root.join("marker"), "from-root").unwrap();
        let p = project(
            root.clone(),
            r#"cp "$PROJECT/marker" "$WORKTREE/copied" && printf '%s' "$NAME" > "$WORKTREE/name""#,
        );
        let w = ws(dest.clone());
        run(&p, &w).unwrap();
        assert_eq!(std::fs::read_to_string(dest.join("copied")).unwrap(), "from-root");
        assert_eq!(std::fs::read_to_string(dest.join("name")).unwrap(), "kid");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn nonzero_is_err() {
        let dest = std::env::temp_dir().join(format!("sola-ws-startup-fail-{}", std::process::id()));
        std::fs::create_dir_all(&dest).unwrap();
        let p = project(dest.clone(), "echo nope >&2; exit 7");
        let w = ws(dest.clone());
        let err = run(&p, &w).unwrap_err();
        assert!(err.contains("nope"), "{err}");
        let _ = std::fs::remove_dir_all(&dest);
    }
}
