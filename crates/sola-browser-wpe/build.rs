//! Bake NixOS's system library path into the binary's RUNPATH so the
//! dynamic loader can find libraries that are dlopen'd at runtime.
//!
//! Same rationale as `crates/sola-monitor-iced/build.rs` — iced's
//! transitive `smithay-clipboard` flips `wayland-sys` into dlopen
//! mode, so `libwayland-client.so.0` is loaded at runtime rather than
//! resolved at link time. We'll also dlopen WPE / libwpe /
//! `wpebackend-fdo` from this same path once they're wired in, so
//! the same RUNPATH covers them too.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    println!("cargo:rustc-link-arg=-Wl,--enable-new-dtags");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/run/current-system/sw/lib");
}
