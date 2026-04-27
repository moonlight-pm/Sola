//! Screenshot handler. Delegates to `grim`, a small wlroots screenshot
//! tool. Region targeting (per-window) uses `grim -g "X,Y WxH"` and
//! the geometry tracked by `WindowRegistry` from inbound `Frame`
//! topics — so the rect captured is whatever the shell most recently
//! placed the window at.
//!
//! Caveat: region capture takes whatever is visually at those screen
//! coordinates. If another window overlaps the target, the screenshot
//! will include that overlap. Callers can `Topic::Focus` or otherwise
//! raise the window first if a clean capture is required.
//!
//! Requires `grim` on PATH. NixOS: add `pkgs.grim` to
//! `environment.systemPackages`.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use sola_bus::topics::{
    CaptureScreenPayload, CaptureTarget, ScreenshotPayload, Topic,
};

use crate::bus::BusClient;
use crate::client::AppData;

const SCREENSHOT_DIR: &str = "/tmp/sola/screenshots";

pub fn handle(state: &mut AppData, req: CaptureScreenPayload) {
    let path = req.path.unwrap_or_else(default_path);

    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!(path = %path.display(), %e, "failed to create screenshot dir");
        }
    }

    let mut cmd = Command::new("grim");
    match &req.target {
        CaptureTarget::FullOutput => {}
        CaptureTarget::Window { app_id, title } => {
            let entry = state.registry.find_by_app_title(app_id, title.as_deref());
            let Some(entry) = entry else {
                let known: Vec<String> = state
                    .registry
                    .as_windows()
                    .iter()
                    .map(|w| format!("{}/{}", w.app_id, w.title))
                    .collect();
                emit_err(
                    &mut state.bus,
                    format!(
                        "no window for app_id={app_id:?} title={title:?} (known: {})",
                        known.join(", "),
                    ),
                );
                return;
            };
            let Some((x, y, w, h)) = entry.frame else {
                emit_err(
                    &mut state.bus,
                    format!("window {app_id:?} has no recorded frame yet"),
                );
                return;
            };
            cmd.arg("-g").arg(format!("{x},{y} {w}x{h}"));
        }
    }
    cmd.arg(&path);

    let result = match cmd.output() {
        Ok(out) if out.status.success() => {
            tracing::info!(path = %path.display(), target = ?req.target, "screenshot saved");
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

    state
        .bus
        .emit(Topic::Screenshot(ScreenshotPayload { result }));
}

fn emit_err(bus: &mut BusClient, msg: String) {
    bus.emit(Topic::Screenshot(ScreenshotPayload { result: Err(msg) }));
}

fn default_path() -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    PathBuf::from(format!("{SCREENSHOT_DIR}/{ts}.png"))
}
