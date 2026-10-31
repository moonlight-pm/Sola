//! Process-tree presence. Tells us *who* is in the pane, not state.
//!
//! Grok is preferred when several agents share a tree. OpenCode is
//! presence-only; Grok, Codex, and Claude also get hooks.

use sola_terminal::tmux;
use std::collections::{HashMap, VecDeque};
use std::fs;

use crate::machine::Machine;

/// Known agent binaries, Grok first.
pub const AGENTS: &[&str] = &["grok", "claude", "codex", "opencode"];

/// Who is live in the pane. `Unknown` means the walk failed — do not
/// treat that as “back to shell” or a live agent will flicker idle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Unknown,
    Shell,
    Agent(&'static str),
}

/// Walk the tmux pane's descendants. Prefer Grok if both are present.
pub fn scan_session(tmux_session: &str) -> Presence {
    let Some(pid) = tmux::pane_pid(tmux_session) else {
        return Presence::Unknown;
    };
    match scan_pid(pid) {
        Some(name) => Presence::Agent(name),
        None => Presence::Shell,
    }
}

pub fn scan_pid(root: i32) -> Option<&'static str> {
    scan_tree(root, agent_from_proc, children_of)
}

/// The agent in the process tree under `root` (Grok first when several).
fn scan_tree(
    root: i32,
    agent_of: impl Fn(i32) -> Option<&'static str>,
    children: impl Fn(i32) -> Vec<i32>,
) -> Option<&'static str> {
    let mut found = Vec::new();
    let mut q = VecDeque::from([root]);
    let mut seen = std::collections::HashSet::new();
    while let Some(pid) = q.pop_front() {
        if !seen.insert(pid) {
            continue;
        }
        if let Some(name) = agent_of(pid) {
            if !found.contains(&name) {
                found.push(name);
            }
        }
        for child in children(pid) {
            q.push_back(child);
        }
    }
    if found.contains(&"grok") {
        return Some("grok");
    }
    found.into_iter().next()
}

/// [`scan_session`] for each of `sessions` on another computer: their
/// panes' pids and its process table, in one look.
pub fn scan_remote(m: &Machine, sessions: &[String]) -> HashMap<String, Presence> {
    let pick = sola_terminal::remote::pick("\"=$s\"");
    let script = format!(
        "for s in \"$@\"; do {pick} p=$(tmux -L \"$L\" display-message -p -t \"=$s:\" \"#{{pane_pid}}\" 2>/dev/null) \
         && echo \"$s $p\"; done; echo --; ps -eo pid=,ppid=,comm=,args="
    );
    let mut argv = vec!["sh", "-c", script.as_str(), "sh"];
    argv.extend(sessions.iter().map(String::as_str));
    let out = match m.run(None, &[], &argv, None) {
        Ok(o) if o.status.success() => o.stdout,
        _ => return HashMap::new(),
    };
    parse_remote_scan(&String::from_utf8_lossy(&out))
}

/// `<session> <pid>` lines, `--`, then `ps -eo pid=,ppid=,comm=,args=`.
fn parse_remote_scan(text: &str) -> HashMap<String, Presence> {
    let (panes, table) = text.split_once("\n--\n").unwrap_or((text, ""));
    let mut parent: HashMap<i32, Vec<i32>> = HashMap::new();
    let mut agent: HashMap<i32, &'static str> = HashMap::new();
    for line in table.lines() {
        let mut f = line.split_whitespace();
        let (Some(pid), Some(ppid), Some(comm)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let (Ok(pid), Ok(ppid)) = (pid.parse::<i32>(), ppid.parse::<i32>()) else {
            continue;
        };
        parent.entry(ppid).or_default().push(pid);
        let named = agent_from_name(comm).or_else(|| f.next().and_then(agent_from_name));
        if let Some(a) = named {
            agent.insert(pid, a);
        }
    }
    panes
        .lines()
        .filter_map(|line| {
            let (session, pid) = line.split_once(' ')?;
            let pid: i32 = pid.trim().parse().ok()?;
            let who = scan_tree(
                pid,
                |p| agent.get(&p).copied(),
                |p| parent.get(&p).cloned().unwrap_or_default(),
            );
            Some((session.to_string(), who.map_or(Presence::Shell, Presence::Agent)))
        })
        .collect()
}

pub fn agent_from_name(name: &str) -> Option<&'static str> {
    let base = name.rsplit('/').next().unwrap_or(name);
    if base == "grok" || base.starts_with("grok-") {
        return Some("grok");
    }
    for &agent in &AGENTS[1..] {
        if base == agent || base.starts_with(&format!("{agent}-")) {
            return Some(agent);
        }
    }
    None
}

fn agent_from_proc(pid: i32) -> Option<&'static str> {
    let comm = fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    if let Some(a) = agent_from_name(comm.trim()) {
        return Some(a);
    }
    let exe = fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    agent_from_name(&exe.to_string_lossy())
}

fn children_of(pid: i32) -> Vec<i32> {
    let path = format!("/proc/{pid}/task/{pid}/children");
    if let Ok(text) = fs::read_to_string(&path) {
        return text
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
    }
    children_by_ppid_scan(pid)
}

fn children_by_ppid_scan(ppid: i32) -> Vec<i32> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ent in entries.flatten() {
        let pid: i32 = match ent.file_name().to_string_lossy().parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let status = match fs::read_to_string(ent.path().join("status")) {
            Ok(s) => s,
            Err(_) => continue,
        };
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("PPid:") {
                if rest.trim().parse::<i32>() == Ok(ppid) {
                    out.push(pid);
                }
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_scan_finds_the_agent_under_each_pane() {
        let text = "sws-a 100\nsws-b 200\n--\n\
            100 1 zsh -zsh\n\
            101 100 node /usr/bin/claude --resume x\n\
            200 1 zsh -zsh\n\
            201 200 vim vim notes\n\
            300 1 grok grok\n";
        let found = parse_remote_scan(text);
        assert_eq!(found["sws-a"], Presence::Agent("claude"));
        assert_eq!(found["sws-b"], Presence::Shell);
        assert!(!found.contains_key("sws-c"));
    }

    #[test]
    fn a_remote_scan_runs_here() {
        // No such tmux sessions: the table still comes, the panes do not.
        let found = scan_remote(&Machine::Local, &["sws-no-such-session".into()]);
        assert!(found.is_empty());
    }

    #[test]
    fn grok_is_first_and_matches_prefix() {
        assert_eq!(agent_from_name("grok"), Some("grok"));
        assert_eq!(agent_from_name("/opt/sola/bin/grok"), Some("grok"));
        assert_eq!(agent_from_name("grok-1.0.3-linux-x86_64"), Some("grok"));
        assert_eq!(agent_from_name("claude"), Some("claude"));
        assert_eq!(agent_from_name("codex"), Some("codex"));
        assert_eq!(agent_from_name("codex-0.50.0"), Some("codex"));
        assert_eq!(agent_from_name("bash"), None);
        assert_eq!(AGENTS[0], "grok");
    }
}
