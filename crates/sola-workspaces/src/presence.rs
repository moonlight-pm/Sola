//! Process-tree presence. Tells us *who* is in the pane, not state.
//!
//! Grok is first. Other known CLIs are presence-only.

use sola_terminal::tmux;
use std::collections::VecDeque;
use std::fs;

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
    let mut found = Vec::new();
    let mut q = VecDeque::from([root]);
    let mut seen = std::collections::HashSet::new();
    while let Some(pid) = q.pop_front() {
        if !seen.insert(pid) {
            continue;
        }
        if let Some(name) = agent_from_proc(pid) {
            if !found.contains(&name) {
                found.push(name);
            }
        }
        for child in children_of(pid) {
            q.push_back(child);
        }
    }
    if found.contains(&"grok") {
        return Some("grok");
    }
    found.into_iter().next()
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
    fn grok_is_first_and_matches_prefix() {
        assert_eq!(agent_from_name("grok"), Some("grok"));
        assert_eq!(agent_from_name("/opt/sola/bin/grok"), Some("grok"));
        assert_eq!(agent_from_name("grok-1.0.3-linux-x86_64"), Some("grok"));
        assert_eq!(agent_from_name("claude"), Some("claude"));
        assert_eq!(agent_from_name("bash"), None);
        assert_eq!(AGENTS[0], "grok");
    }
}
