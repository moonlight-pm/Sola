//! Zero-dep stub for `tauri-apps/download-cef`.
//!
//! `cef-dll-sys` (a transitive dep of `cef`) declares an unconditional
//! `[build-dependencies] download-cef = "2.3"`, and the upstream
//! `download-cef` pulls in `ureq` + `rustls` + `ring` + `rustls-webpki`
//! + `bzip2` + `tar` (all unconditionally — no feature flags exist to
//! opt out). Those crates rebuilt every time we alternated between
//! `cargo build -p sola-kit` and `cargo build` (whole workspace) and
//! `cargo make build sola-kit`, because cargo's resolver computes a
//! different feature unification for shared deps depending on the
//! in-scope package set, invalidating fingerprints.
//!
//! This crate is wired in via `[patch.crates-io]` in the workspace
//! Cargo.toml, so cef-dll-sys's `download-cef` import resolves here.
//! We re-implement only the surface that `cef-dll-sys/build.rs`
//! actually consumes, with everything routed through the OUT_DIR
//! fallback branch (no FLATPAK, no CEF_PATH) so the network/extract
//! code paths in upstream are simply absent from our build graph.
//!
//! The single piece of real work is in [`extract_target_archive`]:
//! after `cef-dll-sys` "downloads" (via our no-op stubs), we symlink
//! `<location>/cef_linux_x86_64/libcef.so` to the pre-installed
//! `~/.cache/sola/cef-<CEF_VERSION>/Release/libcef.so` placed there
//! by `cargo make install-cef`. cef-dll-sys's
//! `cargo::rustc-link-search=native=<location>/cef_linux_x86_64`
//! then resolves `-lcef` correctly. install-cef remains the owner of
//! the actual download, NixOS patchelf, and Resources/* symlinks.

use std::fmt;
use std::path::{Path, PathBuf};

/// Single source of truth, shared with `crates/sola-make/src/cef.rs`
/// and `crates/sola-kit/build.rs` via the same workspace-root file.
const CEF_VERSION: &str = include_str!("../../../cef-version");

fn cef_version_trimmed() -> &'static str {
    // `trim_ascii_end` is `const fn` since Rust 1.80 but `str::trim_end`
    // is not, so do it at runtime — called rarely (build-time only).
    CEF_VERSION.trim_end()
}

fn sola_cef_dir() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home)
        .join(".cache/sola")
        .join(format!("cef-{}", cef_version_trimmed()))
}

#[derive(Debug)]
pub struct Error(String);

impl Error {
    fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct OsAndArch {
    pub os: &'static str,
    pub arch: &'static str,
}

impl fmt::Display for OsAndArch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cef_{}_{}", self.os, self.arch)
    }
}

impl TryFrom<&str> for OsAndArch {
    type Error = Error;

    fn try_from(target: &str) -> Result<Self> {
        match target {
            "x86_64-unknown-linux-gnu" => Ok(OsAndArch { os: "linux", arch: "x86_64" }),
            "aarch64-unknown-linux-gnu" => Ok(OsAndArch { os: "linux", arch: "aarch64" }),
            "arm-unknown-linux-gnueabi" => Ok(OsAndArch { os: "linux", arch: "arm" }),
            "aarch64-apple-darwin" => Ok(OsAndArch { os: "macos", arch: "aarch64" }),
            "x86_64-apple-darwin" => Ok(OsAndArch { os: "macos", arch: "x86_64" }),
            "x86_64-pc-windows-msvc" => Ok(OsAndArch { os: "windows", arch: "x86_64" }),
            "aarch64-pc-windows-msvc" => Ok(OsAndArch { os: "windows", arch: "aarch64" }),
            "i686-pc-windows-msvc" => Ok(OsAndArch { os: "windows", arch: "x86" }),
            other => Err(Error::new(format!("unsupported target: {other}"))),
        }
    }
}

/// Strips build metadata from the cargo-package version
/// (`"147.1.0+147.0.10"` → `"147.0.10"`). cef-dll-sys's `build.rs`
/// passes the result to [`CefPlatform::version`] which we ignore, so
/// the precise return value doesn't matter — but we keep the upstream
/// shape to avoid surprises.
pub fn default_version(version: &str) -> String {
    match version.split_once('+') {
        Some((_, build)) => build.to_string(),
        None => version.to_string(),
    }
}

pub fn default_download_url() -> String {
    String::new()
}

/// Only invoked from the FLATPAK and CEF_PATH branches of
/// cef-dll-sys's `build.rs`. We never set those env vars, so this is
/// dead code — but the symbol must exist for the upstream build.rs to
/// compile against our crate.
pub fn check_archive_json(_version: &str, _location: &str) -> Result<()> {
    Ok(())
}

#[derive(Default)]
pub struct CefIndex {
    inner: CefPlatform,
}

impl CefIndex {
    pub fn download_from(_url: &str) -> Result<Self> {
        Ok(Self::default())
    }

    /// Validates the target string so an unsupported triple still
    /// produces a clear error, then returns the (always-the-same)
    /// inner platform stub.
    pub fn platform(&self, target: &str) -> Result<&CefPlatform> {
        let _ = OsAndArch::try_from(target)?;
        Ok(&self.inner)
    }
}

#[derive(Default)]
pub struct CefPlatform {
    inner: CefVersion,
}

impl CefPlatform {
    pub fn version(&self, _cef_version: &str) -> Result<&CefVersion> {
        Ok(&self.inner)
    }
}

#[derive(Default)]
pub struct CefVersion;

impl CefVersion {
    /// cef-dll-sys passes this return value as the `archive` argument
    /// to [`extract_target_archive`], which ignores it. The path does
    /// not need to exist.
    pub fn download_archive_from<P: AsRef<Path>>(
        &self,
        _url: &str,
        location: P,
        _show_progress: bool,
    ) -> Result<PathBuf> {
        Ok(location.as_ref().join("unused-by-stub.tar.bz2"))
    }

    pub fn write_archive_json<P: AsRef<Path>>(&self, _location: P) -> Result<()> {
        Ok(())
    }
}

/// The only function in this stub that does real work.
///
/// `cef-dll-sys/build.rs` (in its OUT_DIR fallback branch) calls this
/// expecting an extracted CEF tree at `<location>/cef_linux_x86_64/`
/// containing at minimum `libcef.so`. cef-dll-sys then emits
/// `cargo::rustc-link-search=native=<location>/cef_linux_x86_64` and
/// `cargo::rustc-link-lib=dylib=cef`, so the linker resolves `-lcef`
/// against `libcef.so` at the top of that directory.
///
/// We satisfy this by symlinking `<location>/<os_arch>/libcef.so` to
/// the pre-installed `~/.cache/sola/cef-<version>/Release/libcef.so`
/// placed there by `cargo make install-cef`. install-cef is also
/// responsible for the NixOS patchelf and the Resources/* runtime
/// symlinks; this stub is purely about satisfying cef-dll-sys's link
/// step.
///
/// On Linux, cef-dll-sys does NOT invoke its `cmake::Config::build()`
/// step (libcef_dll_wrapper is only built on Windows/macOS), so we
/// don't need to expose CMakeLists.txt, cmake/, include/, libcef_dll/
/// inside the cef_dir.
pub fn extract_target_archive<P: AsRef<Path>, Q: AsRef<Path>>(
    target: &str,
    _archive: P,
    location: Q,
    _show_progress: bool,
) -> Result<PathBuf> {
    let os_arch = OsAndArch::try_from(target)?;
    let location = location.as_ref();
    let cef_dir = location.join(os_arch.to_string());

    let real = sola_cef_dir();
    let real_libcef = real.join("Release/libcef.so");
    if !real_libcef.exists() {
        return Err(Error::new(format!(
            "CEF binary distribution missing.\n  expected: {}\n  run: cargo make install-cef",
            real_libcef.display(),
        )));
    }

    std::fs::create_dir_all(&cef_dir)?;
    let lib_link = cef_dir.join("libcef.so");
    if lib_link.is_symlink() || lib_link.exists() {
        std::fs::remove_file(&lib_link)?;
    }
    std::os::unix::fs::symlink(&real_libcef, &lib_link)?;

    Ok(cef_dir)
}
