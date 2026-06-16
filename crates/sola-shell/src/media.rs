//! Global media-key actions.
//!
//! When a global `XF86Audio*` chord fires, sola-river delivers it as a
//! `Topic::Chord` — it never reaches a focused window, because River does
//! not deliver bound keys to any surface. `on_chord` recognises the media
//! keysyms (see [`crate::keys::media_action`]) and runs the actual control
//! out-of-process via `solactl media <action>`: MPRIS over D-Bus for the
//! transport keys, `wpctl` for the default-sink mute/volume.
//!
//! Keeping the D-Bus / PipeWire logic in `solactl` keeps it out of the
//! shell's render loop and dependency tree; a per-keypress spawn is cheap
//! for keys this infrequent.

use std::process::Command;

/// Run `solactl media <action>`, detached. The child is reaped on a
/// short-lived thread so the long-running shell never accumulates zombies.
/// Best-effort: a spawn failure is logged, never surfaced to the user.
pub fn trigger(action: &str) {
    let exe = solactl_path();
    match Command::new(&exe).arg("media").arg(action).spawn() {
        Ok(child) => {
            // `solactl media` exits in tens of milliseconds; a detached
            // joiner reaps it without blocking the update loop.
            std::thread::spawn(move || {
                let mut child = child;
                let _ = child.wait();
            });
        }
        Err(e) => {
            tracing::warn!(action, exe = %exe.display(), "failed to spawn solactl media: {e}");
        }
    }
}

/// Resolve the deployed `solactl` next to our own binary
/// (`/opt/sola/bin/sola-shell` → `/opt/sola/bin/solactl`), falling back to
/// a bare `solactl` (PATH lookup) if the self-path can't be resolved.
fn solactl_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("solactl")))
        .unwrap_or_else(|| std::path::PathBuf::from("solactl"))
}
