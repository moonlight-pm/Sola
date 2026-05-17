use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN_DIR: &str = "/opt/sola/bin";
const LOG_DIR: &str = "/opt/sola/log";
const SHARE_DIR: &str = "/opt/sola/share";

/// Ensure install directories exist.
pub fn ensure_dirs() -> Result<(), String> {
    run("sudo", &["mkdir", "-p", BIN_DIR, LOG_DIR, SHARE_DIR])
}

/// Copy a binary from `src` to the bin directory.
/// Returns true if the destination was written, false if it was
/// already identical and skipped.
pub fn install_binary(src: &str) -> Result<bool, String> {
    let name = Path::new(src)
        .file_name()
        .ok_or_else(|| format!("invalid binary path: {src}"))?
        .to_string_lossy();
    let dest = format!("{BIN_DIR}/{name}");

    // Skip if the destination already matches — otherwise cp would
    // retouch the inode and trigger sola's restart watcher.
    if Path::new(&dest).exists() && files_identical(src, &dest)? {
        return Ok(false);
    }

    run("sudo", &["cp", "--remove-destination", src, &dest])?;
    run("sudo", &["chmod", "755", &dest])?;
    Ok(true)
}

/// Mirror per-crate `dist/` trees into `$XDG_DATA_HOME` (defaulting to
/// `~/.local/share`). Each crate may ship a `dist/` directory whose
/// layout maps 1:1 onto that prefix — e.g.
/// `crates/sola-browser/dist/applications/foo.desktop` is copied to
/// `~/.local/share/applications/foo.desktop`. This is how `.desktop`
/// files, MIME XML, icon themes, etc. ship without touching install
/// logic per file type.
///
/// User-local on purpose: Sola is a single-user system, and putting
/// these in `XDG_DATA_HOME` means xdg-open / GIO find them with no
/// `XDG_DATA_DIRS` ceremony.
///
/// Returns the number of files written (skipping unchanged ones).
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
        let mime_line = content
            .lines()
            .find_map(|l| l.strip_prefix("MimeType="))
            .unwrap_or("");
        for mime in mime_line.split(';').filter(|s| !s.is_empty()) {
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
    let src = "target/debug/sola";
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
/// If `app` is provided, builds and installs only that app.
/// Otherwise builds and installs all workspace binaries.
pub fn install(app: Option<&str>) {
    // Bootstrap third-party assets if any pack is missing or stale.
    // /opt/sola/share is the single source of truth at runtime; install
    // never rsyncs it from the source tree (nothing's committed there).
    if let Some(reason) = super::assets::pull_reason() {
        println!("Refreshing assets ({reason})...");
        super::assets::pull();
    }

    println!("Building...");
    super::build(app.map(|s| s.to_string()), false);

    println!("Preparing install...");
    if let Err(e) = ensure_dirs() {
        eprintln!("failed to create directories: {e}");
        std::process::exit(1);
    }

    let binaries: Vec<String> = if let Some(name) = app {
        vec![super::resolve_crate_name(name)]
    } else {
        super::discover_binaries()
    };

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

    println!("Installing binaries...");
    for name in &binaries {
        let src = format!("target/debug/{name}");
        if Path::new(&src).exists() {
            // CEF-linking binaries need a RUNPATH that resolves libcef.so
            // at the dev cache. Patch the build output before install_binary's
            // cmp check so source and dest stay in sync (the prebuilt-release
            // pipeline does the same thing in publish.rs but rpaths to
            // /opt/sola/cef instead of the dev cache).
            if super::cef::CEF_LINKING_BINS.contains(&name.as_str()) {
                if let Err(e) = patchelf_cef_rpath(&src) {
                    eprintln!("  failed to patch RUNPATH for {name}: {e}");
                    std::process::exit(1);
                }
            }
            match install_binary(&src) {
                Ok(true) => println!("  installed {name}"),
                Ok(false) => println!("  unchanged {name}"),
                Err(e) => {
                    eprintln!("  failed to install {name}: {e}");
                    std::process::exit(1);
                }
            }
        } else {
            eprintln!("  warning: binary not found: {src}");
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
    // Always refresh and re-register — handles the case where the user
    // installed xdg-utils after a previous install ran without it.
    refresh_desktop_database();
    register_mime_defaults();

    println!("Installed to {BIN_DIR}");
}

/// Patch a freshly built CEF-linking binary's RUNPATH to point at the
/// dev CEF cache so it can resolve libcef.so when launched standalone
/// or via sola-session (which does not set LD_LIBRARY_PATH for kit apps).
/// Idempotent: patchelf rewriting the same RUNPATH yields the same bytes.
fn patchelf_cef_rpath(bin: &str) -> Result<(), String> {
    let release = super::cef::release_dir();
    let rpath = format!(
        "{}:/run/current-system/sw/share/nix-ld/lib",
        release.display()
    );
    run("patchelf", &["--set-rpath", &rpath, bin])
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
