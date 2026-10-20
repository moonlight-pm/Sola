//! Where Sola's programs, shared files and logs live.
//!
//! Each directory is its environment override, else the platform's
//! default:
//!
//! | Dir | Override | Oath | Elsewhere (NixOS) |
//! |-----|----------|------|-------------------|
//! | bin | `SOLA_BIN_DIR` | `/bin` (the package link farm) | `/opt/sola/bin` |
//! | share | `SOLA_SHARE_DIR` | `/oath/store/pkg/sola/share` | `/opt/sola/share` |
//! | log | `SOLA_LOG_DIR` | `$XDG_STATE_HOME/sola` (`~/.local/state/sola`) | `/opt/sola/log` |
//!
//! On Oath, PID 1 runs Sola's processes as the desk user and the store
//! is read-only, so logs go to that user's state directory. Each value
//! is resolved once per process.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::env::on_oath;

const OPT_BIN: &str = "/opt/sola/bin";
const OPT_SHARE: &str = "/opt/sola/share";
const OPT_LOG: &str = "/opt/sola/log";
const OATH_BIN: &str = "/bin";
const OATH_SHARE: &str = "/oath/store/pkg/sola/share";

/// Directory of Sola's executables.
pub fn bin_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        resolve(
            std::env::var_os("SOLA_BIN_DIR"),
            on_oath().then(|| PathBuf::from(OATH_BIN)),
            OPT_BIN,
        )
    })
}

/// [`bin_dir`] joined with a program name (`sola-session`, …).
pub fn bin_path(name: &str) -> PathBuf {
    bin_dir().join(name)
}

/// Sola's shared files: icons, cursors.
pub fn share_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        resolve(
            std::env::var_os("SOLA_SHARE_DIR"),
            on_oath().then(|| PathBuf::from(OATH_SHARE)),
            OPT_SHARE,
        )
    })
}

/// Directory Sola's processes write logs to (`sola.log`, `app-<id>.log`).
pub fn log_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        resolve(
            std::env::var_os("SOLA_LOG_DIR"),
            if on_oath() { state_dir() } else { None },
            OPT_LOG,
        )
    })
}

/// [`log_dir`] joined with a file name.
pub fn log_path(name: &str) -> PathBuf {
    log_dir().join(name)
}

/// `$XDG_STATE_HOME/sola`, else `$HOME/.local/state/sola`.
fn state_dir() -> Option<PathBuf> {
    state_dir_from(std::env::var_os("XDG_STATE_HOME"), std::env::var_os("HOME"))
}

fn state_dir_from(xdg_state: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let base = match xdg_state.map(PathBuf::from).filter(|p| p.is_absolute()) {
        Some(p) => p,
        None => PathBuf::from(home?).join(".local/state"),
    };
    Some(base.join("sola"))
}

/// A non-empty override wins, then the platform's default, then `/opt/sola`.
fn resolve(over: Option<OsString>, platform: Option<PathBuf>, opt: &str) -> PathBuf {
    over.filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or(platform)
        .unwrap_or_else(|| PathBuf::from(opt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_wins_over_platform_and_opt() {
        let got = resolve(Some("/x/bin".into()), Some("/bin".into()), OPT_BIN);
        assert_eq!(got, PathBuf::from("/x/bin"));
    }

    #[test]
    fn empty_override_is_ignored() {
        assert_eq!(resolve(Some("".into()), None, OPT_BIN), PathBuf::from(OPT_BIN));
    }

    #[test]
    fn platform_default_wins_over_opt() {
        let got = resolve(None, Some(OATH_SHARE.into()), OPT_SHARE);
        assert_eq!(got, PathBuf::from(OATH_SHARE));
    }

    #[test]
    fn opt_sola_without_override_or_platform() {
        assert_eq!(resolve(None, None, OPT_LOG), PathBuf::from(OPT_LOG));
    }

    #[test]
    fn state_dir_prefers_absolute_xdg_state_home() {
        let got = state_dir_from(Some("/s".into()), Some("/home".into()));
        assert_eq!(got, Some(PathBuf::from("/s/sola")));
        let got = state_dir_from(Some("rel".into()), Some("/home".into()));
        assert_eq!(got, Some(PathBuf::from("/home/.local/state/sola")));
        assert_eq!(state_dir_from(None, None), None);
    }
}
