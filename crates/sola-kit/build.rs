//! sola-kit build script.
//!
//! 1. Ensures the pinned CEF binary distribution is present at
//!    ~/.cache/sola/cef-<version>/ (downloads if missing — first build
//!    on a fresh machine takes ~1-2 minutes).
//! 2. Emits link directives so cargo links against libcef.so from that
//!    cache directory and bakes a RUNPATH into the binary covering
//!    both libcef.so and the CEF transitive system deps that NixOS
//!    exposes via `programs.nix-ld.libraries`.
//!
//! NixOS runtime: see `docs/vault/Distribution.md` →
//! "CEF runtime libraries (sola-kit)" for the full required
//! `programs.nix-ld.libraries` list. With that in place,
//! `/run/current-system/sw/share/nix-ld/lib` is a flat directory of
//! every CEF-needed `.so`, and the RUNPATH below makes the dynamic
//! linker find them with zero LD_LIBRARY_PATH / wrapper-script
//! ceremony.

use std::path::PathBuf;

/// Stable NixOS path that nix-ld populates from
/// `programs.nix-ld.libraries`. `/run/current-system` is repointed on
/// every `nixos-rebuild switch`, so this path always resolves to the
/// active configuration's library set — safe to bake into RUNPATH.
const NIX_LD_LIB_DIR: &str = "/run/current-system/sw/share/nix-ld/lib";

fn main() {
    let cef_dir = sola_make::cef::ensure_cef()
        .expect("CEF binary distribution required (failed to download)");

    let release_dir = sola_make::cef::release_dir();
    let release_dir_str = release_dir.display().to_string();
    println!("cargo:rustc-link-search=native={release_dir_str}");
    println!("cargo:rustc-link-lib=dylib=cef");

    // Embed the cache path as a compile-time string for runtime CefSettings.
    println!("cargo:rustc-env=SOLA_KIT_CEF_DIR={}", cef_dir.display());

    // Bake RUNPATH into the binary so it finds libcef.so + CEF's
    // transitive system deps without any wrapper or env var.
    //   - release_dir: holds libcef.so itself
    //   - NIX_LD_LIB_DIR: holds the system libs (glib, nss, nspr, atk,
    //     dbus, cups, X11, gbm, expat, xkbcommon, cairo, pango, udev,
    //     alsa, atspi, …) per `programs.nix-ld.libraries`
    // `--enable-new-dtags` makes the linker emit DT_RUNPATH (modern,
    // overridable by LD_LIBRARY_PATH) instead of legacy DT_RPATH.
    let _ = PathBuf::from(NIX_LD_LIB_DIR); // for clarity in diffs
    println!("cargo:rustc-link-arg=-Wl,--enable-new-dtags");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{release_dir_str}");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{NIX_LD_LIB_DIR}");

    println!("cargo:rerun-if-changed=build.rs");
}
