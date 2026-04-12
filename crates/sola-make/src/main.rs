/// Sola build system — orchestrates building and deploying the project.
///
/// Invoked via `cargo make <command>` (alias configured in `.cargo/config.toml`).
/// This uses the "xtask" pattern: a Rust binary in the workspace that replaces
/// Makefiles and shell scripts with type-safe, maintainable build logic.
///
/// See: https://github.com/matklad/cargo-xtask
use std::process::{Command, exit};

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "sola-make", about = "Sola build system")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand, Debug)]
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

/// Construct the `cargo build` arguments for the given options.
///
/// Separated from execution so it can be tested without running cargo.
fn build_args(target: Option<&str>, release: bool) -> Vec<String> {
    let mut args = vec!["build".to_string()];
    if let Some(t) = target {
        args.push("-p".to_string());
        args.push(t.to_string());
    }
    if release {
        args.push("--release".to_string());
    }
    args
}

/// Run `cargo build` with optional crate targeting and release mode.
/// Builds web frontends first if any apps have a `web/` directory.
fn build(target: Option<String>, release: bool) {
    build_web_frontends();

    let args = build_args(target.as_deref(), release);
    let status = Command::new("cargo")
        .args(&args)
        .status()
        .expect("failed to run cargo build");
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}

/// Build web frontends for any app that has a `web/package.json`.
/// Runs `bun install` (if node_modules is missing) and `bun run build`.
fn build_web_frontends() {
    let entries = match std::fs::read_dir("apps") {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let web_dir = entry.path().join("web");
        if !web_dir.join("package.json").exists() {
            continue;
        }
        let app_name = entry.file_name();
        let app_name = app_name.to_string_lossy();

        // Install deps if needed
        if !web_dir.join("node_modules").exists() {
            println!("Installing web deps for {app_name}...");
            run_or_exit("bun", &["install", "--cwd", &web_dir.to_string_lossy()]);
        }

        println!("Building web frontend for {app_name}...");
        run_or_exit("bun", &["run", "--cwd", &web_dir.to_string_lossy(), "build"]);
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
    run_or_exit("ssh", &["canto", "mkdir -p /opt/sola/bin /opt/sola/log"]);

    // Discover and deploy all workspace binaries.
    println!("Deploying binaries to canto...");
    for name in discover_binaries() {
        let src = format!("target/release/{name}");
        if std::path::Path::new(&src).exists() {
            run_or_exit(
                "rsync",
                &["-az", "--progress", &src, "canto:/opt/sola/bin/"],
            );
            println!("  deployed {name}");
        }
    }

    println!("Deployed to canto:/opt/sola/bin/");
}

/// Discover deployable binary names by scanning workspace member directories.
///
/// Looks for `Cargo.toml` files in `crates/` and `apps/` that contain a
/// `src/main.rs` (i.e. are binary crates), and extracts the package name.
/// Skips sola-make itself since it's the build tool.
fn discover_binaries() -> Vec<String> {
    let mut binaries = Vec::new();
    for dir in &["crates", "apps"] {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.join("src/main.rs").exists() {
                continue;
            }
            let toml_path = path.join("Cargo.toml");
            let contents = match std::fs::read_to_string(&toml_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            // Extract package name from `name = "..."` line.
            for line in contents.lines() {
                let line = line.trim();
                if line.starts_with("name") {
                    if let Some(name) = line.split('"').nth(1) {
                        if name != "sola-make" {
                            binaries.push(name.to_string());
                        }
                    }
                    break;
                }
            }
        }
    }
    binaries.sort();
    binaries
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_args_default() {
        assert_eq!(build_args(None, false), vec!["build"]);
    }

    #[test]
    fn build_args_with_target() {
        assert_eq!(
            build_args(Some("sola-compositor"), false),
            vec!["build", "-p", "sola-compositor"]
        );
    }

    #[test]
    fn build_args_release() {
        assert_eq!(build_args(None, true), vec!["build", "--release"]);
    }

    #[test]
    fn build_args_target_and_release() {
        assert_eq!(
            build_args(Some("sola"), true),
            vec!["build", "-p", "sola", "--release"]
        );
    }

    #[test]
    fn cli_parses_build() {
        let cli = Cli::try_parse_from(["sola-make", "build"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Build { target: None, release: false }
        ));
    }

    #[test]
    fn cli_parses_build_with_target() {
        let cli = Cli::try_parse_from(["sola-make", "build", "sola"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Build { target: Some(ref t), release: false } if t == "sola"
        ));
    }

    #[test]
    fn cli_parses_build_release() {
        let cli = Cli::try_parse_from(["sola-make", "build", "--release"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Build { target: None, release: true }
        ));
    }

    #[test]
    fn cli_parses_deploy() {
        let cli = Cli::try_parse_from(["sola-make", "deploy", "canto"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Deploy { target: ref t } if t == "canto"
        ));
    }
}
