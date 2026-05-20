//! Bake NixOS's system library path into the binary's RUNPATH so the
//! dynamic loader can find libraries that are dlopen'd at runtime.
//!
//! iced's transitive deps (smithay-clipboard's `dlopen` default) flip
//! wayland-sys into dlopen mode — libwayland-client.so.0 is then loaded
//! at runtime, not linked at build time. On NixOS the library lives at
//! `/nix/store/<hash>-wayland-*/lib/`, which isn't in the dynamic
//! loader's default search path. Adding `/run/current-system/sw/lib`
//! (the symlink farm where every system package's runtime libs are
//! exposed) to the binary's RUNPATH lets `dlopen` find the active
//! libwayland — and incidentally any other system lib that iced's
//! ecosystem might decide to dlopen later (libxkbcommon, libGL, etc.).
//!
//! RUNPATH (not RPATH) is the correct attribute: RUNPATH is searched
//! AFTER LD_LIBRARY_PATH, so a user who wants to override the system
//! library can still do so. The `--enable-new-dtags` linker flag is
//! what makes ld emit RUNPATH instead of the older RPATH.

fn main() {
    // Re-run only if this file changes; the link args are static.
    println!("cargo:rerun-if-changed=build.rs");

    println!("cargo:rustc-link-arg=-Wl,--enable-new-dtags");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/run/current-system/sw/lib");
}
