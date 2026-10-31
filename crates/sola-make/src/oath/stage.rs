//! Build Sola's binaries and assemble each Oath package's tree under
//! `target/oath/<name>/`: `bin/`, `share/` (sola), a `cef` link
//! (browser, design) and `components.json`.
//!
//! The build is the ordinary incremental `cargo build`, linked against
//! this machine's packages; CEF comes from `pkg:cef`, so the CEF
//! programs' RUNPATH names its live slot rather than a home cache.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use super::packages::{PACKAGES, Package, build_crates};

const STAGE_ROOT: &str = "target/oath";
/// `pkg:cef`'s CEF (`Release/`, `Resources/`), through its live slot.
const OATH_CEF: &str = "/oath/store/pkg/cef/live/cef";
const FIRST_PARTY_ICONS: &str = "crates/sola-assets/icons";

pub struct Staged {
    pub pkg: &'static Package,
    pub dir: PathBuf,
}

/// Build, then stage every package. Exits on failure.
pub fn stage(release: bool) -> Vec<Staged> {
    link_cef_cache();
    build(release);
    let share_cache = Path::new(STAGE_ROOT).join("share-cache");
    crate::assets::sync_into(&share_cache, false);
    // The crate's version, never the commit: where a tree came from is its
    // source record (`provenance`), so a package nothing touched keeps
    // its hash and does not move.
    let version = env!("CARGO_PKG_VERSION");
    PACKAGES
        .iter()
        .map(|pkg| Staged {
            pkg,
            dir: stage_one(pkg, release, &share_cache, &version),
        })
        .collect()
}

/// `cef-dll-sys` (through the download-cef stub) reads CEF from
/// `~/.cache/sola/cef-<version>`; point that at `pkg:cef`.
fn link_cef_cache() {
    let version =
        fs::read_to_string("cef-version").unwrap_or_else(|e| fail(format!("cef-version: {e}")));
    let Some(home) = std::env::var_os("HOME") else {
        fail("HOME is not set".into());
    };
    let link = Path::new(&home)
        .join(".cache/sola")
        .join(format!("cef-{}", version.trim()));
    if link.join("Release/libcef.so").is_file() {
        return;
    }
    if let Some(parent) = link.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::remove_file(&link);
    std::os::unix::fs::symlink(OATH_CEF, &link)
        .unwrap_or_else(|e| fail(format!("link {} -> {OATH_CEF}: {e}", link.display())));
    println!("[oath] {} -> {OATH_CEF}", link.display());
}

fn build(release: bool) {
    let mut cmd = Command::new("cargo");
    cmd.arg("build").env("SOLA_CEF_DIR", OATH_CEF);
    if release {
        cmd.arg("--release");
    }
    for krate in build_crates(PACKAGES.iter().flat_map(|p| p.bins.iter().copied())) {
        cmd.args(["-p", krate]);
    }
    let status = cmd.status().unwrap_or_else(|e| fail(format!("cargo: {e}")));
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}

fn stage_one(pkg: &Package, release: bool, share_cache: &Path, version: &str) -> PathBuf {
    let dir = Path::new(STAGE_ROOT).join(pkg.name);
    let _ = fs::remove_dir_all(&dir);
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap_or_else(|e| fail(format!("{}: {e}", bin.display())));

    let profile = if release { "release" } else { "debug" };
    for name in pkg.bins {
        let from = Path::new("target").join(profile).join(name);
        fs::copy(&from, bin.join(name))
            .unwrap_or_else(|e| fail(format!("copy {}: {e}", from.display())));
    }
    if pkg.share {
        let share = dir.join("share");
        fs::create_dir_all(&share).unwrap_or_else(|e| fail(format!("{}: {e}", share.display())));
        copy_tree(share_cache, &share);
        let icons = share.join("icons");
        fs::create_dir_all(&icons).unwrap_or_else(|e| fail(format!("{}: {e}", icons.display())));
        copy_tree(Path::new(FIRST_PARTY_ICONS), &icons);
    }
    if pkg.cef {
        std::os::unix::fs::symlink(OATH_CEF, dir.join("cef"))
            .unwrap_or_else(|e| fail(format!("{}: cef link: {e}", pkg.name)));
    }
    let components = serde_json::json!([{
        "name": pkg.name,
        "purl": format!("pkg:generic/{}@{version}", pkg.name),
        "version": version,
    }]);
    fs::write(dir.join("components.json"), format!("{components}\n"))
        .unwrap_or_else(|e| fail(format!("{}: components.json: {e}", pkg.name)));
    println!("[oath] staged {}", dir.display());
    dir
}

/// The contents of `from` into `to`, symlinks kept (the cursor theme's
/// CSS-name aliases are links).
fn copy_tree(from: &Path, to: &Path) {
    let status = Command::new("cp")
        .arg("-a")
        .arg(from.join("."))
        .arg(to)
        .status()
        .unwrap_or_else(|e| fail(format!("cp: {e}")));
    if !status.success() {
        fail(format!("cp -a {} {} failed", from.display(), to.display()));
    }
}

fn fail(msg: String) -> ! {
    eprintln!("cargo make oath: {msg}");
    exit(1)
}
