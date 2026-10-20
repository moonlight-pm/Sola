//! Design's own CEF profile — never `browser_data_root()`.
//!
//! The engine sockets, CEF user data (`cef/`), and `projects.json` share
//! `~/.config/sola/design/`. Cache is discardable under `~/.cache/sola/design/`.

use std::path::PathBuf;

use sola_browser::profiles::ActiveProfile;

pub const PROFILE_ID: &str = "sola-design";

pub fn data_dir() -> PathBuf {
    sola_core::config::sola_config_dir().join("design")
}

fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"))
        .join("sola")
        .join("design")
}

/// Bind this process so the CefEngine helper uses design paths.
pub fn bind() -> Result<ActiveProfile, String> {
    sola_browser::profiles::bind_external(PROFILE_ID, data_dir(), cache_dir())
}
