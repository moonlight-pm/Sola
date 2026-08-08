//! Best-effort process identity from `/proc` for windows whose xdg
//! `app_id` is empty or wrong (gamescope's wayland host often lands as
//! `app_id=""` under River even though libdecor sets `"gamescope"`).

use std::path::Path;

/// True when `pid` looks like a gamescope host process (cmdline contains
/// `gamescope` as an argv token or path component).
pub fn process_is_gamescope(pid: u32) -> bool {
    let raw = std::fs::read(Path::new("/proc").join(pid.to_string()).join("cmdline"))
        .unwrap_or_default();
    if raw.is_empty() {
        return false;
    }
    // `/proc/*/cmdline` is NUL-separated argv.
    raw.split(|&b| b == 0)
        .filter(|t| !t.is_empty())
        .any(|tok| {
            let s = String::from_utf8_lossy(tok);
            // Match path tail or bare name without catching e.g. "my-gamescope-docs".
            let base = s.rsplit('/').next().unwrap_or(&s);
            base == "gamescope" || base.starts_with("gamescope-")
        })
}

/// Canonical Wayland app_id we force for gamescope hosts.
pub const GAMESCOPE_APP_ID: &str = "gamescope";

/// Fallback window title when gamescope never sets one (or clears it).
pub const GAMESCOPE_DEFAULT_TITLE: &str = "Gamescope";

/// Whether this app_id should take the gamescope host path (sizing, etc.).
pub fn is_gamescope_app_id(app_id: &str) -> bool {
    app_id.eq_ignore_ascii_case(GAMESCOPE_APP_ID)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_gamescope_app_id_case_insensitive() {
        assert!(is_gamescope_app_id("gamescope"));
        assert!(is_gamescope_app_id("Gamescope"));
        assert!(!is_gamescope_app_id(""));
        assert!(!is_gamescope_app_id("steam"));
    }
}
