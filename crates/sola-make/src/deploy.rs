use std::path::Path;
use std::process::Command;

const BIN_DIR: &str = "/opt/sola/bin";
const LOG_DIR: &str = "/opt/sola/log";

pub trait DeployTarget {
    /// Ensure target directories exist.
    fn ensure_dirs(&self) -> Result<(), String>;

    /// Copy a binary from `src` to the target's bin directory.
    fn deploy_binary(&self, src: &str) -> Result<(), String>;

    /// Human-readable label for log messages.
    fn label(&self) -> &str;
}

/// Deploy to the local machine via sudo cp.
pub struct Local;

impl DeployTarget for Local {
    fn ensure_dirs(&self) -> Result<(), String> {
        run("sudo", &["mkdir", "-p", BIN_DIR, LOG_DIR])
    }

    fn deploy_binary(&self, src: &str) -> Result<(), String> {
        let name = Path::new(src)
            .file_name()
            .ok_or_else(|| format!("invalid binary path: {src}"))?
            .to_string_lossy();
        let dest = format!("{BIN_DIR}/{name}");
        run("sudo", &["cp", src, &dest])?;
        run("sudo", &["chmod", "755", &dest])
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
        let cmd = format!("mkdir -p {BIN_DIR} {LOG_DIR}");
        run("ssh", &[self.host, &cmd])
    }

    fn deploy_binary(&self, src: &str) -> Result<(), String> {
        let dest = format!("{}:{BIN_DIR}/", self.host);
        run("rsync", &["-az", "--progress", src, &dest])
    }

    fn label(&self) -> &str {
        self.host
    }
}

/// Build release and deploy binaries to the given target.
///
/// If `app` is provided, builds and deploys only that app.
/// Otherwise builds and deploys all workspace binaries.
pub fn deploy(target: &dyn DeployTarget, app: Option<&str>) {
    println!("Building release...");
    let build_target = app.map(|name| super::resolve_crate_name(name));
    super::build(build_target, true);

    println!("Preparing {}...", target.label());
    if let Err(e) = target.ensure_dirs() {
        eprintln!("failed to create directories on {}: {e}", target.label());
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
            if let Err(e) = target.deploy_binary(&src) {
                eprintln!("  failed to deploy {name}: {e}");
                std::process::exit(1);
            }
            println!("  deployed {name}");
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
