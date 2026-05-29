//! /proc + PATH-based binary resolution for the "running but not
//! configured" candidate list. Pure leaf module — no bus, no UI,
//! no app state. Lifted from the legacy main.rs unchanged; the
//! original docstrings remain authoritative for behaviour.

use std::path::Path;

use sola_core::applications::resolve_in_path;

/// App IDs that are part of Sola itself and should never appear as
/// "running, not configured" candidates. Kept local to sola-settings
/// (rather than imported from sola-core) so editing the launcher
/// builtin list — which lives in sola-shell — doesn't have to also
/// rebuild this crate. New first-party apps go in both places.
const SYSTEM_APP_IDS: &[&str] = &[
    "sola-shell",
    "sola-settings",
    "sola-monitor",
    "sola-terminal",
    "sola-browser",
    "sola-kit",
    "sola-kit-legacy",
];

pub fn is_system_app(app_id: &str) -> bool {
    SYSTEM_APP_IDS.contains(&app_id)
}

/// Best-effort suggestion of a launch command for a window we just
/// noticed. See module docstring on the legacy version for the full
/// rationale; behaviour preserved exactly.
pub fn suggest_command(app_id: &str, pid: Option<u32>) -> Option<String> {
    if let Some(path) = resolve_from_app_id(app_id) {
        return Some(path);
    }
    pid.and_then(resolve_binary_for_pid)
}

fn resolve_from_app_id(app_id: &str) -> Option<String> {
    let trimmed = app_id.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut tried: Vec<String> = Vec::new();
    let try_name = |name: &str, tried: &mut Vec<String>| -> Option<String> {
        if name.is_empty() || tried.iter().any(|t| t == name) {
            return None;
        }
        tried.push(name.to_string());
        resolve_in_path(name).map(|p| p.to_string_lossy().into_owned())
    };

    if let Some(hit) = try_name(&trimmed.to_ascii_lowercase(), &mut tried) {
        return Some(hit);
    }
    let segments: Vec<&str> = trimmed.split('.').collect();
    if segments.len() > 1 {
        let last = segments[segments.len() - 1].to_ascii_lowercase();
        if let Some(hit) = try_name(&last, &mut tried) {
            return Some(hit);
        }
        let second = segments[segments.len() - 2].to_ascii_lowercase();
        if let Some(hit) = try_name(&second, &mut tried) {
            return Some(hit);
        }
    }
    None
}

fn resolve_binary_for_pid(pid: u32) -> Option<String> {
    let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok();
    let cleaned = exe.map(|p| {
        let s = p.to_string_lossy().into_owned();
        s.strip_suffix(" (deleted)")
            .map(str::to_string)
            .unwrap_or(s)
    });

    let file_name = cleaned.as_deref().and_then(|c| {
        Path::new(c)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
    });

    let need_cmdline = file_name.as_deref().is_none_or(is_multi_arg_launcher);
    if need_cmdline {
        return cmdline_positional(pid);
    }
    cleaned
}

fn is_multi_arg_launcher(name: &str) -> bool {
    matches!(
        name,
        "bwrap"
            | "flatpak-spawn"
            | "flatpak"
            | "AppRun"
            | "snap"
            | "snap-confine"
            | "electron"
    )
}

fn cmdline_positional(pid: u32) -> Option<String> {
    let data = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let parts: Vec<&[u8]> = data
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return None;
    }
    let mut take = 1;
    for arg in &parts[1..] {
        if arg.first() == Some(&b'-') {
            break;
        }
        take += 1;
    }
    let joined: Vec<String> = parts[..take]
        .iter()
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    Some(joined.join(" "))
}

