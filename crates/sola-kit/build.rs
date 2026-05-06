//! sola-kit build script.
//!
//! 1. Reads the pinned CEF version from the workspace-root `cef-version`
//!    file. The file is the single source of truth — `sola-make`'s
//!    `cef::CEF_VERSION` reads the same path. Bump = one-line edit.
//! 2. Computes the cache path: `~/.cache/sola/cef-<version>/`.
//! 3. Verifies `Release/libcef.so` exists. If not, errors out with a
//!    clear "run `cargo make install-cef` first" message — the download
//!    + extract + patchelf + Resources-symlink work lives in
//!    `sola-make::cef::ensure_cef` and is invoked explicitly by the
//!    `install-cef` subcommand, never implicitly during `cargo build`.
//!    This keeps `sola-kit`'s build graph free of `ureq`/`ring`/
//!    `rustls`/`bzip2`/`tar` (the download crates) so alternation
//!    between `cargo build -p sola-kit` and `cargo make build sola-kit`
//!    doesn't cascade rebuilds across the workspace.
//! 4. Emits link directives so cargo links against libcef.so from the
//!    cache directory.
//!
//! Setup on a fresh machine:
//!
//! ```sh
//! cargo make install-cef    # one-time: downloads ~150 MB, patchelfs, symlinks
//! cargo build -p sola-kit   # works from here on out
//! ```
//!
//! After a CEF version bump (edit `cef-version` at workspace root):
//! re-run `cargo make install-cef`. The cache directory is
//! version-suffixed so multiple versions can coexist on disk.

use std::path::PathBuf;

/// Single source of truth for the pinned CEF release. `sola-make`
/// reads the same file via the same `include_str!` mechanism.
const CEF_VERSION: &str = include_str!("../../cef-version").trim_ascii_end();

fn cef_dir() -> PathBuf {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .expect("HOME not set; cannot resolve CEF cache path");
            PathBuf::from(home).join(".cache")
        });
    cache.join("sola").join(format!("cef-{CEF_VERSION}"))
}

fn main() {
    let cef_dir = cef_dir();
    let release_dir = cef_dir.join("Release");
    let libcef = release_dir.join("libcef.so");

    if !libcef.exists() {
        // Hard fail with a clear, actionable message. Don't try to
        // download — that would re-introduce the heavy build-deps.
        eprintln!();
        eprintln!("error: CEF binary distribution not found");
        eprintln!("       expected: {}", libcef.display());
        eprintln!();
        eprintln!("       run this once to download + extract + patchelf:");
        eprintln!("           cargo make install-cef");
        eprintln!();
        std::process::exit(1);
    }

    println!("cargo:rustc-link-search=native={}", release_dir.display());
    println!("cargo:rustc-link-lib=dylib=cef");

    // Embed the cache path as a compile-time string for runtime CefSettings.
    println!("cargo:rustc-env=SOLA_KIT_CEF_DIR={}", cef_dir.display());

    // Re-run only when the build script itself or the pinned version changes.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../cef-version");
}
