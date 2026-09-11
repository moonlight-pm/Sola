//! Advertised sola-call methods for owner `browser`.
//!
//! First-class surface: keep this list, dispatch, tests, and
//! `docs/manual/solactl.md` in the same change.
//! Freeze: `docs/specs/2026-09-11-sola-browser-agent-control-design.md`.

use sola_call::{ArgSpec, ArgType, MethodSpec};

pub const OWNER: &str = "browser";

pub const SNAPSHOT_TIMEOUT_MS: u64 = 15_000;
pub const PAGE_TIMEOUT_MS: u64 = 15_000;
pub const WAIT_TIMEOUT_MS: u64 = 32_000;
pub const WAIT_DEFAULT_SECS: u64 = 30;

pub fn methods() -> Vec<MethodSpec> {
    vec![
        method("tabs", "List tabs (id, url, title, group, active)", &[]),
        method(
            "tab.open",
            "Open a tab (does not focus unless --select)",
            &[
                req_s("url", Some('u'), "URL to load"),
                opt_s("group", Some('g'), "Existing group id or name"),
                flag("select", 's', "Focus the new tab (default: background)"),
            ],
        ),
        method(
            "tab.close",
            "Close a tab (never drops the last tab)",
            &[opt_s("tab", Some('t'), "Tab id, url, or title (default: focused)")],
        ),
        method(
            "tab.focus",
            "Focus a tab (the only seat steal besides --select)",
            &[req_s("tab", Some('t'), "Tab id, url, or title")],
        ),
        method(
            "tab.move",
            "Move a tab into a group; omit --group to ungroup",
            &[
                req_s("tab", Some('t'), "Tab id, url, or title"),
                opt_s("group", Some('g'), "Group id or name"),
            ],
        ),
        method(
            "group.list",
            "List tab groups",
            &[],
        ),
        method(
            "group.create",
            "Wrap a listed tab in a new group (ordinary pocket)",
            &[
                req_s("tab", Some('t'), "Tab id, url, or title"),
                opt_s("name", Some('n'), "Group name (default: Group)"),
            ],
        ),
        method(
            "group.rename",
            "Rename a group",
            &[
                req_s("group", Some('g'), "Group id or name"),
                req_s("name", Some('n'), "New name"),
            ],
        ),
        method(
            "group.recolor",
            "Set group pocket color (`#rrggbb`); empty clears",
            &[
                req_s("group", Some('g'), "Group id or name"),
                opt_s("color", Some('c'), "Hex color; omit/empty = default well"),
            ],
        ),
        method(
            "group.collapse",
            "Collapse or expand a group",
            &[
                req_s("group", Some('g'), "Group id or name"),
                flag("collapsed", 'c', "Collapse (omit to expand)"),
            ],
        ),
        method_ms(
            "goto",
            "Navigate a tab (does not focus)",
            &[
                req_s("url", Some('u'), "URL to load"),
                opt_s("tab", Some('t'), "Tab id, url, or title (default: focused)"),
                flag("select", 's', "Focus the tab after navigate"),
            ],
            PAGE_TIMEOUT_MS,
        ),
        method(
            "back",
            "History back",
            &[opt_s("tab", Some('t'), "Tab id, url, or title (default: focused)")],
        ),
        method(
            "forward",
            "History forward",
            &[opt_s("tab", Some('t'), "Tab id, url, or title (default: focused)")],
        ),
        method(
            "reload",
            "Reload",
            &[
                opt_s("tab", Some('t'), "Tab id, url, or title (default: focused)"),
                flag("hard", 'h', "Bypass HTTP cache (⌘⇧R)"),
            ],
        ),
        method(
            "stop",
            "Stop loading",
            &[opt_s("tab", Some('t'), "Tab id, url, or title (default: focused)")],
        ),
        method(
            "find.page",
            "CEF find-in-page (⌘F)",
            &[
                req_s("text", Some('q'), "Query"),
                opt_s("tab", Some('t'), "Tab id, url, or title (default: focused)"),
                flag("back", 'b', "Search backwards"),
            ],
        ),
        method_ms(
            "snapshot",
            "Pruned accessibility YAML + refs for one tab",
            &[
                opt_s("tab", Some('t'), "Tab id, url, or title (default: focused)"),
                opt_s("ref", None, "Subtree ref from the last snapshot"),
                flag("interactive", 'i', "Controls only"),
                flag("json", 'j', "Debug JSON (no backend ids)"),
            ],
            SNAPSHOT_TIMEOUT_MS,
        ),
        method(
            "find",
            "Search the last snapshot YAML",
            &[
                req_s("text", Some('q'), "Substring"),
                opt_s("tab", Some('t'), "Tab whose last snapshot to search"),
            ],
        ),
        method_ms(
            "click",
            "Click a snapshot ref",
            &[
                req_s("ref", Some('r'), "Ref from snapshot (e12 / f1e3)"),
                opt_s("tab", Some('t'), "Tab (default: last snapshot tab)"),
            ],
            PAGE_TIMEOUT_MS,
        ),
        method_ms(
            "hover",
            "Hover a snapshot ref",
            &[
                req_s("ref", Some('r'), "Ref from snapshot"),
                opt_s("tab", Some('t'), "Tab (default: last snapshot tab)"),
            ],
            PAGE_TIMEOUT_MS,
        ),
        method_ms(
            "type",
            "Type into a snapshot ref (appends)",
            &[
                req_s("ref", Some('r'), "Ref from snapshot"),
                req_s("text", Some('x'), "Text to type"),
                opt_s("tab", Some('t'), "Tab (default: last snapshot tab)"),
                flag("submit", 'e', "Press Enter after"),
            ],
            PAGE_TIMEOUT_MS,
        ),
        method_ms(
            "fill",
            "Replace the value of a snapshot ref",
            &[
                req_s("ref", Some('r'), "Ref from snapshot"),
                req_s("text", Some('x'), "New value"),
                opt_s("tab", Some('t'), "Tab (default: last snapshot tab)"),
            ],
            PAGE_TIMEOUT_MS,
        ),
        method_ms(
            "select",
            "Select option(s) on a snapshot ref",
            &[
                req_s("ref", Some('r'), "Ref from snapshot"),
                req_s("values", Some('v'), "Comma-separated option labels or values"),
                opt_s("tab", Some('t'), "Tab (default: last snapshot tab)"),
            ],
            PAGE_TIMEOUT_MS,
        ),
        method_ms(
            "wait",
            "Wait until a tab finishes loading, or text appears",
            &[
                opt_s("tab", Some('t'), "Tab id, url, or title (default: focused)"),
                opt_s("text", Some('q'), "Wait until this appears in a snapshot"),
                flag("load", 'l', "Wait for document load (default when --text is omitted)"),
                opt("timeout", None, ArgType::Int, "Seconds (default 30)"),
            ],
            WAIT_TIMEOUT_MS,
        ),
        method_ms(
            "screenshot",
            "Page PNG when the AX tree is empty (fallback)",
            &[
                opt_s("tab", Some('t'), "Tab id, url, or title (default: focused)"),
                opt("path", Some('o'), ArgType::Path, "Write PNG here (default: cache)"),
            ],
            PAGE_TIMEOUT_MS,
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
            "tabs",
            "tab.open",
            "tab.close",
            "tab.focus",
            "tab.move",
            "group.list",
            "group.create",
            "group.rename",
            "group.recolor",
            "group.collapse",
            "goto",
            "back",
            "forward",
            "reload",
            "stop",
            "find.page",
            "snapshot",
            "find",
            "click",
            "hover",
            "type",
            "fill",
            "select",
            "wait",
            "screenshot",
        ] {
            assert!(names.contains(&need), "missing {need}");
        }
        let open = methods.iter().find(|m| m.name == "tab.open").unwrap();
        let select = open.args.iter().find(|a| a.name == "select").unwrap();
        assert!(matches!(select.ty, ArgType::Bool));
        assert!(!select.required);
        let snap = methods.iter().find(|m| m.name == "snapshot").unwrap();
        assert_eq!(snap.timeout_ms, Some(SNAPSHOT_TIMEOUT_MS));
        assert!(snap.args.iter().any(|a| a.name == "interactive"));
        let wait = methods.iter().find(|m| m.name == "wait").unwrap();
        assert!(wait.args.iter().any(|a| a.name == "load"));
        assert!(matches!(
            wait.args.iter().find(|a| a.name == "load").unwrap().ty,
            ArgType::Bool
        ));
        assert!(!names.iter().any(|n| n.contains("vault")));
    }
}
