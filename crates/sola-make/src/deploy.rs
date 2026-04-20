use std::path::Path;
use std::process::Command;

const BIN_DIR: &str = "/opt/sola/bin";
const LOG_DIR: &str = "/opt/sola/log";
const SHARE_DIR: &str = "/opt/sola/share";
const ASSETS_SRC: &str = "crates/sola-assets/assets/";

pub trait DeployTarget {
    /// Ensure target directories exist.
    fn ensure_dirs(&self) -> Result<(), String>;

    /// Copy a binary from `src` to the target's bin directory.
    /// Returns true if the destination was written, false if it was
    /// already identical and skipped.
    fn deploy_binary(&self, src: &str) -> Result<bool, String>;

    /// Sync the shared assets tree to the target's share directory.
    fn deploy_assets(&self) -> Result<(), String>;

    /// Human-readable label for log messages.
    fn label(&self) -> &str;
}

/// Deploy to the local machine via sudo cp.
pub struct Local;

impl DeployTarget for Local {
    fn ensure_dirs(&self) -> Result<(), String> {
        run("sudo", &["mkdir", "-p", BIN_DIR, LOG_DIR, SHARE_DIR])
    }

    fn deploy_assets(&self) -> Result<(), String> {
        run(
            "sudo",
            &["rsync", "-a", "--checksum", "--delete", ASSETS_SRC, &format!("{SHARE_DIR}/")],
        )
    }

    fn deploy_binary(&self, src: &str) -> Result<bool, String> {
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

    fn label(&self) -> &str {
        "local"
    }
}

/// Deploy to a remote machine via SSH + rsync.
pub struct Remote {
    pub host: &'static str,
}

impl DeployTarget for Remote {
    fn ensure_dirs(&self) -> Result<(), String> {
        let cmd = format!("mkdir -p {BIN_DIR} {LOG_DIR} {SHARE_DIR}");
        run("ssh", &[self.host, &cmd])
    }

    fn deploy_assets(&self) -> Result<(), String> {
        let dest = format!("{}:{SHARE_DIR}/", self.host);
        run(
            "rsync",
            &["-az", "--checksum", "--delete", ASSETS_SRC, &dest],
        )
    }

    fn deploy_binary(&self, src: &str) -> Result<bool, String> {
        let dest = format!("{}:{BIN_DIR}/", self.host);
        // --checksum: compare contents, not mtime, so freshly-rebuilt
        //   binaries with identical contents aren't re-sent.
        // --itemize-changes: one-line summary per file; empty when skipped.
        let output = Command::new("rsync")
            .args(["-az", "--checksum", "--itemize-changes", src, &dest])
            .output()
            .map_err(|e| format!("failed to run rsync: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "rsync failed with exit code {}: {}",
                output.status.code().unwrap_or(1),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        // rsync prints an itemize line per changed file; empty stdout = skipped.
        Ok(!output.stdout.trim_ascii().is_empty())
    }

    fn label(&self) -> &str {
        self.host
    }
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

/// Build release and deploy binaries to the given target.
///
/// If `app` is provided, builds and deploys only that app.
/// Otherwise builds and deploys all workspace binaries.
pub fn deploy(target: &dyn DeployTarget, app: Option<&str>) {
    println!("Building release...");
    super::build(app.map(|s| s.to_string()), true);

    println!("Preparing {}...", target.label());
    if let Err(e) = target.ensure_dirs() {
        eprintln!("failed to create directories on {}: {e}", target.label());
        std::process::exit(1);
    }

    println!("Deploying shared assets to {}...", target.label());
    if let Err(e) = target.deploy_assets() {
        eprintln!("failed to deploy assets: {e}");
        std::process::exit(1);
    }

    let binaries: Vec<String> = if let Some(name) = app {
        vec![super::resolve_crate_name(name)]
    } else {
        super::discover_binaries()
    };

    println!("Deploying binaries to {}...", target.label());
    for name in &binaries {
        let src = format!("target/release/{name}");
        if Path::new(&src).exists() {
            match target.deploy_binary(&src) {
                Ok(true) => println!("  deployed {name}"),
                Ok(false) => println!("  unchanged {name}"),
                Err(e) => {
                    eprintln!("  failed to deploy {name}: {e}");
                    std::process::exit(1);
                }
            }
        } else {
            eprintln!("  warning: binary not found: {src}");
        }
    }

    println!("Deployed to {} ({BIN_DIR})", target.label());
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
