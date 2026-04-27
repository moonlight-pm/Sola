//! Screenshot handler. Delegates to `grim`, a small wlroots screenshot
//! tool that already speaks `wlr-screencopy-unstable-v1`. Spawning a
//! process is the cheapest way to get screenshot capability into Sola
//! without vendoring screencopy bindings, allocating SHM, and PNG-
//! encoding ourselves. A future revision may replace this with native
//! `wlr-screencopy` code; the bus protocol stays identical.
//!
//! Requires `grim` on PATH. NixOS users add `pkgs.grim` to
//! `environment.systemPackages`.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use sola_bus::topics::{CaptureScreenPayload, ScreenshotPayload, Topic};

use crate::bus::BusClient;

const SCREENSHOT_DIR: &str = "/tmp/sola/screenshots";

pub fn handle(bus: &mut BusClient, req: CaptureScreenPayload) {
    let path = req.path.unwrap_or_else(default_path);

    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!(path = %path.display(), %e, "failed to create screenshot dir");
        }
    }

    let result = match Command::new("grim").arg(&path).output() {
        Ok(out) if out.status.success() => {
            tracing::info!(path = %path.display(), "screenshot saved");
            Ok(path)
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(format!("grim exited with status {}: {}", out.status, stderr))
        }
        Err(e) => Err(format!(
            "failed to spawn grim ({e}); install grim and try again"
        )),
    };

    bus.emit(Topic::Screenshot(ScreenshotPayload { result }));
}

fn default_path() -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    PathBuf::from(format!("{SCREENSHOT_DIR}/{ts}.png"))
}
