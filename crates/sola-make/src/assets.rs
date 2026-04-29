//! Asset pack management.
//!
//! `cargo make assets pull` fetches every pack listed in
//! `crates/sola-assets/upstream.toml` and writes it under
//! `/opt/sola/share/<category>/<pack>/`. Nothing is committed to the
//! repo — `cargo make install` automatically pulls when packs are
//! missing or older than [`STALENESS_THRESHOLD`], so a fresh clone
//! just runs install and it bootstraps itself.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use std::time::{Duration, SystemTime};

use serde::Deserialize;

pub const UPSTREAM_TOML: &str = "crates/sola-assets/upstream.toml";
/// Runtime location — mirrors `sola_assets::ASSETS_DIR`.
const SHARE_ROOT: &str = "/opt/sola/share";
/// Auto-pull when any pack is older than this (1 week).
const STALENESS_THRESHOLD: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Deserialize)]
struct Upstream {
    packs: std::collections::BTreeMap<String, Pack>,
}

#[derive(Debug, Deserialize)]
struct Pack {
    /// Source format. Supported prefixes:
    /// - `github:<owner>/<repo>` — git clone
    /// - `nix-pkg:<attr>` — `nix-build '<nixos>' -A <attr>`, use the
    ///   resulting store path as the source root (offline after first build)
    source: String,
    /// Git ref (branch, tag, or commit). Empty string means default branch.
    /// Only honored for `github:` sources.
    #[serde(default)]
    rev: String,
    /// Path (relative to source root) containing the source files.
    src_dir: String,
    /// Destination category (e.g. "icons", "cursors", "fonts").
    category: String,
    /// Pack flavor. Controls which files are copied:
    /// - `"icons"` (default): flat copy of every `.svg` from `src_dir`.
    /// - `"cursors"`: copy every file in `src_dir` (skipping `.cur` /
    ///   `.ani` Windows variants) into `<category>/<name>/cursors/`,
    ///   plus the repo-root `index.theme` into `<category>/<name>/`.
    /// - `"fonts"`: copy each entry in `files` from `src_dir` into
    ///   `<category>/<name>/`.
    #[serde(default)]
    kind: PackKind,
    /// For `kind = "fonts"`: explicit list of entries to copy from
    /// `src_dir`. Each entry is either a bare filename (copied as-is) or
    /// a `{ from, to, transform? }` table. `transform = "ttf-to-woff2"`
    /// runs the input through `woff2_compress` (via nix-shell) and writes
    /// the result to `to`. Other kinds ignore this field.
    #[serde(default)]
    files: Vec<FileEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FileEntry {
    /// Bare filename — copied verbatim.
    Plain(String),
    /// Explicit src→dest mapping with optional transform.
    Mapped {
        from: String,
        to: String,
        #[serde(default)]
        transform: Option<String>,
    },
}

#[derive(Debug, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
enum PackKind {
    #[default]
    Icons,
    Cursors,
    Fonts,
}

fn read_upstream() -> Upstream {
    let raw = fs::read_to_string(UPSTREAM_TOML).unwrap_or_else(|e| {
        eprintln!("failed to read {UPSTREAM_TOML}: {e}");
        exit(1);
    });
    toml::from_str(&raw).unwrap_or_else(|e| {
        eprintln!("failed to parse {UPSTREAM_TOML}: {e}");
        exit(1);
    })
}

/// Result of acquiring a source tree for a pack. Captures whether the
/// path is owned by us (`Tmp`, must be cleaned up) or external (`Pinned`,
/// e.g. a nix store path — leave alone).
enum SourceRoot {
    Tmp(PathBuf),
    Pinned(PathBuf),
}

impl SourceRoot {
    fn path(&self) -> &Path {
        match self {
            SourceRoot::Tmp(p) | SourceRoot::Pinned(p) => p,
        }
    }
}

impl Drop for SourceRoot {
    fn drop(&mut self) {
        if let SourceRoot::Tmp(p) = self {
            let _ = fs::remove_dir_all(p);
        }
    }
}

pub fn pull() {
    let upstream = read_upstream();
    for (name, pack) in &upstream.packs {
        pull_pack(name, pack);
    }
    println!("all packs pulled");
}

/// Reason a pack needs (re-)pulling, if any. `None` = up to date.
pub fn pull_reason() -> Option<String> {
    let upstream = read_upstream();
    let now = SystemTime::now();
    let mut missing = Vec::new();
    let mut stale = Vec::new();
    for (name, pack) in &upstream.packs {
        let dest = PathBuf::from(SHARE_ROOT).join(&pack.category).join(name);
        let populated = fs::read_dir(&dest)
            .map(|d| d.flatten().next().is_some())
            .unwrap_or(false);
        if !populated {
            missing.push(name.clone());
            continue;
        }
        let stale_dir = fs::metadata(&dest)
            .and_then(|m| m.modified())
            .map(|t| {
                now.duration_since(t)
                    .map(|d| d > STALENESS_THRESHOLD)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if stale_dir {
            stale.push(name.clone());
        }
    }
    let mut parts = Vec::new();
    if !missing.is_empty() {
        parts.push(format!("missing: {}", missing.join(", ")));
    }
    if !stale.is_empty() {
        parts.push(format!("stale (>7d): {}", stale.join(", ")));
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

fn pull_pack(name: &str, pack: &Pack) {
    println!("pulling {name} from {}", pack.source);

    let root = fetch_source(name, &pack.source, &pack.rev);

    let src = root.path().join(&pack.src_dir);
    if !src.is_dir() {
        eprintln!(
            "{name}: source directory {} not found in source root",
            src.display()
        );
        exit(1);
    }

    let dest = PathBuf::from(SHARE_ROOT).join(&pack.category).join(name);
    wipe_dir(&dest);

    match pack.kind {
        PackKind::Icons => {
            let count = copy_svgs(&src, &dest);
            println!("  {count} SVGs -> {}", dest.display());
        }
        PackKind::Fonts => {
            if pack.files.is_empty() {
                eprintln!("{name}: kind=fonts requires a non-empty `files` list");
                exit(1);
            }
            fs::create_dir_all(&dest).ok();
            for entry in &pack.files {
                process_font_entry(name, entry, &src, &dest);
            }
            println!("  {} font files -> {}", pack.files.len(), dest.display());
        }
        PackKind::Cursors => {
            let cursors_dest = dest.join("cursors");
            let count = copy_cursor_files(&src, &cursors_dest);
            println!("  {count} cursors -> {}", cursors_dest.display());
            // Cursor themes need an `index.theme` next to `cursors/`.
            // Adwaita keeps it at the repo root; copy it into place.
            let theme_src = root.path().join("index.theme");
            if theme_src.is_file() {
                let theme_dest = dest.join("index.theme");
                if let Err(e) = fs::copy(&theme_src, &theme_dest) {
                    eprintln!(
                        "failed to copy {} -> {}: {e}",
                        theme_src.display(),
                        theme_dest.display()
                    );
                    exit(1);
                }
                println!("  index.theme -> {}", theme_dest.display());
            } else {
                eprintln!(
                    "{name}: warning: no index.theme at source root ({})",
                    theme_src.display()
                );
            }
        }
    }
}

/// Acquire a source tree for the pack. Returns a path that contains the
/// upstream content; resource cleanup (if any) is tied to the returned
/// `SourceRoot` via Drop.
fn fetch_source(name: &str, source: &str, rev: &str) -> SourceRoot {
    if let Some(slug) = source.strip_prefix("github:") {
        let url = format!("https://github.com/{slug}.git");
        let tmp = tempdir_for(name);
        let mut args = vec!["clone", "--quiet", "--no-tags"];
        if rev.is_empty() {
            args.push("--depth");
            args.push("1");
        }
        args.push(url.as_str());
        let tmp_str = tmp.to_str().unwrap();
        args.push(tmp_str);
        run("git", &args);
        if !rev.is_empty() {
            run("git", &["-C", tmp_str, "checkout", "--quiet", rev]);
        }
        SourceRoot::Tmp(tmp)
    } else if let Some(attr) = source.strip_prefix("nix-pkg:") {
        let output = Command::new("nix-build")
            .args(["<nixos>", "-A", attr, "--no-out-link"])
            .output()
            .unwrap_or_else(|e| {
                eprintln!("failed to run nix-build for {attr}: {e}");
                exit(1);
            });
        if !output.status.success() {
            eprintln!(
                "nix-build failed for {attr}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            exit(1);
        }
        let path = String::from_utf8_lossy(&output.stdout)
            .lines()
            .last()
            .unwrap_or("")
            .trim()
            .to_string();
        if path.is_empty() {
            eprintln!("nix-build for {attr} produced no output path");
            exit(1);
        }
        SourceRoot::Pinned(PathBuf::from(path))
    } else {
        eprintln!("{name}: unsupported source format: {source}");
        exit(1);
    }
}

/// Copy or transform a single font entry from `src` to `dest`.
fn process_font_entry(name: &str, entry: &FileEntry, src: &Path, dest: &Path) {
    let (from, to, transform) = match entry {
        FileEntry::Plain(f) => (f.as_str(), f.as_str(), None),
        FileEntry::Mapped {
            from,
            to,
            transform,
        } => (from.as_str(), to.as_str(), transform.as_deref()),
    };
    let from_path = src.join(from);
    let to_path = dest.join(to);
    match transform {
        None => {
            if let Err(e) = fs::copy(&from_path, &to_path) {
                eprintln!(
                    "{name}: failed to copy {} -> {}: {e}",
                    from_path.display(),
                    to_path.display()
                );
                exit(1);
            }
        }
        Some("ttf-to-woff2") => ttf_to_woff2(name, &from_path, &to_path),
        Some(other) => {
            eprintln!("{name}: unknown transform: {other}");
            exit(1);
        }
    }
}

/// Compress a TTF/OTF to WOFF2 using `woff2_compress` from nixpkgs.
/// `woff2_compress` writes `<input>.woff2` next to its input, so we stage
/// in a tmp dir and move the result into place.
fn ttf_to_woff2(name: &str, src: &Path, dest: &Path) {
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("font");
    let tmp = std::env::temp_dir().join(format!(
        "sola-woff2-{name}-{stem}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap_or_else(|e| {
        eprintln!("failed to create {}: {e}", tmp.display());
        exit(1);
    });
    let staged = tmp.join(src.file_name().unwrap_or_else(|| "input".as_ref()));
    if let Err(e) = fs::copy(src, &staged) {
        eprintln!(
            "{name}: failed to stage {} -> {}: {e}",
            src.display(),
            staged.display()
        );
        exit(1);
    }
    let staged_str = staged.to_string_lossy().into_owned();
    run(
        "nix-shell",
        &[
            "-p",
            "woff2",
            "--quiet",
            "--run",
            &format!("woff2_compress {}", shell_escape(&staged_str)),
        ],
    );
    let produced = staged.with_extension("woff2");
    if let Err(e) = fs::rename(&produced, dest).or_else(|_| fs::copy(&produced, dest).map(|_| ())) {
        eprintln!(
            "{name}: failed to place {} at {}: {e}",
            produced.display(),
            dest.display()
        );
        exit(1);
    }
    let _ = fs::remove_dir_all(&tmp);
}

/// Single-quote a string for safe inclusion in a shell command.
fn shell_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

fn tempdir_for(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("sola-assets-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap_or_else(|e| {
        eprintln!("failed to create {}: {e}", base.display());
        exit(1);
    });
    base
}

fn wipe_dir(dir: &Path) {
    let _ = fs::remove_dir_all(dir);
    let _ = fs::create_dir_all(dir);
}

fn copy_svgs(src: &Path, dest: &Path) -> usize {
    fs::create_dir_all(dest).ok();
    let mut count = 0;
    for entry in fs::read_dir(src).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("svg") {
            continue;
        }
        let Some(file_name) = path.file_name() else {
            continue;
        };
        let to = dest.join(file_name);
        if let Err(e) = fs::copy(&path, &to) {
            eprintln!("failed to copy {} -> {}: {e}", path.display(), to.display());
            exit(1);
        }
        count += 1;
    }
    count
}

/// Flat copy of every regular file in `src` to `dest`, skipping the
/// Windows-format `.cur` / `.ani` siblings that GNOME ships alongside
/// the real XCursor binaries — Sola is Wayland-only and they roughly
/// double the on-disk footprint.
fn copy_cursor_files(src: &Path, dest: &Path) -> usize {
    fs::create_dir_all(dest).ok();
    let mut count = 0;
    for entry in fs::read_dir(src).into_iter().flatten().flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext == "cur" || ext == "ani" {
                continue;
            }
        }
        let Some(file_name) = path.file_name() else {
            continue;
        };
        let to = dest.join(file_name);
        if let Err(e) = fs::copy(&path, &to) {
            eprintln!("failed to copy {} -> {}: {e}", path.display(), to.display());
            exit(1);
        }
        count += 1;
    }
    count
}

fn run(program: &str, args: &[&str]) {
    let status = Command::new(program)
        .args(args)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("failed to run {program}: {e}");
            exit(1);
        });
    if !status.success() {
        eprintln!(
            "{program} failed with exit code {}",
            status.code().unwrap_or(1)
        );
        exit(status.code().unwrap_or(1));
    }
}
