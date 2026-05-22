/// Sola build system — orchestrates building and installing the project.
///
/// Invoked via `cargo make <command>` (alias configured in `.cargo/config.toml`).
/// This uses the "xtask" pattern: a Rust binary in the workspace that replaces
/// Makefiles and shell scripts with type-safe, maintainable build logic.
///
/// See: https://github.com/matklad/cargo-xtask
mod assets;
mod cef;
mod install;
mod isolated;
mod publish;
mod watch;

use std::os::unix::process::CommandExt;
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

    /// Download CEF binaries to ~/.cache/sola/cef-<version>/.
    /// Idempotent — skips if already present.
    InstallCef,

    /// Build, bundle, and publish a release to GitHub Releases.
    /// Auto-bumps the patch of the latest vX.Y.Z tag if no version given.
    Publish {
        /// Explicit X.Y.Z version to release. Omit to auto-bump patch.
        version: Option<String>,
    },
}

#[derive(clap::Subcommand, Debug)]
enum AssetsAction {
    /// Reconcile /opt/sola/share with crates/sola-assets/upstream.toml.
    ///
    /// Pulls missing packs, re-pulls packs whose pin has changed, and
    /// removes pack directories that are no longer declared. Packs
    /// already on the desired pin are skipped without network or
    /// copy work. `cargo make install` calls this automatically on
    /// fresh checkouts.
    Sync {
        /// For `github:` packs that track the default branch (empty
        /// `rev` in upstream.toml), re-resolve HEAD via
        /// `git ls-remote` and pull if it advanced. Without this
        /// flag those packs stay pinned to whatever HEAD resolved
        /// to on the last sync.
        #[arg(long)]
        refresh: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Build { target, release } => build_exec(target, release),
        Commands::Assets { action } => match action {
            AssetsAction::Sync { refresh } => assets::sync(refresh),
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
        Commands::InstallCef => {
            match cef::ensure_cef() {
                Ok(path) => {
                    println!("CEF ready at {}", path.display());
                }
                Err(e) => {
                    eprintln!("CEF install failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Publish { version } => publish::publish(version),
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
///
/// Spawns cargo as a child process. Use this when the caller needs to
/// do further work after the build (e.g. `install`'s post-build copy
/// steps). When `target` is `None` (whole-workspace build), also walks
/// the isolated-crates list (see `isolated::discover`) and builds each
/// with its own manifest.
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
    // Whole-workspace build only — single-target requests stay focused.
    if target.is_none() {
        let _ = isolated::build_all(release);
    }
}

/// `exec`-based variant for the top-level `cargo make build` command.
///
/// Replaces the current process image with cargo so the outer
/// `cargo run -q -p sola-make` becomes the inner `cargo build` —
/// one cargo process, no nested-cargo overhead, and no risk of
/// env-var divergence between the parent and child invocation.
///
/// When `target` is `None` (whole-workspace build) we first build any
/// isolated crates (status-based, separate cargo invocations — they
/// have their own target dirs and feature graphs by design), then
/// exec into the workspace cargo build as the final step.
///
/// When `target` is `Some(name)` and `name` matches an isolated crate,
/// build that crate alone (no workspace build, no exec). Otherwise
/// fall through to the normal workspace path.
fn build_exec(target: Option<String>, release: bool) -> ! {
    if let Some(name) = target.as_deref() {
        if let Some(c) = isolated::discover().into_iter().find(|c| c.name == name) {
            let ok = isolated::build(&c, release);
            exit(if ok { 0 } else { 1 });
        }
    }
    if target.is_none() {
        let _ = isolated::build_all(release);
    }
    let resolved = target.as_deref().map(resolve_crate_name);
    let args = build_args(resolved.as_deref(), release);
    // `exec` only returns on failure (e.g. cargo not on PATH).
    let err = Command::new("cargo").args(&args).exec();
    eprintln!("failed to exec cargo: {err}");
    exit(1);
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
/// Skips sola-make itself (it's the build tool) and any path declared
/// in the workspace's `[workspace] exclude` list (those are isolated
/// crates with their own target dirs — see `isolated.rs`).
pub(crate) fn discover_binaries() -> Vec<String> {
    let excluded_paths: std::collections::HashSet<std::path::PathBuf> = isolated::discover()
        .into_iter()
        .map(|c| c.path)
        .collect();

    let mut binaries = Vec::new();
    for dir in &["crates"] {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if excluded_paths.contains(&path) {
                continue;
            }
            let has_main_rs = path.join("src/main.rs").exists();
            let cargo_toml_path = path.join("Cargo.toml");
            let has_bin_section = std::fs::read_to_string(&cargo_toml_path)
                .map(|s| s.contains("[[bin]]"))
                .unwrap_or(false);
            if !has_main_rs && !has_bin_section {
                continue;
            }
            let toml_path = cargo_toml_path;
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

    /// A crate with [[bin]] in Cargo.toml but no src/main.rs must still be
    /// discovered. This verifies the fix that allows sola-kit-legacy (whose binary is
    /// at src/app/main.rs) to be found by the all-apps install path.
    #[test]
    fn discover_binaries_finds_bin_section_without_main_rs() {
        let tmp = std::env::temp_dir().join("sola-make-test-discover");
        let crate_dir = tmp.join("sola-fake-bin");
        let src_dir = crate_dir.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        // Write Cargo.toml with [[bin]] but no src/main.rs.
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"sola-fake-bin\"\nversion = \"0.1.0\"\n\n[[bin]]\nname = \"sola-fake-bin\"\npath = \"src/app/main.rs\"\n",
        ).unwrap();
        // Verify src/main.rs does NOT exist.
        assert!(!crate_dir.join("src/main.rs").exists());
        // Run discover logic for just this temp directory.
        let mut binaries = Vec::new();
        for entry in std::fs::read_dir(&tmp).unwrap().flatten() {
            let path = entry.path();
            let has_main_rs = path.join("src/main.rs").exists();
            let cargo_toml_path = path.join("Cargo.toml");
            let has_bin_section = std::fs::read_to_string(&cargo_toml_path)
                .map(|s| s.contains("[[bin]]"))
                .unwrap_or(false);
            if !has_main_rs && !has_bin_section {
                continue;
            }
            let contents = std::fs::read_to_string(&cargo_toml_path).unwrap();
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
        // Clean up.
        std::fs::remove_dir_all(&tmp).ok();
        assert!(
            binaries.contains(&"sola-fake-bin".to_string()),
            "expected sola-fake-bin to be discovered, got: {:?}",
            binaries
        );
    }
}
