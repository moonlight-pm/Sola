//! CEF link path — same binary is re-exec'd as renderer / GPU / utility.
//!
//! sola-browser's build.rs already links libcef into that rlib, but this
//! process image is `sola-wrapper`, so we emit the same search path.

use std::path::PathBuf;

const CEF_VERSION: &str = include_str!("../../cef-version").trim_ascii_end();

/// `SOLA_CEF_DIR` (a packaged CEF, e.g. Oath's `pkg:cef`), else the
/// `cargo make install-cef` cache. Its `Release/` is baked into RUNPATH.
fn cef_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SOLA_CEF_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
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
    println!(
        "cargo:rustc-link-arg=-Wl,-rpath,{}",
        release_dir.display()
    );
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../cef-version");
    println!("cargo:rerun-if-env-changed=SOLA_CEF_DIR");
    println!("cargo:rerun-if-env-changed=XDG_CACHE_HOME");
}
