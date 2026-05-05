//! sola-kit build script.
//!
//! 1. Ensures the pinned CEF binary distribution is present at
//!    ~/.cache/sola/cef-<version>/ (downloads if missing — first build
//!    on a fresh machine takes ~1-2 minutes).
//! 2. Emits link directives so cargo links against libcef.so from that
//!    cache directory.
//! 3. Writes the cache path to target/cef-runpath for the dev-mode
//!    wrapper used by `cargo make run`.
//!
//! NixOS runtime depends on (must be in configuration.nix):
//!   libGL, libgbm, libnss, libnspr, fontconfig, freetype, expat,
//!   alsaLib, libdrm, mesa (for libgbm/libGL), libxkbcommon, wayland.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let cef_dir = sola_make::cef::ensure_cef()
        .expect("CEF binary distribution required (failed to download)");

    let release_dir = sola_make::cef::release_dir();
    println!("cargo:rustc-link-search=native={}", release_dir.display());
    println!("cargo:rustc-link-lib=dylib=cef");

    // Embed the cache path as a compile-time string for runtime CefSettings.
    println!("cargo:rustc-env=SOLA_KIT_CEF_DIR={}", cef_dir.display());

    // Write cef-runpath for dev-mode `cargo make run` wrapper.
    let target_dir: PathBuf = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .map(|p| p.ancestors().nth(3).unwrap_or(&p).to_path_buf())
        .unwrap_or_else(|| PathBuf::from("target"));
    let runpath_file = target_dir.join("cef-runpath");
    let _ = fs::write(&runpath_file, release_dir.display().to_string());

    println!("cargo:rerun-if-changed=build.rs");
}
