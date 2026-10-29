//! Per-project script run after a sibling worktree is created.
//!
//! Scripts are kept by the project's folder (`<host>:<path>` for one on
//! another computer) in `~/.config/sola/projects/startup.json`, apart from
//! the open projects, so a closed project's script is there when it opens
//! again. An empty script is a no-op. The script is `/bin/sh -c` in the
//! new worktree. See [`VARS`].

use std::collections::BTreeMap;
use std::path::PathBuf;


use crate::catalog::{Project, Tab};
use crate::machine::Machine;

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

fn store_path() -> PathBuf {
    crate::paths::config_dir().join("startup.json")
}

/// The store's key for `project`: its folder, with its host first.
fn key(project: &Project) -> String {
    let path = project.root.to_string_lossy();
    match &project.host {
        Some(host) => format!("{host}:{path}"),
        None => path.into_owned(),
    }
}

fn load() -> BTreeMap<String, String> {
    sola_core::config::load_json_or_default(&store_path()).unwrap_or_else(|e| {
        tracing::warn!("startup scripts: {e}");
        BTreeMap::new()
    })
}

/// `project`'s startup script (empty: none).
pub fn script_for(project: &Project) -> String {
    load().remove(&key(project)).unwrap_or_default()
}

/// Keep `script` as `project`'s (empty drops it).
pub fn set_script(project: &Project, script: &str) -> Result<(), String> {
    let mut all = load();
    if script.trim().is_empty() {
        all.remove(&key(project));
    } else {
        all.insert(key(project), script.to_string());
    }
    sola_core::config::save_json_pretty(&store_path(), &all).map_err(|e| e.to_string())
}

/// Run `script` (the project's startup) in `ws.path`, on the computer the
/// project is on. `Ok` when empty or the shell exits 0.
pub fn run(m: &Machine, script: &str, project: &Project, ws: &Tab) -> Result<(), String> {
    let script = script.trim();
    if script.is_empty() {
        return Ok(());
    }
    let root = project.root.to_string_lossy();
    let path = ws.path.to_string_lossy();
    let env = [(PROJECT, root.as_ref()), (WORKTREE, path.as_ref()), (NAME, ws.name.as_str())];
    let output = m
        .run(Some(&ws.path), &env, &["sh", "-c", script], None)
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
    use crate::catalog::Kind;

    fn project(root: PathBuf) -> Project {
        Project {
            selected: None,
            id: "proj".into(),
            name: "Illuno".into(),
            collapsed: false,
            root,
            host: None,
            startup: String::new(),
            closed: false,
        }
    }

    fn ws(path: PathBuf) -> Tab {
        Tab {
            id: "ws-kid".into(),
            project_id: "proj".into(),
            name: "kid".into(),
            title: None,
            path,
            host: None,
            kind: Kind::Worktree,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        }
    }

    #[test]
    fn scripts_are_kept_by_folder_and_host() {
        let here = project(PathBuf::from("/home/j/Projects/Sola"));
        let there = Project {
            host: Some("novus".into()),
            ..here.clone()
        };
        assert_eq!(key(&here), "/home/j/Projects/Sola");
        assert_eq!(key(&there), "novus:/home/j/Projects/Sola");
    }

    #[test]
    fn empty_is_ok() {
        let p = project(PathBuf::from("/tmp"));
        let w = ws(PathBuf::from("/tmp"));
        assert!(run(&Machine::Local, "", &p, &w).is_ok());
    }

    #[test]
    fn writes_using_env() {
        let root =
            std::env::temp_dir().join(format!("sola-ws-startup-root-{}", std::process::id()));
        let dest = std::env::temp_dir().join(format!("sola-ws-startup-ws-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(root.join("marker"), "from-root").unwrap();
        let p = project(root.clone());
        let script = r#"cp "$PROJECT/marker" "$WORKTREE/copied" && printf '%s' "$NAME" > "$WORKTREE/name""#;
        let w = ws(dest.clone());
        run(&Machine::Local, script, &p, &w).unwrap();
        assert_eq!(
            std::fs::read_to_string(dest.join("copied")).unwrap(),
            "from-root"
        );
        assert_eq!(std::fs::read_to_string(dest.join("name")).unwrap(), "kid");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn nonzero_is_err() {
        let dest =
            std::env::temp_dir().join(format!("sola-ws-startup-fail-{}", std::process::id()));
        std::fs::create_dir_all(&dest).unwrap();
        let p = project(dest.clone());
        let w = ws(dest.clone());
        let err = run(&Machine::Local, "echo nope >&2; exit 7", &p, &w).unwrap_err();
        assert!(err.contains("nope"), "{err}");
        let _ = std::fs::remove_dir_all(&dest);
    }
}
