/// Sola build system — orchestrates building and deploying the project.
///
/// Invoked via `cargo make <command>` (alias configured in `.cargo/config.toml`).
/// This uses the "xtask" pattern: a Rust binary in the workspace that replaces
/// Makefiles and shell scripts with type-safe, maintainable build logic.
///
/// See: https://github.com/matklad/cargo-xtask
use std::process::{Command, exit};

use clap::Parser;

#[derive(Parser)]
#[command(name = "sola-make", about = "Sola build system")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Build the project (or a specific crate).
    Build {
        /// Specific crate to build (e.g. "sola", "sola-compositor").
        /// Omit to build the entire workspace.
        target: Option<String>,

        /// Build in release mode (optimized, slower compile).
        #[arg(long)]
        release: bool,
    },

    /// Deploy to a target machine.
    Deploy {
        /// Target machine name (e.g. "canto").
        target: String,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Build { target, release } => build(target, release),
        Commands::Deploy { target } => deploy(&target),
    }
}

/// Run `cargo build` with optional crate targeting and release mode.
fn build(target: Option<String>, release: bool) {
    let mut cmd = Command::new("cargo");
    cmd.arg("build");

    if let Some(ref target) = target {
        cmd.args(["-p", target]);
    }

    if release {
        cmd.arg("--release");
    }

    let status = cmd.status().expect("failed to run cargo build");
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}

/// Route deploy to the appropriate target handler.
fn deploy(target: &str) {
    match target {
        "canto" => deploy_canto(),
        other => {
            eprintln!("unknown deploy target: {other}");
            exit(1);
        }
    }
}

/// Deploy the sola binary to canto via rsync over SSH.
///
/// Steps:
/// 1. Build the workspace in release mode
/// 2. Ensure /opt/sola/bin/ exists on canto
/// 3. rsync the sola binary
fn deploy_canto() {
    println!("Building release...");
    build(None, true);

    println!("Preparing canto...");
    run_or_exit("ssh", &["canto", "mkdir -p /opt/sola/bin"]);

    println!("Deploying sola to canto...");
    run_or_exit(
        "rsync",
        &["-az", "--progress", "target/release/sola", "canto:/opt/sola/bin/"],
    );

    println!("Deployed to canto:/opt/sola/bin/sola");
}

/// Run an external command, exiting on failure.
fn run_or_exit(program: &str, args: &[&str]) {
    let status = Command::new(program)
        .args(args)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("failed to run {program}: {e}");
            exit(1);
        });
    if !status.success() {
        eprintln!("{program} failed with exit code {}", status.code().unwrap_or(1));
        exit(status.code().unwrap_or(1));
    }
}
