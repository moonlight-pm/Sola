//! Shared on-disk assets (icons, fonts, ...) for Sola.
//!
//! Every third-party asset lives at `<share>/<category>/<pack>/...`, where
//! `<share>` is [`sola_core::paths::share_dir`] (`/opt/sola/share` on
//! NixOS), and is populated by `cargo make assets sync`. Nothing is committed to
//! the repo; nothing is rsynced by `install`. A clean clone auto-syncs
//! the first time `cargo make install` runs and is good for every
//! subsequent build.
//!
//! Nothing is compiled into consumer binaries — all data is read from disk.

use std::path::PathBuf;

pub mod icons;

/// Resolve a relative asset path to a real file under the share tree.
/// Returns `None` when the file is missing — callers handle that via
/// 404 paths (URI scheme) or `Option`-returning helpers (icons).
pub fn resolve(path: &str) -> Option<PathBuf> {
    let candidate = sola_core::paths::share_dir().join(path);
    candidate.is_file().then_some(candidate)
}
