//! Detect which Claude CLI sessions are currently live in a terminal.
//!
//! A running `claude` CLI process holds open file descriptors on
//! `~/.claude/tasks/<session-id>/`. Scanning `/proc/*/fd/*` for symlinks
//! pointing into that directory yields the set of live session IDs.

use std::collections::HashSet;
use std::fs;

/// Returns the set of session IDs currently active in a terminal claude process.
/// Linux-only (relies on /proc); returns an empty set on failure.
pub fn detect() -> HashSet<String> {
    let mut ids = HashSet::new();
    let Ok(home) = std::env::var("HOME") else { return ids };
    let prefix = format!("{home}/.claude/tasks/");

    let Ok(proc_dir) = fs::read_dir("/proc") else { return ids };
    for proc_entry in proc_dir.flatten() {
        let name = proc_entry.file_name();
        let Some(pid) = name.to_str() else { continue };
        if !pid.bytes().all(|b| b.is_ascii_digit()) { continue; }

        let fd_dir = proc_entry.path().join("fd");
        let Ok(fds) = fs::read_dir(&fd_dir) else { continue };
        for fd in fds.flatten() {
            let Ok(target) = fs::read_link(fd.path()) else { continue };
            let Some(target_str) = target.to_str() else { continue };
            let Some(rest) = target_str.strip_prefix(&prefix) else { continue };
            // rest looks like "<uuid>" or "<uuid>/.lock" or "<uuid>/1.json (deleted)"
            let uuid = rest.split('/').next().unwrap_or("").trim_end_matches(" (deleted)");
            if !uuid.is_empty() {
                ids.insert(uuid.to_string());
            }
        }
    }

    ids
}
