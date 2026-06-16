//! Build, bundle, and publish a Sola release tarball to GitHub Releases.
//!
//! Pipeline:
//!   1. Validate working tree clean.
//!   2. Resolve version (auto-bump latest tag's patch, or use explicit arg).
//!   3. `cargo build --release` (strip = "debuginfo" set in root Cargo.toml).
//!   4. Stage release binaries + the patched CEF Release/ tree.
//!   5. Pre-patch CEF-linking binaries' RUNPATH to `/opt/sola/cef`
//!      (so the tarball is usable bare too — the Nix derivation re-rpaths
//!      to the store path on install).
//!   6. tar + zstd-19 compress.
//!   7. Compute SRI hash via `nix hash file`.
//!   8. Rewrite `nix/release.nix` with the new version + hash.
//!   9. Commit, tag, push to `github`, create the GitHub release.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const RELEASE_REPO: &str = "moonlight-pm/Sola";
const REMOTE: &str = "origin";
const BRANCH: &str = "master";

/// Binaries that dynamically link `libcef.so` — their RUNPATH needs
/// updating from the build host's `~/.cache/sola/cef-…/Release` path
/// to a stable consumer-side location. Currently empty; add any
/// CEF-linking binary that ships in a release tarball here.
const CEF_LINKING_BINS: &[&str] = &[];

pub fn publish(explicit_version: Option<String>) {
    match run_publish(explicit_version) {
        Ok(version) => println!("\n✓ Published v{version}"),
        Err(e) => {
            eprintln!("publish failed: {e}");
            std::process::exit(1);
        }
    }
}

fn run_publish(explicit_version: Option<String>) -> Result<String, String> {
    require_clean_tree()?;
    let version = resolve_version(explicit_version)?;
    let tag = format!("v{version}");
    require_tag_unique(&tag)?;
    println!(">>> publishing {tag}");

    println!(">>> cargo build --release (workspace)");
    run("cargo", &["build", "--release"])?;

    let staging = mkstaging()?;
    let bundle = staging.join("sola");
    let bin_dir = bundle.join("bin");
    let cef_dir = bundle.join("cef");
    let share_dir = bundle.join("share");
    fs::create_dir_all(&bin_dir).map_err(|e| format!("mkdir bin: {e}"))?;
    fs::create_dir_all(&cef_dir).map_err(|e| format!("mkdir cef: {e}"))?;
    fs::create_dir_all(&share_dir).map_err(|e| format!("mkdir share: {e}"))?;

    println!(">>> staging binaries from target/release");
    for name in crate::discover_binaries() {
        let src = format!("target/release/{name}");
        let dst = bin_dir.join(&name);
        fs::copy(&src, &dst)
            .map_err(|e| format!("copy {src} -> {}: {e}", dst.display()))?;
    }

    println!(">>> staging /opt/sola/share (icons + cursors + applications)");
    if !Path::new("/opt/sola/share").exists() {
        return Err(
            "/opt/sola/share not found — run `cargo make assets pull` then `cargo make install`"
                .to_string(),
        );
    }
    run(
        "cp",
        &["-r", "/opt/sola/share/.", share_dir.to_str().unwrap()],
    )?;

    println!(">>> staging CEF Release tree");
    let cef_version = fs::read_to_string("cef-version")
        .map_err(|e| format!("read cef-version: {e}"))?
        .trim()
        .to_string();
    let home = std::env::var("HOME").map_err(|e| format!("HOME unset: {e}"))?;
    let cef_release = format!("{home}/.cache/sola/cef-{cef_version}/Release");
    if !Path::new(&cef_release).exists() {
        return Err(format!(
            "CEF cache not found at {cef_release} — run `cargo make install-cef` first"
        ));
    }
    run(
        "cp",
        &[
            "-r",
            &format!("{cef_release}/."),
            cef_dir.to_str().unwrap(),
        ],
    )?;

    println!(">>> pre-patching CEF-linking binaries' RUNPATH");
    for bin in CEF_LINKING_BINS {
        let path = bin_dir.join(bin);
        if !path.exists() {
            continue;
        }
        run(
            "patchelf",
            &[
                "--set-rpath",
                "/opt/sola/cef:/run/current-system/sw/share/nix-ld/lib",
                path.to_str().unwrap(),
            ],
        )?;
    }

    let tarball_name = format!("sola-{version}-linux-x86_64.tar.zst");
    let tarball = staging.join(&tarball_name);
    println!(">>> compressing -> {tarball_name}");
    // `tar` invokes the compress-program through execvp; passing the
    // `zstd -T0 -19` string as a single argv element doesn't work.
    // Pipe through sh instead — zstd reads stdin, writes the tarball.
    run(
        "sh",
        &[
            "-c",
            &format!(
                "tar -C {} -cf - . | zstd -T0 -19 -q -o {}",
                shell_quote(bundle.to_str().unwrap()),
                shell_quote(tarball.to_str().unwrap()),
            ),
        ],
    )?;
    let size = fs::metadata(&tarball).map(|m| m.len()).unwrap_or(0);
    println!("    {} bytes ({:.1} MB)", size, size as f64 / 1_048_576.0);

    println!(">>> computing SRI hash");
    let hash = capture(
        "nix",
        &[
            "hash",
            "file",
            "--type",
            "sha256",
            "--base64",
            tarball.to_str().unwrap(),
        ],
    )?;
    let sri = format!("sha256-{}", hash.trim());

    println!(">>> writing nix/release.nix");
    let body = format!("{{\n  version = \"{version}\";\n  hash = \"{sri}\";\n}}\n");
    fs::write("nix/release.nix", body).map_err(|e| format!("write release.nix: {e}"))?;

    println!(">>> commit + tag");
    run("git", &["add", "nix/release.nix"])?;
    run("git", &["commit", "-m", &format!("release: {tag}")])?;
    run("git", &["tag", "-a", &tag, "-m", &format!("Sola {version}")])?;

    println!(">>> push to {REMOTE}");
    run("git", &["push", REMOTE, BRANCH, &tag])?;

    println!(">>> gh release create {tag}");
    run(
        "gh",
        &[
            "release",
            "create",
            &tag,
            tarball.to_str().unwrap(),
            "--repo",
            RELEASE_REPO,
            "--title",
            &format!("Sola {version}"),
            "--notes",
            &format!("Sola desktop shell {version}. See INSTALL.md."),
        ],
    )?;

    let _ = fs::remove_dir_all(&staging);
    Ok(version)
}

fn require_clean_tree() -> Result<(), String> {
    let out = capture("git", &["status", "--porcelain"])?;
    if !out.trim().is_empty() {
        return Err("working tree dirty — commit or stash before publishing".to_string());
    }
    Ok(())
}

fn require_tag_unique(tag: &str) -> Result<(), String> {
    let status = Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", &format!("refs/tags/{tag}")])
        .status()
        .map_err(|e| format!("git rev-parse: {e}"))?;
    if status.success() {
        return Err(format!("tag {tag} already exists"));
    }
    Ok(())
}

/// Either use the explicit version, or auto-bump the patch component
/// of the most recent `vX.Y.Z` tag. Bails if neither is available.
fn resolve_version(explicit: Option<String>) -> Result<String, String> {
    if let Some(v) = explicit {
        validate_semver(&v)?;
        return Ok(v);
    }
    let out = Command::new("git")
        .args(["describe", "--tags", "--abbrev=0", "--match=v*.*.*"])
        .output()
        .map_err(|e| format!("git describe: {e}"))?;
    if !out.status.success() {
        return Err(
            "no existing vX.Y.Z tag — specify an explicit version: `cargo make publish 0.1.0`"
                .to_string(),
        );
    }
    let last = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let trimmed = last.strip_prefix('v').unwrap_or(&last);
    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() != 3 {
        return Err(format!("last tag '{last}' is not vX.Y.Z"));
    }
    let major: u32 = parts[0].parse().map_err(|_| format!("bad major in '{last}'"))?;
    let minor: u32 = parts[1].parse().map_err(|_| format!("bad minor in '{last}'"))?;
    let patch: u32 = parts[2].parse().map_err(|_| format!("bad patch in '{last}'"))?;
    Ok(format!("{major}.{minor}.{patch_next}", patch_next = patch + 1))
}

fn validate_semver(s: &str) -> Result<(), String> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 || !parts.iter().all(|p| p.parse::<u32>().is_ok()) {
        return Err(format!("'{s}' is not X.Y.Z"));
    }
    Ok(())
}

fn mkstaging() -> Result<PathBuf, String> {
    let out = capture("mktemp", &["-d", "-t", "sola-publish-XXXXXX"])?;
    Ok(PathBuf::from(out.trim()))
}

/// Single-quote a string for safe inclusion in a `sh -c` command line.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn run(cmd: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(cmd)
        .args(args)
        .status()
        .map_err(|e| format!("spawn {cmd}: {e}"))?;
    if !status.success() {
        return Err(format!(
            "{cmd} {args:?} exited with {}",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

fn capture(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("spawn {cmd}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{cmd} {args:?} exited with {}: {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_semver() {
        assert!(validate_semver("0.1.0").is_ok());
        assert!(validate_semver("12.34.56").is_ok());
    }

    #[test]
    fn validate_rejects_non_semver() {
        assert!(validate_semver("0.1").is_err());
        assert!(validate_semver("v0.1.0").is_err());
        assert!(validate_semver("0.1.0-rc1").is_err());
        assert!(validate_semver("foo").is_err());
    }

    #[test]
    fn shell_quote_handles_single_quotes() {
        assert_eq!(shell_quote("foo"), "'foo'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }
}
