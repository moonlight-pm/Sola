//! Advertised sola-call methods for owner `projects`.
//!
//! First-class surface: keep this list, dispatch, tests, and
//! `docs/manual/solactl.md` in the same change.
//! Freeze: `docs/specs/2026-08-18-workspaces-cli-design.md`.

use sola_call::{ArgSpec, ArgType, MethodSpec};

pub const OWNER: &str = "projects";

pub const SPAWN_TIMEOUT_MS: u64 = 60_000;
pub const ADD_TIMEOUT_MS: u64 = 15_000;
pub const WAIT_TIMEOUT_MS: u64 = 302_000;
pub const WAIT_DEFAULT_SECS: u64 = 300;

pub fn methods() -> Vec<MethodSpec> {
    vec![
        method("ps", "Project → session → state table (the sessions open here)", &[]),
        method("project.list", "List projects", &[]),
        method(
            "window.navigate",
            "Open a Projects window to pick a project in (the launcher's Projects); Esc closes it",
            &[],
        ),
        method_ms(
            "project.open",
            "Open a project's sessions here, each in its own window: those running on its computer (whichever computer started them), else a new main session; a folder that is not a project yet is added first. \
             With --host, the folder is on that computer: its sessions run there, in tmux over ssh",
            &[
                req(
                    "path",
                    Some('p'),
                    ArgType::Path,
                    "Folder path (`~` ok); with --host, its full path there",
                ),
                opt_s("host", None, "The computer it is on (an ssh host); omit for this one"),
                opt_s("session", None, "Open this session alone (its id there, e.g. ws-main)"),
            ],
            ADD_TIMEOUT_MS,
        ),
        method(
            "project.close",
            "Close a project's windows here. Its sessions keep running where they are (tab.rm ends one)",
            &[req_s("project", Some('p'), "Project id or name")],
        ),
        method(
            "tab.list",
            "List the sessions open here",
            &[opt_s("project", Some('p'), "Project id or name")],
        ),
        method_ms(
            "tab.spawn",
            "Start a session in a project's folder, in its own window (Projects makes no worktree: a session makes its own when its work needs one)",
            &[
                req_s("project", Some('p'), "Project id or name"),
                opt_s("name", Some('n'), "Session name, unique in its project here (default: the next number)"),
                opt_s("title", None, "Label (default: name)"),
                opt_s("agent", Some('a'), "grok, codex, or claude"),
                opt_s("prompt", None, "First-turn prompt (implies grok)"),
                opt(
                    "prompt-file",
                    None,
                    ArgType::Path,
                    "Read prompt from this file",
                ),
                opt_s("parent", None, "Parent session, pane, or path"),
                flag(
                    "select",
                    's',
                    "Bring its window up (CLI default is behind)",
                ),
            ],
            SPAWN_TIMEOUT_MS,
        ),
        method(
            "tab.rm",
            "End a session: its tmux killed where it runs, for every computer watching it. Its folder and worktrees stay",
            &[req_s("tab", Some('t'), "Session id or name")],
        ),
        method(
            "tab.select",
            "Bring a session's window up",
            &[req_s("tab", Some('t'), "Session id or name")],
        ),
        method(
            "tab.set",
            "Set a session's name and/or title",
            &[
                req_s("tab", Some('t'), "Session id or name"),
                opt_s("name", Some('n'), "Session name, unique in its project here"),
                opt_s("title", None, "Label; empty clears (falls back to name)"),
            ],
        ),
        method_ms(
            "tab.exec",
            "Start or brief Grok, Codex, or Claude in an existing session",
            &[
                req_s("tab", Some('t'), "Session id or name"),
                opt_s("agent", Some('a'), "grok, codex, or claude (default grok)"),
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
            "List panes in a tab",
            &[opt_s("tab", Some('t'), "Tab id or name")],
        ),
        method(
            "pane.send",
            "Paste into a pane (bracketed; Enter optional)",
            &[
                opt_s("pane", None, "Tab / pane id"),
                req_s("text", Some('t'), "Text to paste"),
                flag("enter", 'e', "Submit after the paste"),
            ],
        ),
        method(
            "pane.read",
            "Read pane scrollback",
            &[
                opt_s("pane", None, "Tab / pane id"),
                opt("lines", Some('l'), ArgType::Int, "Last N lines"),
            ],
        ),
        method_ms(
            "pane.wait",
            "Wait until a pane reaches a status",
            &[
                opt_s("pane", None, "Tab / pane id"),
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
            "Resolve this pane / path to a tab",
            &[
                opt_s(
                    "pane",
                    None,
                    "Pane or tab id (default: $SOLA_PANE_ID)",
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
            "project.open",
            "window.navigate",
            "project.close",
            "tab.list",
            "tab.spawn",
            "tab.rm",
            "tab.select",
            "tab.set",
            "tab.exec",
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
            .find(|m| m.name == "tab.spawn")
            .unwrap();
        assert_eq!(spawn.timeout_ms, Some(SPAWN_TIMEOUT_MS));
        assert!(spawn.args.iter().any(|a| a.name == "prompt-file"));
        // Projects makes no worktrees (sessions anywhere, 2026-10-30).
        for gone in ["root", "branch", "base-branch"] {
            assert!(spawn.args.iter().all(|a| a.name != gone), "{gone}");
        }
        assert!(spawn.args.iter().any(|a| a.name == "name" && !a.required));
        let select = spawn.args.iter().find(|a| a.name == "select").unwrap();
        assert!(matches!(select.ty, ArgType::Bool));
        assert!(!select.required);
        let rm = methods.iter().find(|m| m.name == "tab.rm").unwrap();
        assert_eq!(rm.args.len(), 1);
        let set = methods.iter().find(|m| m.name == "tab.set").unwrap();
        let names: Vec<&str> = set.args.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["tab", "name", "title"]);
    }
}
