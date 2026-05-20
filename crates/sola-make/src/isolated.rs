//! Isolated / experimental crates — built and installed alongside the
//! workspace, but kept out of `[workspace] members` so their dependency
//! features don't unify with the workspace and break unrelated crates.
//!
//! Cargo unifies features across every member of a workspace. When an
//! experimental crate pulls in something like `iced` (whose transitive
//! deps enable `wayland-sys/dlopen`), that feature flip propagates to
//! every workspace crate that touches wayland-sys — and sola-river,
//! which had been linking wayland-client statically, suddenly starts
//! dlopen'ing it at runtime and fails on NixOS where libwayland isn't
//! on the dynamic loader's default path. The fix is to keep such
//! crates out of the workspace.
//!
//! This module makes that exclusion painless. `cargo make build` /
//! `install` discover excluded paths from the workspace Cargo.toml's
//! `[workspace] exclude` list and build/install each one with its own
//! manifest, so adding a new experimental crate is just a matter of
//! dropping it in and adding it to the `exclude` glob.

use std::path::PathBuf;
use std::process::Command;

/// One isolated crate to build / install.
pub struct IsolatedCrate {
    /// Path relative to the workspace root (e.g. "crates/sola-monitor-iced").
    pub path: PathBuf,
    /// Package name from the crate's `Cargo.toml` (e.g. "sola-monitor-iced").
    /// Also the produced binary name; we don't support multi-bin
    /// isolated crates because none of our use cases need it yet.
    pub name: String,
}

/// Read the workspace root `Cargo.toml`, parse its `[workspace] exclude`
/// list, and resolve each entry to an `IsolatedCrate` (path + binary
/// name) by reading the crate's own `Cargo.toml`. Entries without a
/// readable `Cargo.toml` or a `[package].name` field are skipped with
/// a warning — they're not buildable as isolated targets.
pub fn discover() -> Vec<IsolatedCrate> {
    let root = match std::fs::read_to_string("Cargo.toml") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("sola-make: failed to read workspace Cargo.toml: {e}");
            return Vec::new();
        }
    };

    let parsed: toml::Value = match toml::from_str(&root) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("sola-make: failed to parse workspace Cargo.toml: {e}");
            return Vec::new();
        }
    };

    let excludes = parsed
        .get("workspace")
        .and_then(|w| w.get("exclude"))
        .and_then(|e| e.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(PathBuf::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut out = Vec::new();
    for path in excludes {
        let manifest = path.join("Cargo.toml");
        let contents = match std::fs::read_to_string(&manifest) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let parsed: toml::Value = match toml::from_str(&contents) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let name = parsed
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .map(|s| s.to_string());
        let Some(name) = name else {
            eprintln!(
                "sola-make: isolated crate {} has no [package].name, skipping",
                path.display()
            );
            continue;
        };
        out.push(IsolatedCrate { path, name });
    }
    out
}

/// Build a single isolated crate with its own manifest. Each crate has
/// its own `target/` directory (cargo's default when a manifest isn't
/// part of a workspace), so feature unification is contained.
pub fn build(c: &IsolatedCrate, release: bool) -> bool {
    let manifest = c.path.join("Cargo.toml");
    let mut args = vec!["build".to_string(), "--manifest-path".to_string()];
    args.push(manifest.to_string_lossy().into_owned());
    if release {
        args.push("--release".to_string());
    }
    println!("  building isolated crate: {}", c.name);
    let status = Command::new("cargo")
        .args(&args)
        .status()
        .expect("failed to run cargo build for isolated crate");
    status.success()
}

/// Path to a built binary inside an isolated crate's own `target/`.
/// Used by install to find the freshly built artifact to copy into
/// `/opt/sola/bin/`.
pub fn binary_path(c: &IsolatedCrate, release: bool) -> PathBuf {
    let profile = if release { "release" } else { "debug" };
    c.path.join("target").join(profile).join(&c.name)
}

/// Whether the discovered crate has a buildable binary target. Used by
/// install to skip pure-library isolated crates (none today, but the
/// glob is generic).
pub fn has_binary(c: &IsolatedCrate) -> bool {
    let main = c.path.join("src/main.rs");
    if main.exists() {
        return true;
    }
    let manifest = c.path.join("Cargo.toml");
    std::fs::read_to_string(&manifest)
        .map(|s| s.contains("[[bin]]"))
        .unwrap_or(false)
}

/// Convenience for the common discover → filter binaries → build loop.
/// Returns the list that was built so the caller can install them.
pub fn build_all(release: bool) -> Vec<IsolatedCrate> {
    let crates = discover()
        .into_iter()
        .filter(has_binary)
        .collect::<Vec<_>>();
    if crates.is_empty() {
        return crates;
    }
    let mut built = Vec::new();
    for c in crates {
        if build(&c, release) {
            built.push(c);
        } else {
            eprintln!("sola-make: isolated build failed for {}", c.name);
            std::process::exit(1);
        }
    }
    built
}

