//! Open image files in **sola-paint** (Sola's default image app).
//!
//! Surfaces that open a raster path should call [`open`] so behaviour
//! stays consistent:
//!
//! - `solactl open` (xdg-open / MIME path when pointed here)
//! - file managers via `sola-paint.desktop`
//!
//! Spawns `/opt/sola/bin/sola-paint <path>` detached. Override the binary
//! with `SOLA_PAINT`. No other image-app fallback.
//!
//! When sola-paint is already running, a second spawn still starts a new
//! process (no single-instance handoff yet). Prefer bus `Topic::OpenImage`
//! for in-session "open in existing window"; paint listens when it is up.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Env override for the sola-paint binary (defaults to
/// `/opt/sola/bin/sola-paint`).
pub const SOLA_PAINT_ENV: &str = "SOLA_PAINT";

const DEFAULT_SOLA_PAINT: &str = "/opt/sola/bin/sola-paint";

/// Extensions sola-paint treats as raster images.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "jpe", "gif", "webp", "bmp", "tif", "tiff", "tga",
];

/// True when `target` is a filesystem path (or `file://` URL) whose
/// extension looks like a raster image.
pub fn looks_like_image(target: &str) -> bool {
    let path = strip_file_url(target);
    if path.is_empty() || path.contains("://") {
        return false;
    }
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            IMAGE_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

/// Open `path` (or a `file://` URL) in sola-paint. Spawns detached.
pub fn open(path: &str) -> Result<(), String> {
    let path = strip_file_url(path);
    if path.trim().is_empty() {
        return Err("empty image path".into());
    }
    let bin = sola_paint_bin().ok_or_else(|| {
        format!(
            "sola-paint not found at {DEFAULT_SOLA_PAINT} \
             (install it, or set {SOLA_PAINT_ENV} to the binary path)"
        )
    })?;
    Command::new(&bin)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", bin.display()))?;
    Ok(())
}

/// Best-effort open: log failures, never panic.
pub fn open_logged(path: &str) {
    match open(path) {
        Ok(()) => tracing::info!(%path, "opened image in sola-paint"),
        Err(e) => tracing::warn!(%path, error = %e, "failed to open image"),
    }
}

/// Path to the sola-paint binary, if present on disk.
pub fn sola_paint_bin() -> Option<PathBuf> {
    if let Ok(p) = std::env::var(SOLA_PAINT_ENV) {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
        tracing::warn!(
            path = %path.display(),
            "{SOLA_PAINT_ENV} set but file missing; falling back to default"
        );
    }
    let path = PathBuf::from(DEFAULT_SOLA_PAINT);
    path.is_file().then_some(path)
}

fn strip_file_url(target: &str) -> &str {
    target
        .strip_prefix("file://")
        .map(|rest| rest.split_once('?').map(|(p, _)| p).unwrap_or(rest))
        .unwrap_or(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_path_errors() {
        assert!(open("").is_err());
        assert!(open("   ").is_err());
    }

    #[test]
    fn detects_image_paths() {
        assert!(looks_like_image("/tmp/shot.png"));
        assert!(looks_like_image("file:///home/me/pic.JPEG"));
        assert!(looks_like_image("portrait.webp"));
        assert!(!looks_like_image("https://example.com/a.png"));
        assert!(!looks_like_image("/tmp/notes.txt"));
        assert!(!looks_like_image("https://example.com"));
    }
}
