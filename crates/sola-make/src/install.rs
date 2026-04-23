use std::path::Path;
use std::process::Command;

const BIN_DIR: &str = "/opt/sola/bin";
const LOG_DIR: &str = "/opt/sola/log";
const SHARE_DIR: &str = "/opt/sola/share";
const ASSETS_SRC: &str = "crates/sola-assets/assets/";

/// Ensure install directories exist.
pub fn ensure_dirs() -> Result<(), String> {
    run("sudo", &["mkdir", "-p", BIN_DIR, LOG_DIR, SHARE_DIR])
}

/// Sync the shared assets tree to the share directory.
pub fn install_assets() -> Result<(), String> {
    run(
        "sudo",
        &[
            "rsync",
            "-a",
            "--checksum",
            "--delete",
            ASSETS_SRC,
            &format!("{SHARE_DIR}/"),
        ],
    )
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
    println!("Building...");
    super::build(app.map(|s| s.to_string()), false);

    println!("Preparing install...");
    if let Err(e) = ensure_dirs() {
        eprintln!("failed to create directories: {e}");
        std::process::exit(1);
    }

    println!("Installing shared assets...");
    if let Err(e) = install_assets() {
        eprintln!("failed to install assets: {e}");
        std::process::exit(1);
    }

    let binaries: Vec<String> = if let Some(name) = app {
        vec![super::resolve_crate_name(name)]
    } else {
        super::discover_binaries()
    };

    println!("Installing binaries...");
    for name in &binaries {
        let src = format!("target/debug/{name}");
        if Path::new(&src).exists() {
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
