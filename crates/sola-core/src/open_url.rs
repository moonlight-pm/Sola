//! Open http(s) URLs in the system browser — currently **Helium**.
//!
//! sola-browser is not the day-to-day handler (WPE/CEF still immature for
//! general browsing). All Sola surfaces that open links should call
//! [`open`] so behaviour stays consistent:
//!
//! - `solactl open` (xdg-open / MIME path when pointed here)
//! - sola-shell handling of `Topic::OpenUrl`
//! - terminal / mail clickable links
//!
//! Helium is launched the same way as `helium.desktop`:
//! `appimage-run ~/Applications/Helium.AppImage <url>`.
//! Override the AppImage path with `HELIUM_APPIMAGE`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Env override for the Helium AppImage path (defaults to
/// `$HOME/Applications/Helium.AppImage`, matching `helium.desktop`).
pub const HELIUM_APPIMAGE_ENV: &str = "HELIUM_APPIMAGE";

/// Open `uri` in Helium. Spawns detached; returns after spawn (not after the
/// browser finishes). Chromium-based Helium hands the URL to a running
/// instance when one exists.
pub fn open(uri: &str) -> Result<(), String> {
    if uri.trim().is_empty() {
        return Err("empty URL".into());
    }
    let appimage = helium_appimage().ok_or_else(|| {
        format!(
            "Helium AppImage not found (set {HELIUM_APPIMAGE_ENV} or install \
             ~/Applications/Helium.AppImage)"
        )
    })?;
    Command::new("appimage-run")
        .arg(&appimage)
        .arg(uri)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn appimage-run {}: {e}", appimage.display()))?;
    Ok(())
}

/// Best-effort open: log failures, never panic. For UI event handlers.
pub fn open_logged(uri: &str) {
    match open(uri) {
        Ok(()) => tracing::info!(%uri, "opened URL in Helium"),
        Err(e) => tracing::warn!(%uri, error = %e, "failed to open URL in Helium"),
    }
}

/// Path to the Helium AppImage, if present on disk.
pub fn helium_appimage() -> Option<PathBuf> {
    if let Ok(p) = std::env::var(HELIUM_APPIMAGE_ENV) {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
        tracing::warn!(
            path = %path.display(),
            "HELIUM_APPIMAGE set but file missing; falling back to default"
        );
    }
    let home = std::env::var_os("HOME")?;
    let path = Path::new(&home).join("Applications/Helium.AppImage");
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_url_errors() {
        assert!(open("").is_err());
        assert!(open("   ").is_err());
    }

    #[test]
    fn helium_appimage_resolves_default_when_present() {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let expected = Path::new(&home).join("Applications/Helium.AppImage");
        if !expected.is_file() {
            return;
        }
        // SAFETY: test-only, single-threaded unit test process.
        unsafe { std::env::remove_var(HELIUM_APPIMAGE_ENV) };
        assert_eq!(helium_appimage().as_deref(), Some(expected.as_path()));
    }
}
