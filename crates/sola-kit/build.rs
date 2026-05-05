//! sola-kit build script.
//!
//! 1. Ensures the pinned CEF binary distribution is present at
//!    ~/.cache/sola/cef-<version>/ (downloads if missing — first build
//!    on a fresh machine takes ~1-2 minutes). `ensure_cef` also
//!    patchelfs libcef.so on a fresh download so its DT_RUNPATH covers
//!    `/run/current-system/sw/share/nix-ld/lib` — see
//!    `crates/sola-make/src/cef.rs::patch_libcef_for_nix_ld` and
//!    `docs/vault/Distribution.md` → "CEF runtime libraries
//!    (sola-kit)".
//! 2. Emits link directives so cargo links against libcef.so from that
//!    cache directory.
//!
//! With (1) in place, the dynamic linker resolves libcef.so's
//! transitive deps (glib, nss, atk, X11, gbm, …) directly through
//! libcef's own RUNPATH — sola-kit's binary doesn't need any
//! additional rpath beyond what `cef-dll-sys` already emits.

fn main() {
    let cef_dir = sola_make::cef::ensure_cef()
        .expect("CEF binary distribution required (failed to download)");

    let release_dir = sola_make::cef::release_dir();
    let release_dir_str = release_dir.display().to_string();
    println!("cargo:rustc-link-search=native={release_dir_str}");
    println!("cargo:rustc-link-lib=dylib=cef");

    // Embed the cache path as a compile-time string for runtime CefSettings.
    println!("cargo:rustc-env=SOLA_KIT_CEF_DIR={}", cef_dir.display());

    // Make the cache copy of libcef.so come FIRST in the binary's
    // RUNPATH. The `cef-dll-sys` build script also copies libcef.so to
    // its own per-build OUT_DIR and emits an `-rpath` for it; without
    // this directive we'd resolve libcef via that copy at runtime, and
    // its (un-patched) `$ORIGIN` RUNPATH wouldn't find CEF's transitive
    // system deps. The cache copy is the one `ensure_cef` patchelf'd
    // with `/run/current-system/sw/share/nix-ld/lib`, so we want it to
    // win the lookup.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{release_dir_str}");

    println!("cargo:rerun-if-changed=build.rs");
}
