//! Grok binary version + update check (`grok update --check --json`).

use std::process::Command;
use std::time::Duration;

use serde::Deserialize;

use crate::backend;
use crate::bridge;
use crate::protocol::AgentEvent;

#[derive(Debug, Clone, Default)]
pub struct GrokVersionInfo {
    pub current: Option<String>,
    pub latest: Option<String>,
    pub update_available: bool,
    pub channel: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckJson {
    current_version: Option<String>,
    latest_version: Option<String>,
    update_available: Option<bool>,
    channel: Option<String>,
}

/// Run `grok update --check --json` (best-effort).
pub fn check_update() -> GrokVersionInfo {
    let grok = backend::resolve_grok_binary();
    let out = Command::new(&grok)
        .args(["update", "--check", "--json"])
        .output();
    let Ok(out) = out else {
        return GrokVersionInfo::default();
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    // JSON may be mixed with log lines — take the last object-looking line.
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))
        .unwrap_or(stdout.trim());
    match serde_json::from_str::<CheckJson>(line) {
        Ok(j) => GrokVersionInfo {
            current: j.current_version,
            latest: j.latest_version,
            update_available: j.update_available.unwrap_or(false),
            channel: j.channel,
        },
        Err(_) => {
            // Fallback: parse `grok --version` → "grok 0.2.112 (...)"
            version_from_cli()
        }
    }
}

pub fn version_from_cli() -> GrokVersionInfo {
    let grok = backend::resolve_grok_binary();
    let out = Command::new(&grok).arg("--version").output();
    let Ok(out) = out else {
        return GrokVersionInfo::default();
    };
    let s = String::from_utf8_lossy(&out.stdout);
    // "grok 0.2.112 (9bbd559437) [stable]"
    let current = s
        .split_whitespace()
        .nth(1)
        .map(|v| v.to_string());
    GrokVersionInfo {
        current,
        latest: None,
        update_available: false,
        channel: None,
    }
}

/// Background refresh loop — emits `AgentEvent::GrokVersion` periodically.
pub fn start_update_watcher() {
    std::thread::Builder::new()
        .name("sola-agent-grok-ver".into())
        .spawn(|| {
            // Immediate check so the footer paints soon after connect.
            emit_check();
            loop {
                std::thread::sleep(Duration::from_secs(15 * 60));
                emit_check();
            }
        })
        .ok();
}

fn emit_check() {
    let info = check_update();
    bridge::emit(AgentEvent::GrokVersion {
        current: info.current,
        latest: info.latest,
        update_available: info.update_available,
        channel: info.channel,
    });
}
