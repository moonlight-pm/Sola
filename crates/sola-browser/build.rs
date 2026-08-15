//! sola-browser build script (CEF link path).
//!
//! Mirrors `crates/sola-kit/build.rs` — the workspace's existing
//! CEF consumer. Reads the pinned CEF version from the workspace-
//! root `cef-version` file, verifies `~/.cache/sola/cef-<ver>/Release/libcef.so`
//! exists (errors with a clear "run `cargo make install-cef`" message
//! if not), and emits link directives.
//!
//! No download path here — that lives only in sola-make so this
//! crate doesn't drag `ureq`/`ring`/`rustls`/`bzip2`/`tar` into its
//! build graph.

use std::path::PathBuf;

const CEF_VERSION: &str = include_str!("../../cef-version").trim_ascii_end();

fn cef_dir() -> PathBuf {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home =
                std::env::var_os("HOME").expect("HOME not set; cannot resolve CEF cache path");
            PathBuf::from(home).join(".cache")
        });
    cache.join("sola").join(format!("cef-{CEF_VERSION}"))
}

fn main() {
    let cef_dir = cef_dir();
    let release_dir = cef_dir.join("Release");
    let libcef = release_dir.join("libcef.so");

    if !libcef.exists() {
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
    println!("cargo:rustc-env=SOLA_BROWSER_CEF_DIR={}", cef_dir.display());

    // RUNPATH (NixOS sw lib + opengl-driver) is set workspace-wide via
    // .cargo/config.toml — no per-crate link-args needed.

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../cef-version");
}
