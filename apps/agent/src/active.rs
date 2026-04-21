//! Detect which Claude CLI sessions are currently live in a terminal.
//!
//! The `claude` CLI writes a per-process marker at
//! `~/.claude/sessions/<pid>.json` with the session_id and a `kind`
//! discriminator. Interactive terminal sessions use `kind: "interactive"`;
//! agent-spawned ones (stream-json) are different. We collect the session
//! IDs from interactive markers whose PID is still alive.

use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
struct SessionMarker {
    pid: u32,
    #[serde(rename = "sessionId")]
    session_id: String,
    kind: String,
    /// "cli" for terminal sessions, "sdk-cli" for stream-json spawns
    /// (that's our own agent-spawned processes). Filter those out so
    /// we don't mark our own live turn as a read-only terminal session.
    #[serde(default)]
    entrypoint: String,
}

/// Returns the set of session IDs currently active in a terminal claude process.
/// Linux-only (relies on /proc for liveness); returns an empty set on failure.
pub fn detect() -> HashSet<String> {
    let mut ids = HashSet::new();
    let Ok(home) = std::env::var("HOME") else { return ids };
    let dir = format!("{home}/.claude/sessions");

    let Ok(entries) = fs::read_dir(&dir) else { return ids };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") { continue; }

        let Ok(json) = fs::read_to_string(&path) else { continue };
        let Ok(marker) = serde_json::from_str::<SessionMarker>(&json) else { continue };

        if marker.kind != "interactive" { continue; }
        if marker.entrypoint == "sdk-cli" { continue; }
        if !Path::new(&format!("/proc/{}", marker.pid)).exists() { continue; }

        ids.insert(marker.session_id);
    }
    ids
}
