//! Shared on-disk assets (icons, fonts, ...) for Sola.
//!
//! Every third-party asset lives at `/opt/sola/share/<category>/<pack>/...`
//! and is populated by `cargo make assets sync`. Nothing is committed to
//! the repo; nothing is rsynced by `install`. A clean clone auto-syncs
//! the first time `cargo make install` runs and is good for every
//! subsequent build.
//!
//! Nothing is compiled into consumer binaries — all data is read from disk.

use std::path::{Path, PathBuf};

pub mod icons;

pub const ASSETS_DIR: &str = "/opt/sola/share";

/// Resolve a relative asset path to a real file under `ASSETS_DIR`.
/// Returns `None` when the file is missing — callers handle that via
/// 404 paths (URI scheme) or `Option`-returning helpers (icons).
pub fn resolve(path: &str) -> Option<PathBuf> {
    let candidate = Path::new(ASSETS_DIR).join(path);
    candidate.is_file().then_some(candidate)
}
