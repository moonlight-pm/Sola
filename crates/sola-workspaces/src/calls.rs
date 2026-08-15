//! Advertised sola-call methods for owner `ws`.

use sola_call::{ArgSpec, ArgType, MethodSpec};

pub const OWNER: &str = "ws";

pub fn methods() -> Vec<MethodSpec> {
    vec![
        method("ps", "Project → workspace → state table", &[]),
        method("project.list", "List projects", &[]),
        method(
            "workspace.list",
            "List workspaces",
            &[opt("project", Some('p'), "Project id or name")],
        ),
        method(
            "workspace.spawn",
            "Create a sibling worktree and open a pane",
            &[
                req("project", Some('p'), "Project id or name"),
                req("name", Some('n'), "Worktree / branch name"),
                opt("agent", Some('a'), "Only grok in v1"),
                opt("prompt", None, "First-turn prompt (implies grok)"),
                opt("parent", None, "Parent workspace id (default: main)"),
            ],
        ),
        method(
            "workspace.rm",
            "Unregister a workspace and kill its tmux session",
            &[req("workspace", Some('w'), "Workspace id or name")],
        ),
        method(
            "pane.list",
            "List panes in a workspace",
            &[opt("workspace", Some('w'), "Workspace id or name")],
        ),
        method(
            "pane.send",
            "Type into a pane",
            &[
                opt("pane", None, "Workspace / pane id"),
                req("text", Some('t'), "Text to type"),
                flag("enter", 'e', "Send Enter after the text"),
            ],
        ),
        method(
            "pane.read",
            "Read pane scrollback",
            &[
                opt("pane", None, "Workspace / pane id"),
                opt_int("lines", 'l', "Last N lines"),
            ],
        ),
    ]
}

fn method(name: &str, summary: &str, args: &[ArgSpec]) -> MethodSpec {
    MethodSpec {
        name: name.into(),
        summary: summary.into(),
        args: args.to_vec(),
    }
}

fn req(name: &str, short: Option<char>, help: &str) -> ArgSpec {
    arg(name, true, ArgType::String, short, help)
}

fn opt(name: &str, short: Option<char>, help: &str) -> ArgSpec {
    arg(name, false, ArgType::String, short, help)
}

fn opt_int(name: &str, short: char, help: &str) -> ArgSpec {
    arg(name, false, ArgType::Int, Some(short), help)
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
