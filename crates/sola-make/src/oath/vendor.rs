//! `pkg:sola-crates`: every crate Sola's `Cargo.lock` names (crates.io and
//! git), from `cargo vendor`, so a plan builds offline, without what a
//! build never reads ([`super::prune`]).
//!
//! The tree is `vendor/` plus `vendor.toml`, cargo's source replacement
//! with the vendor path as `@VENDOR@` for the plan to fill in.

use std::fs;
use std::path::Path;
use std::process::Command;

pub fn vendor(dir: &Path) -> Result<(), String> {
    let vendor = dir.join("vendor");
    let out = Command::new("cargo")
        .args(["vendor", "--locked", "--versioned-dirs"])
        .arg(&vendor)
        .output()
        .map_err(|e| format!("cargo vendor: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo vendor: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    super::prune::prune(&vendor)?;
    let shown = format!("\"{}\"", vendor.display());
    let config: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.starts_with("To use vendored sources"))
        .map(|l| l.replace(&shown, "\"@VENDOR@\""))
        .collect();
    let config = config.join("\n").trim().to_string() + "\n";
    if !config.contains("@VENDOR@") {
        return Err("cargo vendor printed no source replacement".into());
    }
    fs::write(dir.join("vendor.toml"), config).map_err(|e| format!("vendor.toml: {e}"))
}
