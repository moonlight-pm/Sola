//! Open http(s) URLs in **sola-browser** (Sola's product browser).
//!
//! All Sola surfaces that open links should call [`open`] so behaviour
//! stays consistent:
//!
//! - `solactl open` (xdg-open / MIME path when pointed here)
//! - sola-shell handling of `Topic::OpenUrl`
//! - terminal / mail / arcade clickable links
//!
//! Spawns `/opt/sola/bin/sola-browser <url>` detached. Override the binary
//! with `SOLA_BROWSER`. No other browser fallback.
//!
//! When sola-browser is already running, a second spawn still starts a new
//! process (no single-instance handoff yet). Prefer bus `Topic::OpenUrl` for
//! in-session "open in existing window" once a singleton path exists; today
//! the browser also listens for that topic when it is up.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Env override for the sola-browser binary (defaults to
/// `/opt/sola/bin/sola-browser`).
pub const SOLA_BROWSER_ENV: &str = "SOLA_BROWSER";

const DEFAULT_SOLA_BROWSER: &str = "/opt/sola/bin/sola-browser";

/// Open `uri` in sola-browser. Spawns detached; returns after spawn (not
/// after the browser finishes).
pub fn open(uri: &str) -> Result<(), String> {
    if uri.trim().is_empty() {
        return Err("empty URL".into());
    }

    let bin = sola_browser_bin().ok_or_else(|| {
        format!(
            "sola-browser not found at {DEFAULT_SOLA_BROWSER} \
             (install it, or set {SOLA_BROWSER_ENV} to the binary path)"
        )
    })?;
    Command::new(&bin)
        .arg(uri)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", bin.display()))?;
    Ok(())
}

/// Best-effort open: log failures, never panic. For UI event handlers.
pub fn open_logged(uri: &str) {
    match open(uri) {
        Ok(()) => tracing::info!(%uri, "opened URL in sola-browser"),
        Err(e) => tracing::warn!(%uri, error = %e, "failed to open URL"),
    }
}

/// Path to the sola-browser binary, if present on disk.
pub fn sola_browser_bin() -> Option<PathBuf> {
    if let Ok(p) = std::env::var(SOLA_BROWSER_ENV) {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
        tracing::warn!(
            path = %path.display(),
            "{SOLA_BROWSER_ENV} set but file missing; falling back to default"
        );
    }
    let path = PathBuf::from(DEFAULT_SOLA_BROWSER);
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
    fn sola_browser_default_when_present() {
        // SAFETY: test-only, single-threaded unit test process.
        unsafe { std::env::remove_var(SOLA_BROWSER_ENV) };
        let path = PathBuf::from(DEFAULT_SOLA_BROWSER);
        if !path.is_file() {
            return;
        }
        assert_eq!(sola_browser_bin().as_deref(), Some(path.as_path()));
    }
}
