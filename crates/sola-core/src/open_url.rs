//! Open http(s) URLs in **sola-browser** (Sola's product browser).
//!
//! All Sola surfaces that open links should call [`open`] so behaviour
//! stays consistent:
//!
//! - `solactl open` (xdg-open / MIME path when pointed here)
//! - sola-shell handling of `Topic::OpenUrl`
//! - terminal / mail / arcade clickable links
//!
//! Spawns `env::bin_path("sola-browser")` (`/bin` on Oath, `/opt/sola/bin`
//! on NixOS). Override the binary with `SOLA_BROWSER`. No other browser
//! fallback.
//!
//! When sola-browser is already running, [`open`] first writes the URL to
//! `chrome.sock` (the iced singleton). Only if that fails do we spawn a
//! new process.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Env override for the sola-browser binary (defaults to
/// [`crate::env::bin_path`] `"sola-browser"`).
pub const SOLA_BROWSER_ENV: &str = "SOLA_BROWSER";

/// Open `uri` in sola-browser. Spawns detached; returns after spawn (not
/// after the browser finishes).
pub fn open(uri: &str) -> Result<(), String> {
    if uri.trim().is_empty() {
        return Err("empty URL".into());
    }

    if try_handoff_running_chrome(uri) {
        return Ok(());
    }

    let bin = sola_browser_bin().ok_or_else(|| {
        format!(
            "sola-browser not found at {} \
             (install it, or set {SOLA_BROWSER_ENV} to the binary path)",
            crate::env::bin_path("sola-browser").display()
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

/// Path must match `sola-browser` `instance::chrome_sock_path`.
fn chrome_sock_path() -> PathBuf {
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from(".local/share"));
    root.join("sola/browser/chrome.sock")
}

fn try_handoff_running_chrome(uri: &str) -> bool {
    let sock = chrome_sock_path();
    let Ok(mut stream) = UnixStream::connect(&sock) else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    if stream.write_all(format!("{uri}\n").as_bytes()).is_err() {
        return false;
    }
    tracing::info!(%uri, path = %sock.display(), "handed URL to running sola-browser");
    true
}

/// True when iced chrome is bound to `chrome.sock`.
///
/// Connect-only: the chrome treats an empty read as a probe, not an
/// activate / open (see `sola-browser` `instance::read_handoff`).
pub fn chrome_is_running() -> bool {
    UnixStream::connect(chrome_sock_path()).is_ok()
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
    let path = crate::env::bin_path("sola-browser");
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
        let path = crate::env::bin_path("sola-browser");
        if !path.is_file() {
            return;
        }
        assert_eq!(sola_browser_bin().as_deref(), Some(path.as_path()));
    }
}
