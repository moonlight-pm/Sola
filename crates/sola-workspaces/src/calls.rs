//! Advertised sola-call methods for owner `workspaces`.
//!
//! First-class surface: keep this list, dispatch, tests, and
//! `docs/manual/solactl.md` in the same change.
//! Freeze: `docs/specs/2026-08-18-workspaces-cli-design.md`.

use sola_call::{ArgSpec, ArgType, MethodSpec};

pub const OWNER: &str = "workspaces";

pub const SPAWN_TIMEOUT_MS: u64 = 60_000;
pub const ADD_TIMEOUT_MS: u64 = 15_000;
pub const WAIT_TIMEOUT_MS: u64 = 302_000;
pub const WAIT_DEFAULT_SECS: u64 = 300;

pub fn methods() -> Vec<MethodSpec> {
    vec![
        method("ps", "Project → workspace → state table", &[]),
        method("project.list", "List projects", &[]),
        method_ms(
            "project.add",
            "Register a project from a folder path",
            &[req(
                "path",
                Some('p'),
                ArgType::Path,
                "Folder path (`~` ok)",
            )],
            ADD_TIMEOUT_MS,
        ),
        method(
            "project.rm",
            "Unregister a project and kill its tmux sessions",
            &[req_s("project", Some('p'), "Project id or name")],
        ),
        method(
            "project.startup",
            "Get or set the project spawn script",
            &[
                opt_s("project", Some('p'), "Project id or name"),
                opt_s(
                    "script",
                    None,
                    "Set the script; omit to read. Empty clears.",
                ),
            ],
        ),
        method(
            "workspace.list",
            "List workspaces",
            &[opt_s("project", Some('p'), "Project id or name")],
        ),
        method_ms(
            "workspace.spawn",
            "Create a sibling worktree and open a pane",
            &[
                req_s("project", Some('p'), "Project id or name"),
                req_s("name", Some('n'), "Rail + .worktrees/ slug"),
                opt_s("branch", Some('b'), "Git branch (default: same as name)"),
                opt_s("base-branch", None, "Start-point (default: HEAD)"),
                opt_s("title", None, "Rail subtitle (name · title)"),
                opt_s("agent", Some('a'), "Only grok in v1"),
                opt_s("prompt", None, "First-turn prompt (implies grok)"),
                opt(
                    "prompt-file",
                    None,
                    ArgType::Path,
                    "Read prompt from this file",
                ),
                opt_s("parent", None, "Parent workspace, pane, or path"),
                flag(
                    "select",
                    's',
                    "Focus the new workspace (CLI default is background)",
                ),
            ],
            SPAWN_TIMEOUT_MS,
        ),
        method(
            "workspace.rm",
            "Unregister a workspace and kill its tmux session",
            &[
                req_s("workspace", Some('w'), "Workspace id or name"),
                arg(
                    "worktree",
                    false,
                    ArgType::Bool,
                    None,
                    "Also git worktree remove the checkout",
                ),
                flag(
                    "force",
                    'f',
                    "Force git worktree remove (dirty / toss). Needs --worktree.",
                ),
            ],
        ),
        method(
            "workspace.select",
            "Focus a workspace in the rail and attach",
            &[req_s("workspace", Some('w'), "Workspace id or name")],
        ),
        method(
            "workspace.set",
            "Set workspace name, title, and/or branch",
            &[
                req_s("workspace", Some('w'), "Workspace id or name"),
                opt_s(
                    "name",
                    Some('n'),
                    "Rail slug; also git worktree move to .worktrees/<name>",
                ),
                opt_s("title", None, "Rail subtitle; empty clears"),
                opt_s(
                    "branch",
                    Some('b'),
                    "Rename this checkout's git branch (does not move the folder)",
                ),
            ],
        ),
        method_ms(
            "workspace.exec",
            "Start or brief Grok in an existing workspace",
            &[
                req_s("workspace", Some('w'), "Workspace id or name"),
                opt_s("agent", Some('a'), "Only grok in v1"),
                opt_s("prompt", None, "Prompt to send or pass as argv"),
                opt(
                    "prompt-file",
                    None,
                    ArgType::Path,
                    "Read prompt from this file",
                ),
            ],
            SPAWN_TIMEOUT_MS,
        ),
        method(
            "pane.list",
            "List panes in a workspace",
            &[opt_s("workspace", Some('w'), "Workspace id or name")],
        ),
        method(
            "pane.send",
            "Paste into a pane (bracketed; Enter optional)",
            &[
                opt_s("pane", None, "Workspace / pane id"),
                req_s("text", Some('t'), "Text to paste"),
                flag("enter", 'e', "Submit after the paste"),
            ],
        ),
        method(
            "pane.read",
            "Read pane scrollback",
            &[
                opt_s("pane", None, "Workspace / pane id"),
                opt("lines", Some('l'), ArgType::Int, "Last N lines"),
            ],
        ),
        method_ms(
            "pane.wait",
            "Wait until a pane reaches a status",
            &[
                opt_s("pane", None, "Workspace / pane id"),
                opt_s(
                    "status",
                    Some('s'),
                    "working|waiting|done|idle (default done)",
                ),
                opt(
                    "timeout",
                    None,
                    ArgType::Int,
                    "Seconds to wait (default 300)",
                ),
                flag("fresh", 'f', "Wait for a transition onto that status"),
            ],
            WAIT_TIMEOUT_MS,
        ),
        method(
            "whoami",
            "Resolve this pane / path to a workspace",
            &[
                opt_s(
                    "pane",
                    None,
                    "Pane or workspace id (default: $SOLA_PANE_ID)",
                ),
                opt(
                    "path",
                    None,
                    ArgType::Path,
                    "Checkout path (default: $SOLA_WS_PATH)",
                ),
            ],
        ),
    ]
}

fn method(name: &str, summary: &str, args: &[ArgSpec]) -> MethodSpec {
    MethodSpec {
        name: name.into(),
        summary: summary.into(),
        args: args.to_vec(),
        timeout_ms: None,
    }
}

fn method_ms(name: &str, summary: &str, args: &[ArgSpec], timeout_ms: u64) -> MethodSpec {
    MethodSpec {
        name: name.into(),
        summary: summary.into(),
        args: args.to_vec(),
        timeout_ms: Some(timeout_ms),
    }
}

fn req_s(name: &str, short: Option<char>, help: &str) -> ArgSpec {
    arg(name, true, ArgType::String, short, help)
}

fn opt_s(name: &str, short: Option<char>, help: &str) -> ArgSpec {
    arg(name, false, ArgType::String, short, help)
}

fn req(name: &str, short: Option<char>, ty: ArgType, help: &str) -> ArgSpec {
    arg(name, true, ty, short, help)
}

fn opt(name: &str, short: Option<char>, ty: ArgType, help: &str) -> ArgSpec {
    arg(name, false, ty, short, help)
}

fn flag(name: &str, short: char, help: &str) -> ArgSpec {
    arg(name, false, ArgType::Bool, Some(short), help)
}

fn arg(name: &str, required: bool, ty: ArgType, short: Option<char>, help: &str) -> ArgSpec {
    ArgSpec {
        name: name.into(),
        long: Some(name.into()),
        short,
        ty,
        required,
        help: help.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_control_plane() {
        let methods = methods();
        let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
        for need in [
            "ps",
            "project.list",
            "project.add",
            "project.rm",
            "project.startup",
            "workspace.list",
            "workspace.spawn",
            "workspace.rm",
            "workspace.select",
            "workspace.set",
            "workspace.exec",
            "pane.list",
            "pane.send",
            "pane.read",
            "pane.wait",
            "whoami",
        ] {
            assert!(names.contains(&need), "missing {need}");
        }
        let spawn = methods
            .iter()
            .find(|m| m.name == "workspace.spawn")
            .unwrap();
        assert_eq!(spawn.timeout_ms, Some(SPAWN_TIMEOUT_MS));
        assert!(spawn.args.iter().any(|a| a.name == "prompt-file"));
        assert!(spawn.args.iter().any(|a| a.name == "base-branch"));
        assert!(spawn.args.iter().any(|a| a.name == "branch"));
        let select = spawn.args.iter().find(|a| a.name == "select").unwrap();
        assert!(matches!(select.ty, ArgType::Bool));
        assert!(!select.required);
        let rm = methods.iter().find(|m| m.name == "workspace.rm").unwrap();
        assert!(rm.args.iter().any(|a| a.name == "worktree"));
        let force = rm.args.iter().find(|a| a.name == "force").unwrap();
        assert!(matches!(force.ty, ArgType::Bool));
        assert!(!force.required);
        let set = methods.iter().find(|m| m.name == "workspace.set").unwrap();
        assert!(set.args.iter().any(|a| a.name == "name"));
        assert!(set.args.iter().any(|a| a.name == "title"));
        assert!(set.args.iter().any(|a| a.name == "branch"));
    }
}
