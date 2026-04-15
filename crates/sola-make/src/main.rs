/// Sola build system — orchestrates building and deploying the project.
///
/// Invoked via `cargo make <command>` (alias configured in `.cargo/config.toml`).
/// This uses the "xtask" pattern: a Rust binary in the workspace that replaces
/// Makefiles and shell scripts with type-safe, maintainable build logic.
///
/// See: https://github.com/matklad/cargo-xtask
mod deploy;
mod watch;

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
    ///
    /// Deploys locally by default. Use --canto for remote deploy.
    Deploy {
        /// Specific app to deploy (e.g. "terminal").
        /// Omit to deploy all binaries.
        app: Option<String>,

        /// Deploy to canto (remote). Omit for local deploy.
        #[arg(long)]
        canto: bool,

        /// Watch for changes and redeploy automatically.
        #[arg(long)]
        watch: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Build { target, release } => build(target, release),
        Commands::Deploy { app, canto, watch } => {
            let target: Box<dyn deploy::DeployTarget> = if canto {
                Box::new(deploy::Remote { host: "canto" })
            } else {
                Box::new(deploy::Local)
            };
            if watch && app.is_none() {
                eprintln!("error: --watch requires an app name");
                exit(1);
            }
            if watch {
                watch::watch_and_deploy(&app.unwrap(), target.as_ref());
            } else {
                deploy::deploy(target.as_ref(), app.as_deref());
            }
        }
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
/// Apps using vendored dependencies + on-demand TS stripping (like terminal)
/// don't need a build step — their web/ sources are embedded directly.
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
        run_or_exit(
            "bun",
            &["run", "--cwd", &web_dir.to_string_lossy(), "build"],
        );
    }
}

/// Resolve a short app name (e.g. "terminal") to the crate's package name
/// (e.g. "sola-terminal"). Checks both `apps/<name>/Cargo.toml` and
/// `crates/<name>/Cargo.toml`. Falls back to "sola-<name>" if not found.
pub(crate) fn resolve_crate_name(name: &str) -> String {
    for prefix in &["apps", "crates"] {
        let toml_path = format!("{prefix}/{name}/Cargo.toml");
        if let Ok(contents) = std::fs::read_to_string(&toml_path) {
            for line in contents.lines() {
                let line = line.trim();
                if line.starts_with("name") {
                    if let Some(pkg_name) = line.split('"').nth(1) {
                        return pkg_name.to_string();
                    }
                }
            }
        }
    }
    format!("sola-{name}")
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
        eprintln!(
            "{program} failed with exit code {}",
            status.code().unwrap_or(1)
        );
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
            Commands::Build {
                target: None,
                release: false
            }
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
            Commands::Build {
                target: None,
                release: true
            }
        ));
    }

    #[test]
    fn cli_parses_deploy_canto() {
        let cli = Cli::try_parse_from(["sola-make", "deploy", "--canto"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Deploy {
                app: None,
                canto: true,
                watch: false
            }
        ));
    }

    #[test]
    fn cli_parses_deploy_app_canto() {
        let cli = Cli::try_parse_from(["sola-make", "deploy", "terminal", "--canto"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Deploy { app: Some(ref a), canto: true, watch: false } if a == "terminal"
        ));
    }

    #[test]
    fn cli_parses_deploy_local() {
        let cli = Cli::try_parse_from(["sola-make", "deploy"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Deploy {
                app: None,
                canto: false,
                watch: false
            }
        ));
    }

    #[test]
    fn cli_parses_deploy_app_local() {
        let cli = Cli::try_parse_from(["sola-make", "deploy", "terminal"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Deploy { app: Some(ref a), canto: false, watch: false } if a == "terminal"
        ));
    }

    #[test]
    fn cli_parses_deploy_watch() {
        let cli =
            Cli::try_parse_from(["sola-make", "deploy", "terminal", "--canto", "--watch"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Deploy { app: Some(ref a), canto: true, watch: true } if a == "terminal"
        ));
    }

    #[test]
    fn cli_parses_deploy_watch_local() {
        let cli = Cli::try_parse_from(["sola-make", "deploy", "terminal", "--watch"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Deploy { app: Some(ref a), canto: false, watch: true } if a == "terminal"
        ));
    }
}
