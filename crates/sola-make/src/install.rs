use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

pub(crate) const BIN_DIR: &str = "/opt/sola/bin";
const LOG_DIR: &str = "/opt/sola/log";
const SHARE_DIR: &str = "/opt/sola/share";
/// Staged copy of the workspace `nix/` tree. configuration.nix imports
/// the vendored WPEWebKit derivation from here so the absolute path is
/// stable across reboots (versus `/home/joshua/Workspace/Sola`).
const NIX_DIR: &str = "/opt/sola/nix";

/// Preferred binary replace order when installing multiple targets.
///
/// Matches the process manager's dependency chain: bus first (IPC), then
/// call host, river (compositor bridge), shell, session, kvm. `sola` itself
/// is last because replacing it restarts the whole session.
///
/// Unknown binaries (apps, terminal, …) sort after these and keep the
/// relative order the user passed on the CLI.
const INSTALL_RESTART_ORDER: &[&str] = &[
    "sola-bus",
    "sola-call",
    "sola-river",
    "sola-shell",
    "sola-session",
    "sola-kvm",
    "sola",
];

/// Pause between binary *replaces* so sola's file watcher can restart the
/// previous process and it can re-subscribe / re-frame before the next
/// kill. Only applied between successful writes (unchanged copies skip).
const INSTALL_RESTART_GAP: Duration = Duration::from_millis(1000);

/// Stable rank for sort: known managed order first, then everything else.
fn install_restart_rank(name: &str) -> usize {
    INSTALL_RESTART_ORDER
        .iter()
        .position(|n| *n == name)
        .unwrap_or(INSTALL_RESTART_ORDER.len())
}

/// Sort install targets so dependent restarts land in a safe order.
/// Stable: same-rank names keep caller order.
fn sort_binaries_for_restart(binaries: &mut [String]) {
    binaries.sort_by(|a, b| {
        install_restart_rank(a)
            .cmp(&install_restart_rank(b))
            .then_with(|| a.cmp(b))
    });
}

/// Ensure install directories exist.
pub fn ensure_dirs() -> Result<(), String> {
    // Prefer user mkdir when /opt/sola is already owned by the user (dev).
    // Fall back to sudo for a fresh root-owned tree.
    let dirs = [BIN_DIR, LOG_DIR, SHARE_DIR];
    let mut missing = Vec::new();
    for d in dirs {
        if !Path::new(d).is_dir() {
            missing.push(d);
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    let mut user_ok = true;
    for d in &missing {
        if let Err(e) = fs::create_dir_all(d) {
            user_ok = false;
            eprintln!("  note: mkdir {d} as user: {e}");
            break;
        }
    }
    if user_ok {
        return Ok(());
    }
    run("sudo", &["mkdir", "-p", BIN_DIR, LOG_DIR, SHARE_DIR])
}

/// True when `install_binary` would write (dest missing or bytes differ).
fn binary_needs_install(name: &str, src: &str) -> Result<bool, String> {
    let dest = format!("{BIN_DIR}/{name}");
    if Path::new(&dest).exists() && files_identical(src, &dest)? {
        return Ok(false);
    }
    Ok(true)
}

/// Copy a binary from `src` to the bin directory.
/// Returns true if the destination was written, false if it was
/// already identical and skipped.
///
/// Prefers a direct write when the process can create/replace files under
/// [`BIN_DIR`] (common when the user owns `/opt/sola/bin`). Falls back to
/// `sudo cp` when the existing file is root/`nobody`-owned. Direct write
/// also works under sandboxes where sudo is blocked by NoNewPrivileges.
pub fn install_binary(src: &str) -> Result<bool, String> {
    let name = Path::new(src)
        .file_name()
        .ok_or_else(|| format!("invalid binary path: {src}"))?
        .to_string_lossy();
    let dest = format!("{BIN_DIR}/{name}");

    // Skip if the destination already matches — otherwise cp would
    // retouch the inode and trigger sola's restart watcher.
    if !binary_needs_install(name.as_ref(), src)? {
        return Ok(false);
    }

    match install_binary_user(src, &dest) {
        Ok(()) => return Ok(true),
        Err(user_err) => {
            // Fall back to sudo for root-owned trees.
            match run("sudo", &["cp", "--remove-destination", src, &dest])
                .and_then(|_| run("sudo", &["chmod", "755", &dest]))
            {
                Ok(()) => return Ok(true),
                Err(sudo_err) => {
                    return Err(format!(
                        "direct install failed ({user_err}); sudo install failed ({sudo_err})"
                    ));
                }
            }
        }
    }
}

/// User-owned replace: unlink dest (if any) then copy. Works when the
/// directory is writable even if the existing file is owned by another uid
/// (sticky/bit not set — directory owner can unlink).
fn install_binary_user(src: &str, dest: &str) -> Result<(), String> {
    if Path::new(dest).exists() {
        fs::remove_file(dest).map_err(|e| format!("remove {dest}: {e}"))?;
    }
    fs::copy(src, dest).map_err(|e| format!("copy {src} -> {dest}: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dest, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod {dest}: {e}"))?;
    }
    Ok(())
}

/// Mirror per-crate `dist/` trees into `$XDG_DATA_HOME` (defaulting to
/// `~/.local/share`). Each crate may ship a `dist/` directory whose
/// layout maps 1:1 onto that prefix — e.g.
/// `crates/sola-browser/dist/applications/sola-browser.desktop` is copied to
/// `~/.local/share/applications/sola-browser.desktop`. This is how `.desktop`
/// files, MIME XML, icon themes, etc. ship without touching install
/// logic per file type.
///
/// User-local on purpose: Sola is a single-user system, and putting
/// these in `XDG_DATA_HOME` means xdg-open / GIO find them with no
/// `XDG_DATA_DIRS` ceremony.
///
/// Returns the number of files written (skipping unchanged ones).
/// Copy first-party icon packs from `crates/sola-assets/icons/` into
/// `/opt/sola/share/icons/`. This is called unconditionally from
/// `install()` so any SVG committed under `sola-assets/icons/<pack>/`
/// is always present on disk after install — no upstream fetch required.
///
/// Requires `sudo` because `/opt/sola/share/` is root-owned.
/// Returns the number of files written.
pub fn install_first_party_icons() -> Result<usize, String> {
    let src_root = Path::new("crates/sola-assets/icons");
    if !src_root.is_dir() {
        return Ok(0);
    }
    let dest_root = Path::new(SHARE_DIR).join("icons");
    // Ensure the parent exists (SHARE_DIR already created by ensure_dirs).
    run("sudo", &["mkdir", "-p", dest_root.to_string_lossy().as_ref()])?;
    let entries = match fs::read_dir(src_root) {
        Ok(e) => e,
        Err(e) => return Err(format!("read_dir {}: {e}", src_root.display())),
    };
    let mut written = 0usize;
    for entry in entries.flatten() {
        let pack_src = entry.path();
        if !pack_src.is_dir() {
            continue;
        }
        let pack_name = match pack_src.file_name() {
            Some(n) => n.to_os_string(),
            None => continue,
        };
        let pack_dest = dest_root.join(&pack_name);
        run("sudo", &["mkdir", "-p", pack_dest.to_string_lossy().as_ref()])?;
        // copy_tree writes without sudo; pack_dest is created with sudo above
        // but subsequent file writes need ownership.  Use sudo cp for each file.
        let svgs = match fs::read_dir(&pack_src) {
            Ok(e) => e,
            Err(e) => return Err(format!("read_dir {}: {e}", pack_src.display())),
        };
        for svg_entry in svgs.flatten() {
            let svg_path = svg_entry.path();
            if svg_path.extension().and_then(|e| e.to_str()) != Some("svg") {
                continue;
            }
            let dest_file = pack_dest.join(svg_entry.file_name());
            if dest_file.exists()
                && files_identical(
                    svg_path.to_string_lossy().as_ref(),
                    dest_file.to_string_lossy().as_ref(),
                )?
            {
                continue;
            }
            run(
                "sudo",
                &[
                    "cp",
                    svg_path.to_string_lossy().as_ref(),
                    dest_file.to_string_lossy().as_ref(),
                ],
            )?;
            written += 1;
        }
    }
    Ok(written)
}

/// Mirror the workspace's `nix/` tree into `/opt/sola/nix/` so
/// `/etc/nixos/configuration.nix` can `pkgs.callPackage` the vendored
/// derivations (currently just `wpewebkit/`) from a stable absolute
/// path that doesn't depend on the workspace location. Idempotent —
/// files whose bytes already match are skipped.
pub fn install_nix_modules() -> Result<usize, String> {
    let src = Path::new("nix");
    if !src.is_dir() {
        return Ok(0);
    }
    let dest = Path::new(NIX_DIR);
    fs::create_dir_all(dest).map_err(|e| format!("mkdir {}: {e}", dest.display()))?;
    copy_tree(src, dest)
}

pub fn install_dist_files() -> Result<usize, String> {
    let prefix = xdg_data_home();
    let crates_root = Path::new("crates");
    let entries = match fs::read_dir(crates_root) {
        Ok(e) => e,
        Err(e) => return Err(format!("read_dir crates: {e}")),
    };
    let mut written = 0usize;
    for entry in entries.flatten() {
        let dist = entry.path().join("dist");
        if !dist.is_dir() {
            continue;
        }
        written += copy_tree(&dist, &prefix)?;
    }
    Ok(written)
}

/// `$XDG_DATA_HOME` if set, else `~/.local/share` (per the basedir spec).
fn xdg_data_home() -> PathBuf {
    if let Ok(v) = std::env::var("XDG_DATA_HOME") {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".local").join("share")
}

/// Refresh the desktop-database cache for `~/.local/share/applications`
/// so `xdg-open` / `gio` see newly installed `.desktop` files
/// immediately (without `update-desktop-database` the cache lags by
/// up to a minute, depending on the host). Best-effort: missing
/// `update-desktop-database` is non-fatal — the user just sees the
/// new handler on the next cache refresh.
fn refresh_desktop_database() {
    let dir = xdg_data_home().join("applications");
    if !dir.is_dir() {
        return;
    }
    let status = Command::new("update-desktop-database")
        .arg(&dir)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!(
            "  warning: update-desktop-database exited {} for {}",
            s.code().unwrap_or(-1),
            dir.display()
        ),
        Err(e) => eprintln!(
            "  warning: update-desktop-database not run ({e}); install `desktop-file-utils` so xdg-open finds new handlers immediately"
        ),
    }
}

/// Register every Sola-installed `.desktop` as the default handler for
/// each MIME type it claims. Reads `MimeType=…;…;` from each file in
/// `~/.local/share/applications/` and runs `xdg-mime default <file>
/// <mime>` per entry. Sola is a single-user system; the user installs
/// us, they want us to be the default.
///
/// Best-effort: missing `xdg-mime` warns once and bails (so subsequent
/// invocations on the same install don't spam the user).
fn register_mime_defaults() {
    let dir = xdg_data_home().join("applications");
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut registered = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !filename.starts_with("sola-") {
            continue; // don't clobber unrelated handlers (e.g. firefox)
        }
        let Ok(content) = fs::read_to_string(&path) else { continue };
        for mime in claimed_mime_types(&content) {
            let status = Command::new("xdg-mime")
                .args(["default", filename, mime])
                .status();
            match status {
                Ok(s) if s.success() => registered += 1,
                Ok(s) => eprintln!(
                    "  warning: xdg-mime default {filename} {mime} exited {}",
                    s.code().unwrap_or(-1),
                ),
                Err(e) => {
                    eprintln!(
                        "  warning: xdg-mime not run ({e}); install `xdg-utils` so {filename} becomes the default handler"
                    );
                    return;
                }
            }
        }
    }
    if registered > 0 {
        println!("Registered {registered} MIME default(s)");
    }
}

/// MIME types listed on a `.desktop` `MimeType=` line (semicolon-separated).
fn claimed_mime_types(desktop: &str) -> Vec<&str> {
    desktop
        .lines()
        .find_map(|l| l.strip_prefix("MimeType="))
        .unwrap_or("")
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Recursively mirror `src` onto `dest`. Files identical to the
/// destination are skipped (avoids retouching inodes). Returns the
/// number of files written. No `sudo` — the caller must own `dest`.
fn copy_tree(src: &Path, dest: &Path) -> Result<usize, String> {
    let mut written = 0usize;
    let entries = fs::read_dir(src)
        .map_err(|e| format!("read_dir {}: {e}", src.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name() {
            Some(n) => n,
            None => continue,
        };
        let dest_path: PathBuf = dest.join(name);
        if path.is_dir() {
            fs::create_dir_all(&dest_path)
                .map_err(|e| format!("mkdir {}: {e}", dest_path.display()))?;
            written += copy_tree(&path, &dest_path)?;
        } else if path.is_file() {
            let src_str = path.to_string_lossy().into_owned();
            let dest_str = dest_path.to_string_lossy().into_owned();
            if dest_path.exists() && files_identical(&src_str, &dest_str)? {
                continue;
            }
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
            }
            fs::copy(&path, &dest_path)
                .map_err(|e| format!("cp {} -> {}: {e}", src_str, dest_str))?;
            written += 1;
        }
    }
    Ok(written)
}

/// If the install set includes the `sola` process manager and the
/// freshly built binary differs from the installed one, prompt the user
/// to confirm. Returns `Ok(true)` if the install should proceed,
/// `Ok(false)` if the user declined.
///
/// Skips the prompt (returns `Ok(true)`) when sola isn't being
/// installed, when the source binary is missing (the install loop will
/// warn), or when the bytes already match (the copy would be a no-op).
fn confirm_sola_replace(binaries: &[String]) -> Result<bool, String> {
    if !binaries.iter().any(|b| b == "sola") {
        return Ok(true);
    }
    // Confirm against whichever profile we might replace; prefer release if present.
    let src = if Path::new("target/release/sola").exists() {
        "target/release/sola"
    } else {
        "target/debug/sola"
    };
    let dest = format!("{BIN_DIR}/sola");
    if !Path::new(src).exists() {
        return Ok(true);
    }
    if Path::new(&dest).exists() && files_identical(src, &dest)? {
        return Ok(true);
    }
    print!(
        "About to replace {dest} (the running process manager will restart and tear down every Sola process). Continue? [y/N] "
    );
    io::stdout().flush().ok();
    let mut response = String::new();
    io::stdin()
        .read_line(&mut response)
        .map_err(|e| format!("read stdin: {e}"))?;
    let answer = response.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

/// Compare two files byte-for-byte via `cmp -s`. Returns true on match.
fn files_identical(a: &str, b: &str) -> Result<bool, String> {
    let status = Command::new("cmp")
        .args(["-s", a, b])
        .status()
        .map_err(|e| format!("failed to run cmp: {e}"))?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(format!("cmp failed comparing {a} and {b}")),
    }
}

/// Build and install binaries locally.
///
/// If `apps` is non-empty, builds and installs only those apps (short
/// names like `shell` resolve to `sola-shell`). Otherwise builds and
/// installs all workspace binaries.
///
/// Default is release (`target/release/`). Pass `--debug` for an
/// unoptimized build. Release is much faster at runtime (Bitwarden KDF,
/// screenshot PNG encode).
pub fn install(apps: &[String], release: bool) {
    // Bootstrap third-party assets if any pack is missing.
    // /opt/sola/share is the single source of truth at runtime; install
    // never rsyncs it from the source tree (nothing's committed there).
    // `sync(false)` is idempotent — when every pack is already on the
    // desired pin it's a no-op aside from a few stat() calls.
    if super::assets::needs_sync() {
        println!("Syncing assets...");
        super::assets::sync(false);
    }

    let mut binaries: Vec<String> = if apps.is_empty() {
        super::discover_binaries()
    } else {
        let mut out = Vec::new();
        for n in apps {
            let pkg = super::resolve_crate_name(n);
            let dir = format!("crates/{pkg}");
            let toml = std::path::Path::new(&dir).join("Cargo.toml");
            if let Ok(contents) = std::fs::read_to_string(&toml) {
                out.extend(super::bin_names_from_toml(&contents));
            } else {
                out.push(pkg);
            }
        }
        out.sort();
        out.dedup();
        out
    };
    // Build packages in CLI/discovery order (cargo doesn't care); sort only
    // for the copy loop so on-disk replaces — and thus sola's restart
    // watcher — fire bus → river → shell → … with a settle gap between.
    let build_packages = binaries.clone();
    sort_binaries_for_restart(&mut binaries);

    let profile = if release { "release" } else { "debug" };
    println!("Building ({profile})...");
    // Empty packages ⇒ full workspace build; otherwise one `-p` per app
    // so `cargo make install shell kit` is a single cargo invocation.
    super::build(
        if apps.is_empty() {
            &[]
        } else {
            &build_packages
        },
        release,
    );

    println!("Preparing install...");
    if let Err(e) = ensure_dirs() {
        eprintln!("failed to create directories: {e}");
        std::process::exit(1);
    }

    // Replacing `sola` itself restarts the process manager, which tears
    // down every Sola process. Confirm before doing that — and if the
    // user declines, abort the whole install so nothing else gets
    // touched either.
    match confirm_sola_replace(&binaries) {
        Ok(true) => {}
        Ok(false) => {
            println!("Install cancelled.");
            return;
        }
        Err(e) => {
            eprintln!("failed to check sola binary: {e}");
            std::process::exit(1);
        }
    }

    if binaries.len() > 1 {
        println!(
            "Installing binaries (restart order: {}; {}ms gap between replaces)...",
            binaries.join(" → "),
            INSTALL_RESTART_GAP.as_millis()
        );
    } else {
        println!("Installing binaries...");
    }
    let mut wrote_previous = false;
    for name in &binaries {
        let src = format!("target/{profile}/{name}");
        if !Path::new(&src).exists() {
            eprintln!("  warning: binary not found: {src}");
            continue;
        }
        // Gap only before a real replace (unchanged copies do not restart).
        let needs_write = match binary_needs_install(name, &src) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("  failed to check {name}: {e}");
                std::process::exit(1);
            }
        };
        if needs_write && wrote_previous {
            println!(
                "  … {}ms settle for prior restart",
                INSTALL_RESTART_GAP.as_millis()
            );
            thread::sleep(INSTALL_RESTART_GAP);
        }
        match install_binary(&src) {
            Ok(true) => {
                println!("  installed {name}");
                wrote_previous = true;
            }
            Ok(false) => println!("  unchanged {name}"),
            Err(e) => {
                eprintln!("  failed to install {name}: {e}");
                std::process::exit(1);
            }
        }
    }

    // Isolated crates live outside the workspace and have their own
    // target dirs. Whole-workspace install picks them up here so the
    // user gets a single `cargo make install` UX even when the
    // workspace is bifurcated for feature-isolation reasons. Targeted
    // app installs skip this loop entirely.
    if apps.is_empty() {
        for c in super::isolated::discover() {
            if !super::isolated::has_binary(&c) {
                continue;
            }
            let src = super::isolated::binary_path(&c, false);
            if src.exists() {
                match install_binary(src.to_string_lossy().as_ref()) {
                    Ok(true) => println!("  installed {} (isolated)", c.name),
                    Ok(false) => println!("  unchanged {} (isolated)", c.name),
                    Err(e) => {
                        eprintln!("  failed to install {}: {e}", c.name);
                        std::process::exit(1);
                    }
                }
            } else {
                eprintln!("  warning: isolated binary not found: {}", src.display());
            }
        }
    }

    // Stage the workspace `nix/` tree to `/opt/sola/nix/` so
    // configuration.nix can import the vendored WPEWebKit derivation
    // via a stable absolute path. Cheap when up to date (a few stat()
    // calls); only writes when a file differs.
    match install_nix_modules() {
        Ok(0) => {}
        Ok(n) => println!("Staged {n} nix file(s) to {NIX_DIR}"),
        Err(e) => {
            eprintln!("  warning: nix modules not staged: {e}");
        }
    }

    // Mirror crates/*/dist/ trees onto $XDG_DATA_HOME for .desktop
    // files and other static install artifacts. Always runs (cheap)
    // since single-app installs may share dist files with the desktop.
    match install_dist_files() {
        Ok(0) => {}
        Ok(n) => println!("Installed {n} dist file(s)"),
        Err(e) => {
            eprintln!("failed to install dist files: {e}");
            std::process::exit(1);
        }
    }
    // First-party icons (crates/sola-assets/icons/<pack>/*.svg → /opt/sola/share/icons/<pack>/).
    match install_first_party_icons() {
        Ok(0) => {}
        Ok(n) => println!("Installed {n} first-party icon(s)"),
        Err(e) => {
            // Non-fatal: binary install already succeeded; share/ may be
            // root-owned under sandboxes without sudo (NoNewPrivileges).
            eprintln!("  warning: first-party icons not updated: {e}");
        }
    }
    // Always refresh and re-register — handles the case where the user
    // installed xdg-utils after a previous install ran without it.
    refresh_desktop_database();
    register_mime_defaults();

    println!("Installed to {BIN_DIR}");
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| format!("failed to run {program}: {e}"))?;
    if !status.success() {
        return Err(format!(
            "{program} failed with exit code {}",
            status.code().unwrap_or(1)
        ));
    }
    Ok(())
}

#[cfg(test)]
mod restart_order_tests {
    use super::{install_restart_rank, sort_binaries_for_restart};

    #[test]
    fn managed_order_bus_before_river_before_shell() {
        assert!(install_restart_rank("sola-bus") < install_restart_rank("sola-call"));
        assert!(install_restart_rank("sola-call") < install_restart_rank("sola-river"));
        assert!(install_restart_rank("sola-river") < install_restart_rank("sola-shell"));
        assert!(install_restart_rank("sola-shell") < install_restart_rank("sola"));
    }

    #[test]
    fn sort_puts_river_before_shell_regardless_of_cli_order() {
        let mut names = vec![
            "sola-shell".into(),
            "sola-terminal".into(),
            "sola-river".into(),
        ];
        sort_binaries_for_restart(&mut names);
        assert_eq!(
            names,
            vec![
                "sola-river".to_string(),
                "sola-shell".to_string(),
                "sola-terminal".to_string(),
            ]
        );
    }
}

#[cfg(test)]
mod desktop_mime_tests {
    use super::claimed_mime_types;

    /// In-tree desktop is the source of truth for `register_mime_defaults`.
    const BROWSER_DESKTOP: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sola-browser/dist/applications/sola-browser.desktop"
    ));

    #[test]
    fn splits_mimetype_line() {
        let desktop = "Name=X\nMimeType=text/html;x-scheme-handler/https;\n";
        assert_eq!(
            claimed_mime_types(desktop),
            vec!["text/html", "x-scheme-handler/https"]
        );
    }

    #[test]
    fn sola_browser_desktop_owns_url_and_html_types() {
        let exec = BROWSER_DESKTOP
            .lines()
            .find_map(|l| l.strip_prefix("Exec="))
            .unwrap_or("");
        assert!(
            exec.contains("/opt/sola/bin/sola-browser"),
            "desktop must exec sola-browser (chrome.sock handoff), not another opener: {exec}"
        );
        assert!(
            !exec.to_ascii_lowercase().contains("helium"),
            "desktop must not exec Helium: {exec}"
        );
        let mime = claimed_mime_types(BROWSER_DESKTOP);
        for need in [
            "x-scheme-handler/http",
            "x-scheme-handler/https",
            "text/html",
            "application/xhtml+xml",
            "x-scheme-handler/about",
            "x-scheme-handler/unknown",
        ] {
            assert!(
                mime.contains(&need),
                "sola-browser.desktop must claim {need}, got {mime:?}"
            );
        }
    }
}
