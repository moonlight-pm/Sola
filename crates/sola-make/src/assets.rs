//! Asset pack management.
//!
//! `cargo make assets sync` makes `/opt/sola/share/<category>/<pack>/`
//! match every pack listed in `crates/sola-assets/upstream.toml`:
//!
//! - Packs whose installed pin already matches the desired pin are
//!   skipped (no network, no copy).
//! - Packs that are missing or out of date are pulled fresh.
//! - Pack directories that are no longer declared in `upstream.toml`
//!   are removed from `/opt/sola/share/<category>/`.
//!
//! The installed pin and an "intent hash" (covering `src_dir`,
//! `kind`, and the `files` list) are recorded in `<dest>/.solapack`
//! after every successful pull. `sync` re-pulls when either drifts —
//! so editing a pack's `files = [...]` list invalidates the manifest
//! even when the upstream pin is unchanged. Nothing is committed to
//! the repo — `cargo make install` calls `sync(false)` automatically
//! so a fresh clone bootstraps itself.
//!
//! Pins:
//! - `github:` packs with a non-empty `rev` pin to that rev (no
//!   network at sync time when the manifest matches).
//! - `github:` packs with an empty `rev` track the default branch.
//!   Their installed pin is whatever HEAD resolved to on the last
//!   pull; pass `--refresh` to re-resolve via `git ls-remote`.
//! - `nix-pkg:` packs pin to the resolved store path. `nix-build` is
//!   invoked every sync (cache hit is fast); a changed store path
//!   triggers a re-pull.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use serde::Deserialize;

pub const UPSTREAM_TOML: &str = "crates/sola-assets/upstream.toml";
/// Runtime location — mirrors `sola_assets::ASSETS_DIR`.
const SHARE_ROOT: &str = "/opt/sola/share";
/// Per-pack pin file (written under `<dest>/`) used by `sync` to
/// decide whether the pack is up to date.
const MANIFEST_FILE: &str = ".solapack";

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

/// Synchronize `/opt/sola/share` with `upstream.toml`. Skips packs
/// that are already on the desired pin; re-pulls packs that are
/// missing, empty, or pinned differently; removes pack directories
/// that aren't declared in `upstream.toml`.
///
/// `refresh = true` re-resolves the upstream HEAD of `github:` packs
/// with an empty `rev` so that tracking-the-default-branch packs can
/// move forward. Without it, those packs stay pinned to whatever HEAD
/// resolved to on the last pull.
pub fn sync(refresh: bool) {
    let upstream = read_upstream();
    for (name, pack) in &upstream.packs {
        sync_pack(name, pack, refresh);
    }
    remove_orphans(&upstream);
    println!("sync complete");
}

/// Resolved upstream pin for a pack plus, for nix-pkg packs, the
/// store path that `nix-build` just yielded — caching it here lets
/// `pull_pack` re-use the result instead of invoking `nix-build` a
/// second time.
struct ResolvedSource {
    pin: String,
    nix_path: Option<PathBuf>,
}

/// Per-pack sync step: resolve the upstream pin, compare to the
/// installed manifest (both pin AND intent hash), pull if either
/// drifts.
fn sync_pack(name: &str, pack: &Pack, refresh: bool) {
    let dest = dest_dir(name, pack);
    let installed = read_manifest(&dest);
    let populated = dest_populated(&dest);
    let desired_intent = pack_intent_hash(pack);

    // Fast path: pinned github pack with a matching manifest (both
    // pin and intent) needs no network at all.
    if populated && let Some(ref inst) = installed {
        let intent_matches = inst.intent.as_deref() == Some(desired_intent.as_str());
        if pack.source.starts_with("github:") && intent_matches {
            if !pack.rev.is_empty() && inst.pin == pack.rev {
                println!("{name}: up to date ({})", short_pin(&inst.pin));
                return;
            }
            if pack.rev.is_empty() && !refresh {
                println!(
                    "{name}: up to date ({}) [tracking default branch — pass --refresh to re-resolve]",
                    short_pin(&inst.pin)
                );
                return;
            }
        }
        // nix-pkg always needs nix-build to know the current store
        // path; an intent mismatch on any pack also falls through so
        // we re-pull with the new file list.
    }

    let resolved = resolve_upstream(name, pack);

    let pin_matches = installed
        .as_ref()
        .map(|m| m.pin == resolved.pin)
        .unwrap_or(false);
    let intent_matches = installed
        .as_ref()
        .and_then(|m| m.intent.as_deref())
        == Some(desired_intent.as_str());
    if populated && pin_matches && intent_matches {
        println!("{name}: up to date ({})", short_pin(&resolved.pin));
        return;
    }

    let reason = if installed.is_none() {
        "no manifest"
    } else if !populated {
        "empty dest"
    } else if !pin_matches {
        "pin changed"
    } else {
        "files changed"
    };
    println!(
        "{name}: pulling ({reason}) -> {}",
        short_pin(&resolved.pin)
    );
    pull_pack(name, pack, resolved.nix_path);
    write_manifest(&dest, &resolved.pin, &desired_intent);
}

/// Where a pack's content lives under `/opt/sola/share/`.
fn dest_dir(name: &str, pack: &Pack) -> PathBuf {
    PathBuf::from(SHARE_ROOT).join(&pack.category).join(name)
}

/// `true` if `dest` exists and contains at least one entry.
fn dest_populated(dest: &Path) -> bool {
    fs::read_dir(dest)
        .map(|mut d| d.next().is_some())
        .unwrap_or(false)
}

/// Parsed `.solapack` manifest written by a previous sync.
struct InstalledManifest {
    pin: String,
    /// `None` for legacy single-line manifests written before intent
    /// tracking landed; treated as "intent unknown → re-pull".
    intent: Option<String>,
}

/// Read the installed manifest, if any. Returns `None` when the file
/// is missing or empty.
fn read_manifest(dest: &Path) -> Option<InstalledManifest> {
    let raw = fs::read_to_string(dest.join(MANIFEST_FILE)).ok()?;
    let mut lines = raw.lines();
    let pin = lines.next()?.trim().to_string();
    if pin.is_empty() {
        return None;
    }
    let intent = lines.next().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    Some(InstalledManifest { pin, intent })
}

/// Hash of the per-pack "intent" — everything other than the upstream
/// source that affects what ends up on disk: which subtree we copy
/// from, what flavor of copy, and (for fonts) which files. Two packs
/// that resolve to the same pin but a different `files = [...]` list
/// must yield different intent hashes so sync re-pulls when the user
/// changes that list. Hash is non-cryptographic — just a change
/// detector.
fn pack_intent_hash(pack: &Pack) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    pack.src_dir.hash(&mut h);
    format!("{:?}", pack.kind).hash(&mut h);
    pack.files.len().hash(&mut h);
    for entry in &pack.files {
        match entry {
            FileEntry::Plain(s) => {
                0u8.hash(&mut h);
                s.hash(&mut h);
            }
            FileEntry::Mapped {
                from,
                to,
                transform,
            } => {
                1u8.hash(&mut h);
                from.hash(&mut h);
                to.hash(&mut h);
                transform.hash(&mut h);
            }
        }
    }
    format!("{:016x}", h.finish())
}

/// Write the resolved pin + intent hash to `<dest>/.solapack`.
/// Two-line format keeps it human-readable while still capturing the
/// pack's intent so file-list edits invalidate the manifest.
fn write_manifest(dest: &Path, pin: &str, intent: &str) {
    let path = dest.join(MANIFEST_FILE);
    if let Err(e) = fs::write(&path, format!("{pin}\n{intent}\n")) {
        eprintln!("failed to write {}: {e}", path.display());
        exit(1);
    }
}

/// Trim long pins for readable progress output. SHAs collapse to 12
/// hex chars; store paths stay as-is (already self-explanatory).
fn short_pin(pin: &str) -> &str {
    if pin.starts_with("/nix/store/") {
        return pin;
    }
    if pin.len() > 12 && pin.chars().all(|c| c.is_ascii_hexdigit()) {
        return &pin[..12];
    }
    pin
}

/// Resolve the upstream's current pin without populating the dest.
/// For github sources this means the rev (or `git ls-remote HEAD`
/// for tracking-default packs); for nix-pkg sources it's the store
/// path that `nix-build` resolves to.
fn resolve_upstream(name: &str, pack: &Pack) -> ResolvedSource {
    if let Some(slug) = pack.source.strip_prefix("github:") {
        let pin = if pack.rev.is_empty() {
            ls_remote_head(slug)
        } else {
            pack.rev.clone()
        };
        ResolvedSource { pin, nix_path: None }
    } else if let Some(attr) = pack.source.strip_prefix("nix-pkg:") {
        let path = nix_build_attr(attr);
        ResolvedSource {
            pin: path.to_string_lossy().to_string(),
            nix_path: Some(path),
        }
    } else {
        eprintln!("{name}: unsupported source format: {}", pack.source);
        exit(1);
    }
}

/// Query the remote's HEAD without cloning. Single network roundtrip;
/// only used when a tracking-default pack actually needs refreshing.
fn ls_remote_head(slug: &str) -> String {
    let url = format!("https://github.com/{slug}.git");
    let output = Command::new("git")
        .args(["ls-remote", &url, "HEAD"])
        .output()
        .unwrap_or_else(|e| {
            eprintln!("git ls-remote {url} failed: {e}");
            exit(1);
        });
    if !output.status.success() {
        eprintln!(
            "git ls-remote {url} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        exit(1);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let sha = stdout
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().next())
        .unwrap_or("")
        .to_string();
    if sha.is_empty() {
        eprintln!("git ls-remote {url}: no HEAD in output");
        exit(1);
    }
    sha
}

/// Resolve a nixpkgs attribute to its store path. Cheap on cache hit;
/// repeated calls within one `sync` are avoided by stashing the result
/// in `ResolvedSource::nix_path` for `pull_pack`'s benefit.
fn nix_build_attr(attr: &str) -> PathBuf {
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
    PathBuf::from(path)
}

/// Remove pack directories under `/opt/sola/share/<category>/` that
/// aren't declared in `upstream.toml`. Only categories that currently
/// contain at least one declared pack are scanned, so a hand-managed
/// directory under an unrelated category name is left alone.
fn remove_orphans(upstream: &Upstream) {
    let categories: BTreeSet<&str> = upstream
        .packs
        .values()
        .map(|p| p.category.as_str())
        .collect();
    let declared: BTreeSet<PathBuf> = upstream
        .packs
        .iter()
        .map(|(name, pack)| dest_dir(name, pack))
        .collect();

    for category in categories {
        let cat_path = Path::new(SHARE_ROOT).join(category);
        let Ok(entries) = fs::read_dir(&cat_path) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if declared.contains(&path) {
                continue;
            }
            println!("removing orphan: {}", path.display());
            if let Err(e) = fs::remove_dir_all(&path) {
                eprintln!("failed to remove {}: {e}", path.display());
            }
        }
    }
}

/// `true` if any pack referenced in `upstream.toml` is missing,
/// has no manifest, or has an out-of-date intent hash. Used by
/// `install` to print a single "Syncing assets…" banner before
/// delegating to `sync`. Avoids the per-pack chatter when there's
/// nothing to do.
pub fn needs_sync() -> bool {
    let upstream = read_upstream();
    upstream.packs.iter().any(|(name, pack)| {
        let dest = dest_dir(name, pack);
        if !dest_populated(&dest) {
            return true;
        }
        match read_manifest(&dest) {
            None => true,
            Some(m) => m.intent.as_deref() != Some(pack_intent_hash(pack).as_str()),
        }
    })
}

fn pull_pack(name: &str, pack: &Pack, nix_resolved: Option<PathBuf>) {
    println!("pulling {name} from {}", pack.source);

    let root = fetch_source(name, &pack.source, &pack.rev, nix_resolved);

    let src = root.path().join(&pack.src_dir);
    if !src.is_dir() {
        eprintln!(
            "{name}: source directory {} not found in source root",
            src.display()
        );
        exit(1);
    }

    let dest = dest_dir(name, pack);
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
            // XDG-conventional location is the parent of the cursors
            // directory (McMojave's `dist/index.theme`); Adwaita keeps
            // it at the repo root. Search both.
            let theme_candidates = [
                src.parent().map(|p| p.join("index.theme")),
                Some(root.path().join("index.theme")),
            ];
            let theme_src = theme_candidates
                .into_iter()
                .flatten()
                .find(|p| p.is_file());
            if let Some(theme_src) = theme_src {
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
                    "{name}: warning: no index.theme found next to cursors or at repo root"
                );
            }
        }
    }
}

/// Acquire a source tree for the pack. Returns a path that contains the
/// upstream content; resource cleanup (if any) is tied to the returned
/// `SourceRoot` via Drop.
///
/// `nix_resolved` short-circuits the `nix-build` invocation when
/// `sync_pack` already resolved the store path during pin resolution.
fn fetch_source(
    name: &str,
    source: &str,
    rev: &str,
    nix_resolved: Option<PathBuf>,
) -> SourceRoot {
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
    } else if source.strip_prefix("nix-pkg:").is_some() {
        if let Some(p) = nix_resolved {
            return SourceRoot::Pinned(p);
        }
        // Fall through to a fresh nix-build only if no pre-resolved
        // path was supplied (shouldn't happen via sync_pack, but keeps
        // the function usable from elsewhere).
        let attr = source.strip_prefix("nix-pkg:").unwrap();
        SourceRoot::Pinned(nix_build_attr(attr))
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
