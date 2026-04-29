/// Sola build system — orchestrates building and installing the project.
///
/// Invoked via `cargo make <command>` (alias configured in `.cargo/config.toml`).
/// This uses the "xtask" pattern: a Rust binary in the workspace that replaces
/// Makefiles and shell scripts with type-safe, maintainable build logic.
///
/// See: https://github.com/matklad/cargo-xtask
mod assets;
mod install;
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

    /// Manage third-party assets (icons, fonts, cursors).
    Assets {
        #[command(subcommand)]
        action: AssetsAction,
    },

    /// Install binaries locally to /opt/sola/bin.
    Install {
        /// Specific app to install (e.g. "terminal").
        /// Omit to install all binaries.
        app: Option<String>,

        /// Watch for changes and reinstall automatically.
        #[arg(long)]
        watch: bool,
    },
}

#[derive(clap::Subcommand, Debug)]
enum AssetsAction {
    /// Pull asset packs from their pinned upstream sources to /opt/sola/share.
    /// `cargo make install` calls this automatically when packs are missing
    /// or older than ~1 week, so manual invocation is rarely needed.
    Pull,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Build { target, release } => build(target, release),
        Commands::Assets { action } => match action {
            AssetsAction::Pull => assets::pull(),
        },
        Commands::Install { app, watch } => {
            if watch && app.is_none() {
                eprintln!("error: --watch requires an app name");
                exit(1);
            }
            if watch {
                watch::watch_and_install(&app.unwrap());
            } else {
                install::install(app.as_deref());
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
fn build(target: Option<String>, release: bool) {
    let resolved = target.as_deref().map(resolve_crate_name);
    let args = build_args(resolved.as_deref(), release);
    let status = Command::new("cargo")
        .args(&args)
        .status()
        .expect("failed to run cargo build");
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
}

/// Resolve a short crate name (e.g. "shell") to its package name
/// (e.g. "sola-shell") by reading `crates/<name>/Cargo.toml`.
/// Falls back to "sola-<name>" if not found.
pub(crate) fn resolve_crate_name(name: &str) -> String {
    let toml_path = format!("crates/{name}/Cargo.toml");
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
    format!("sola-{name}")
}

/// Discover installable binary names by scanning `crates/`.
///
/// Looks for `Cargo.toml` files in `crates/` that contain a
/// `src/main.rs` (i.e. are binary crates), and extracts the package name.
/// Skips sola-make itself since it's the build tool.
fn discover_binaries() -> Vec<String> {
    let mut binaries = Vec::new();
    for dir in &["crates"] {
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
    fn cli_parses_install() {
        let cli = Cli::try_parse_from(["sola-make", "install"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Install {
                app: None,
                watch: false
            }
        ));
    }

    #[test]
    fn cli_parses_install_app() {
        let cli = Cli::try_parse_from(["sola-make", "install", "terminal"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Install { app: Some(ref a), watch: false } if a == "terminal"
        ));
    }

    #[test]
    fn cli_parses_install_watch() {
        let cli = Cli::try_parse_from(["sola-make", "install", "terminal", "--watch"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Install { app: Some(ref a), watch: true } if a == "terminal"
        ));
    }
}
