//! `pkg:sola-crates`: every crate Sola's `Cargo.lock` names (crates.io and
//! git), from `cargo vendor`, so a plan builds offline. Windows-only
//! crates keep only their manifests, as Oath vendored them.
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
    stub_windows_crates(&vendor)?;
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

/// `windows*` / `winapi*` crates down to `Cargo.toml`, `build.rs` and an
/// empty `src/lib.rs`, with a checksum file that lists no files.
fn stub_windows_crates(vendor: &Path) -> Result<(), String> {
    let err = |e: std::io::Error| format!("stub windows crates: {e}");
    for entry in fs::read_dir(vendor).map_err(err)? {
        let entry = entry.map_err(err)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("windows") || name.starts_with("winapi")) {
            continue;
        }
        let dir = entry.path();
        let sums: serde_json::Value = fs::read(dir.join(".cargo-checksum.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .ok_or_else(|| format!("{name}: no .cargo-checksum.json"))?;
        for f in fs::read_dir(&dir).map_err(err)? {
            let f = f.map_err(err)?;
            if matches!(f.file_name().to_str(), Some("Cargo.toml" | "build.rs")) {
                continue;
            }
            let path = f.path();
            if path.is_dir() {
                fs::remove_dir_all(&path).map_err(err)?;
            } else {
                fs::remove_file(&path).map_err(err)?;
            }
        }
        fs::create_dir_all(dir.join("src")).map_err(err)?;
        fs::write(dir.join("src/lib.rs"), "").map_err(err)?;
        let stub = serde_json::json!({ "files": {}, "package": sums["package"] });
        fs::write(dir.join(".cargo-checksum.json"), stub.to_string()).map_err(err)?;
    }
    Ok(())
}
