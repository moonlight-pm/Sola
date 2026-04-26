//! Asset pack management.
//!
//! `cargo make assets pull` refreshes vendored third-party assets (icons, etc.)
//! from the sources pinned in `crates/sola-assets/upstream.toml`.
//!
//! The fetched files are committed to the repo so clean clones build offline.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use serde::Deserialize;

const UPSTREAM_TOML: &str = "crates/sola-assets/upstream.toml";
const ASSETS_ROOT: &str = "crates/sola-assets/assets";

#[derive(Debug, Deserialize)]
struct Upstream {
    packs: std::collections::BTreeMap<String, Pack>,
}

#[derive(Debug, Deserialize)]
struct Pack {
    /// e.g. "github:lucide-icons/lucide"
    source: String,
    /// Git ref (branch, tag, or commit). Empty string means default branch.
    #[serde(default)]
    rev: String,
    /// Path (relative to repo root) containing the source files.
    src_dir: String,
    /// Destination category under `assets/` (e.g. "icons", "cursors").
    category: String,
    /// Pack flavor. Controls which files are copied:
    /// - `"icons"` (default): flat copy of every `.svg` from `src_dir`.
    /// - `"cursors"`: copy every file in `src_dir` (skipping `.cur` /
    ///   `.ani` Windows variants) into `<category>/<name>/cursors/`,
    ///   plus the repo-root `index.theme` into `<category>/<name>/`.
    /// - `"fonts"`: copy each filename in `files` from `src_dir` into
    ///   `<category>/<name>/`.
    #[serde(default)]
    kind: PackKind,
    /// For `kind = "fonts"`: explicit list of filenames to copy from
    /// `src_dir`. Other kinds ignore this field.
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Debug, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
enum PackKind {
    #[default]
    Icons,
    Cursors,
    Fonts,
}

pub fn pull() {
    let raw = fs::read_to_string(UPSTREAM_TOML).unwrap_or_else(|e| {
        eprintln!("failed to read {UPSTREAM_TOML}: {e}");
        exit(1);
    });
    let upstream: Upstream = toml::from_str(&raw).unwrap_or_else(|e| {
        eprintln!("failed to parse {UPSTREAM_TOML}: {e}");
        exit(1);
    });

    for (name, pack) in &upstream.packs {
        pull_pack(name, pack);
    }
    println!("all packs pulled");
}

fn pull_pack(name: &str, pack: &Pack) {
    println!("pulling {name} from {}", pack.source);

    let clone_url = resolve_source(&pack.source).unwrap_or_else(|| {
        eprintln!("unsupported source format: {}", pack.source);
        exit(1);
    });

    let tmp = tempdir_for(name);
    let mut args = vec!["clone", "--quiet", "--no-tags"];
    if pack.rev.is_empty() {
        // Fast path for "latest": shallow-clone the default branch.
        args.push("--depth");
        args.push("1");
    }
    args.push(clone_url.as_str());
    let tmp_str = tmp.to_str().unwrap();
    args.push(tmp_str);
    run("git", &args);

    if !pack.rev.is_empty() {
        run("git", &["-C", tmp_str, "checkout", "--quiet", &pack.rev]);
    }

    let src = tmp.join(&pack.src_dir);
    if !src.is_dir() {
        eprintln!(
            "{name}: source directory {} not found in cloned repo",
            src.display()
        );
        exit(1);
    }

    let dest = PathBuf::from(ASSETS_ROOT).join(&pack.category).join(name);
    wipe_dir_keep_gitkeep(&dest);

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
            for file in &pack.files {
                let from = src.join(file);
                let to = dest.join(file);
                if let Err(e) = fs::copy(&from, &to) {
                    eprintln!("failed to copy {} -> {}: {e}", from.display(), to.display());
                    exit(1);
                }
            }
            println!("  {} font files -> {}", pack.files.len(), dest.display());
        }
        PackKind::Cursors => {
            let cursors_dest = dest.join("cursors");
            let count = copy_cursor_files(&src, &cursors_dest);
            println!("  {count} cursors -> {}", cursors_dest.display());
            // Cursor themes need an `index.theme` next to `cursors/`.
            // Adwaita keeps it at the repo root; copy it into place.
            let theme_src = tmp.join("index.theme");
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
                    "{name}: warning: no index.theme at repo root ({})",
                    theme_src.display()
                );
            }
        }
    }

    let _ = fs::remove_dir_all(&tmp);
}

fn resolve_source(source: &str) -> Option<String> {
    source
        .strip_prefix("github:")
        .map(|slug| format!("https://github.com/{slug}.git"))
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

fn wipe_dir_keep_gitkeep(dir: &Path) {
    if !dir.is_dir() {
        fs::create_dir_all(dir).ok();
        return;
    }
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) == Some(".gitkeep") {
            continue;
        }
        if path.is_dir() {
            let _ = fs::remove_dir_all(&path);
        } else {
            let _ = fs::remove_file(&path);
        }
    }
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
