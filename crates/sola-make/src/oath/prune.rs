//! What `pkg:sola-crates` leaves out: whatever a build here never reads.
//! A crate this target does not compile keeps only what cargo needs to
//! resolve it ([`stub`]); one it does loses its tests, examples, benches and
//! other targets' generated code ([`strip`]). A wrong entry fails the build,
//! it cannot change a program: code that is gone cannot be compiled in.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// Not read when a crate is built as a dependency, unless its own sources
/// point there ([`named_by_sources`]).
const NEVER_READ: &[&str] = &[
    ".github",
    "CHANGELOG.md",
    "Cargo.lock",
    "Cargo.toml.orig",
    "benches",
    "examples",
    "fuzz",
    "test",
    "testdata",
    "tests",
];

/// One crate's own bulk that a build never reads.
const NEVER_READ_BY: &[(&str, &[&str])] = &[
    ("hpke", &["test-vectors-5f503c5.json"]),
    ("libsqlite3-sys", &["sqlcipher"]),
    ("mail-parser", &["resources"]),
    ("tracing", &["test-macros"]),
];

/// Directories of one generated file per target (`mod.rs` picks the target's).
const FILE_PER_TARGET: &[(&str, &[&str])] = &[
    ("cef", &["src/bindings", "src/resources"]),
    ("cef-dll-sys", &["src/bindings"]),
];

/// Directories of one directory per architecture.
const DIR_PER_ARCH: &[(&str, &str)] = &[("linux-raw-sys", "src")];

pub fn prune(vendor: &Path) -> Result<(), String> {
    let host = host()?;
    let compiled = compiled(&host)?;
    for entry in fs::read_dir(vendor).map_err(io("vendor"))? {
        let entry = entry.map_err(io("vendor"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let dir = entry.path();
        if compiled.contains(&name) {
            strip(&dir, &name, &host)?;
        } else {
            stub(&dir)?;
        }
        drop_missing_targets(&dir)?;
        forget_file_sums(&dir)?;
    }
    Ok(())
}

/// This machine's target triple.
fn host() -> Result<String, String> {
    let out = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|e| format!("rustc -vV: {e}"))?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(str::to_string)
        .ok_or_else(|| "rustc -vV: no host".to_string())
}

/// The vendor directories (`name-version`) the workspace compiles for `host`.
/// From `cargo tree`, which resolves features as a build does; `cargo metadata`
/// also counts what dev-dependencies turn on.
fn compiled(host: &str) -> Result<HashSet<String>, String> {
    let out = Command::new("cargo")
        .args(["tree", "--workspace", "--locked", "--target", host])
        .args(["--edges", "normal,build", "--prefix", "none", "--format", "{p}"])
        .output()
        .map_err(|e| format!("cargo tree: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo tree: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).lines().filter_map(vendor_dir).collect())
}

/// The vendor directory of a `cargo tree` line (`name v1.2.3 (source) (*)`).
fn vendor_dir(line: &str) -> Option<String> {
    let mut words = line.split_whitespace();
    let name = words.next()?;
    let version = words.next()?.strip_prefix('v')?;
    Some(format!("{name}-{version}"))
}

/// `dir` (`name-version`) holds crate `name`.
fn is_crate(dir: &str, name: &str) -> bool {
    dir.strip_prefix(name)
        .and_then(|rest| rest.strip_prefix('-'))
        .is_some_and(|version| version.starts_with(|c: char| c.is_ascii_digit()))
}

/// A crate this target never compiles: its manifest and empty sources, which
/// is all cargo reads of it.
fn stub(dir: &Path) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(io("stub"))? {
        let entry = entry.map_err(io("stub"))?;
        if !matches!(entry.file_name().to_str(), Some("Cargo.toml" | ".cargo-checksum.json")) {
            remove(&entry.path())?;
        }
    }
    fs::create_dir_all(dir.join("src")).map_err(io("stub"))?;
    fs::write(dir.join("src/lib.rs"), "").map_err(io("stub"))?;
    fs::write(dir.join("build.rs"), "").map_err(io("stub"))?;
    let mut manifest = read_manifest(dir)?;
    if let Some(lib) = manifest.get_mut("lib").and_then(toml::Value::as_table_mut) {
        lib.insert("path".into(), "src/lib.rs".into());
    }
    if let Some(package) = manifest.get_mut("package").and_then(toml::Value::as_table_mut) {
        if package.get("build").is_some_and(toml::Value::is_str) {
            package.insert("build".into(), "build.rs".into());
        }
    }
    write_manifest(dir, &manifest)
}

/// A crate this target compiles, without what the compile never reads.
fn strip(dir: &Path, name: &str, host: &str) -> Result<(), String> {
    let sources = rust_sources(&dir.join("src"));
    for path in NEVER_READ {
        if !named_by_sources(&sources, path) {
            remove(&dir.join(path))?;
        }
    }
    let own = NEVER_READ_BY
        .iter()
        .filter(|(krate, _)| is_crate(name, krate))
        .flat_map(|(_, paths)| paths.iter());
    for path in own {
        remove(&dir.join(path))?;
    }
    for (krate, dirs) in FILE_PER_TARGET {
        if is_crate(name, krate) {
            for sub in *dirs {
                let keep = format!("{}.rs", host.replace('-', "_"));
                for entry in fs::read_dir(dir.join(sub)).map_err(io(sub))? {
                    let entry = entry.map_err(io(sub))?;
                    let file = entry.file_name();
                    if !matches!(file.to_str(), Some(f) if f == "mod.rs" || f == keep) {
                        remove(&entry.path())?;
                    }
                }
            }
        }
    }
    let arch = host.split('-').next().unwrap_or(host);
    for (krate, sub) in DIR_PER_ARCH {
        if is_crate(name, krate) {
            for entry in fs::read_dir(dir.join(sub)).map_err(io(sub))? {
                let entry = entry.map_err(io(sub))?;
                if entry.path().is_dir() && entry.file_name().to_str() != Some(arch) {
                    remove(&entry.path())?;
                }
            }
        }
    }
    Ok(())
}

/// Every `.rs` file under `dir`, as text.
fn rust_sources(dir: &Path) -> Vec<String> {
    let mut sources = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                todo.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                // A file that is not UTF-8 cannot name a path for a macro.
                sources.extend(fs::read_to_string(&path));
            }
        }
    }
    sources
}

/// A crate's sources may read a top-level entry at compile time, as
/// `include_str!("../examples/a.rs")` or `#[path = "../tests/a.rs"]`: it stays.
fn named_by_sources(sources: &[String], entry: &str) -> bool {
    let named = format!("../{entry}");
    sources.iter().any(|source| source.contains(&named))
}

/// Cargo refuses a manifest whose targets name a file that is gone.
fn drop_missing_targets(dir: &Path) -> Result<(), String> {
    let mut manifest = read_manifest(dir)?;
    let mut changed = false;
    for kind in ["bin", "test", "bench", "example"] {
        if let Some(targets) = manifest.get_mut(kind).and_then(toml::Value::as_array_mut) {
            let before = targets.len();
            targets.retain(|target| {
                let path = target.get("path").and_then(toml::Value::as_str);
                path.is_none_or(|p| dir.join(p).exists())
            });
            changed |= targets.len() != before;
        }
    }
    if changed {
        write_manifest(dir, &manifest)?;
    }
    Ok(())
}

/// Cargo checks the files a vendored crate's checksum file lists; it lists
/// none now. The pack itself is content-addressed.
fn forget_file_sums(dir: &Path) -> Result<(), String> {
    let path = dir.join(".cargo-checksum.json");
    let sums: Value = fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or_else(|| format!("{}: no checksum file", dir.display()))?;
    let kept = serde_json::json!({ "files": {}, "package": sums["package"] });
    fs::write(&path, kept.to_string()).map_err(io("checksum"))
}

fn read_manifest(dir: &Path) -> Result<toml::Table, String> {
    let path = dir.join("Cargo.toml");
    fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|s| s.parse().map_err(|e: toml::de::Error| e.to_string()))
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn write_manifest(dir: &Path, manifest: &toml::Table) -> Result<(), String> {
    let text = toml::to_string(manifest).map_err(|e| format!("{}: {e}", dir.display()))?;
    fs::write(dir.join("Cargo.toml"), text).map_err(io("manifest"))
}

/// Remove a file or directory, if it is there.
fn remove(path: &Path) -> Result<(), String> {
    let done = if path.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) };
    match done {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {e}", path.display())),
        _ => Ok(()),
    }
}

fn io(what: &str) -> impl Fn(std::io::Error) -> String {
    let what = what.to_string();
    move |e| format!("prune {what}: {e}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sola-prune-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_crate_is_told_from_a_longer_name() {
        assert!(is_crate("cef-156.3.0+156.0.15", "cef"));
        assert!(!is_crate("cef-dll-sys-156.3.0+156.0.15", "cef"));
        assert!(is_crate("kem-0.3.0-pre.0", "kem"));
    }

    #[test]
    fn a_tree_line_names_its_vendor_directory() {
        assert_eq!(vendor_dir("zerocopy v0.8.55").as_deref(), Some("zerocopy-0.8.55"));
        assert_eq!(vendor_dir("cef v156.3.0+156.0.15 (*)").as_deref(), Some("cef-156.3.0+156.0.15"));
        assert_eq!(vendor_dir("sola-kit v0.1.0 (/home/Sola/crates/sola-kit)").as_deref(), Some("sola-kit-0.1.0"));
        assert_eq!(vendor_dir(""), None);
    }

    #[test]
    fn a_target_whose_file_went_leaves_the_manifest() {
        let dir = temp("targets");
        fs::create_dir_all(dir.join("src/bin")).unwrap();
        fs::write(dir.join("src/bin/kept.rs"), "").unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n\n\
             [[bin]]\nname = \"kept\"\npath = \"src/bin/kept.rs\"\n\n\
             [[test]]\nname = \"gone\"\npath = \"tests/gone.rs\"\n",
        )
        .unwrap();
        drop_missing_targets(&dir).unwrap();
        let manifest = read_manifest(&dir).unwrap();
        assert_eq!(manifest["bin"].as_array().unwrap().len(), 1);
        assert!(manifest["test"].as_array().unwrap().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_stub_keeps_what_cargo_reads_and_the_crate_s_checksum() {
        let dir = temp("stub");
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/big.rs"), "x").unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\nbuild = \"tools/b.rs\"\n\n\
             [lib]\npath = \"lib/a.rs\"\n",
        )
        .unwrap();
        fs::write(dir.join(".cargo-checksum.json"), r#"{"files":{"src/big.rs":"00"},"package":"abc"}"#).unwrap();
        stub(&dir).unwrap();
        forget_file_sums(&dir).unwrap();
        assert!(!dir.join("src/big.rs").exists());
        assert!(dir.join("src/lib.rs").exists() && dir.join("build.rs").exists());
        let manifest = read_manifest(&dir).unwrap();
        assert_eq!(manifest["lib"]["path"].as_str(), Some("src/lib.rs"));
        assert_eq!(manifest["package"]["build"].as_str(), Some("build.rs"));
        let sums: Value = serde_json::from_slice(&fs::read(dir.join(".cargo-checksum.json")).unwrap()).unwrap();
        assert_eq!(sums["package"], "abc");
        assert_eq!(sums["files"], serde_json::json!({}));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_directory_the_sources_read_stays() {
        let dir = temp("named");
        fs::create_dir_all(dir.join("src/_topic")).unwrap();
        fs::create_dir_all(dir.join("examples")).unwrap();
        fs::create_dir_all(dir.join("benches")).unwrap();
        fs::write(dir.join("src/lib.rs"), "").unwrap();
        fs::write(dir.join("src/_topic/a.rs"), r#"include!("../../examples/a.rs");"#).unwrap();
        strip(&dir, "a-1.0.0", "x86_64-unknown-linux-gnu").unwrap();
        assert!(dir.join("examples").exists());
        assert!(!dir.join("benches").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_strip_leaves_the_target_s_generated_code_and_the_sources() {
        let dir = temp("strip");
        fs::create_dir_all(dir.join("src/bindings")).unwrap();
        fs::create_dir_all(dir.join("src/resources")).unwrap();
        fs::create_dir_all(dir.join("tests")).unwrap();
        for f in ["src/lib.rs", "src/bindings/mod.rs", "src/bindings/x86_64_unknown_linux_gnu.rs", "src/bindings/aarch64_apple_darwin.rs", "README.md"] {
            fs::write(dir.join(f), "").unwrap();
        }
        strip(&dir, "cef-1.0.0", "x86_64-unknown-linux-gnu").unwrap();
        assert!(!dir.join("tests").exists());
        assert!(!dir.join("src/bindings/aarch64_apple_darwin.rs").exists());
        for f in ["src/lib.rs", "src/bindings/mod.rs", "src/bindings/x86_64_unknown_linux_gnu.rs", "README.md"] {
            assert!(dir.join(f).exists(), "{f}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }
}
