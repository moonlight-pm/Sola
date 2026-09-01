# Dev Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable live frontend development by having sola-app watch its own binary for restarts, and adding a `--watch` flag to `cargo make install` that rebuilds and installs on file changes.

**Architecture:** Two independent pieces. (1) `sola-app` gains a binary watcher module that calls `execv` when the binary is replaced on disk. (2) `sola-make` gains a `--watch` flag on the `Install` subcommand that uses the `notify` crate to watch source directories and re-runs build+install on changes.

**Tech Stack:** Rust, `notify` crate (inotify), `nix` crate (execv), clap derive

---

## File Structure

```
crates/sola-app/
  src/
    lib.rs          # Modify: spawn watcher thread in run()
    watcher.rs      # Create: binary watching + exec_self logic

crates/sola-make/
  Cargo.toml        # Modify: add notify dependency
  src/
    main.rs         # Modify: add --watch flag to Install, optional app filter
    watch.rs        # Create: file watching + rebuild/install loop
```

---

### Task 1: Add binary watcher to sola-app

**Files:**
- Create: `crates/sola-app/src/watcher.rs`
- Modify: `crates/sola-app/Cargo.toml`
- Modify: `crates/sola-app/src/lib.rs`

- [ ] **Step 1: Add dependencies to sola-app**

Add `notify` and `nix` to `crates/sola-app/Cargo.toml`:

```toml
notify = "8"
nix = { version = "0.31", features = ["process"] }
```

- [ ] **Step 2: Create `crates/sola-app/src/watcher.rs`**

This module watches the app's own binary and calls `execv` when it changes. Adapted from `crates/sola/src/watcher.rs` — simplified for single-binary watching.

```rust
use std::ffi::CString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

const DEBOUNCE_MS: u64 = 500;

/// Watch the current process's binary and exec_self when it changes on disk.
///
/// Spawns a background thread. When the binary is replaced (e.g. by rsync),
/// the process re-executes itself. This never returns on success — the current
/// process image is replaced by a fresh one.
pub fn watch_own_binary() {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(err) => {
            tracing::warn!(?err, "cannot resolve binary path, skipping self-watch");
            return;
        }
    };

    let bin_dir = match exe.parent() {
        Some(d) => d.to_path_buf(),
        None => {
            tracing::warn!("binary has no parent directory, skipping self-watch");
            return;
        }
    };

    let bin_name = match exe.file_name() {
        Some(n) => n.to_string_lossy().to_string(),
        None => {
            tracing::warn!("binary has no file name, skipping self-watch");
            return;
        }
    };

    let (event_tx, event_rx) = std::sync::mpsc::channel::<notify::Event>();

    let mut watcher = match RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = event_tx.send(event);
            }
        },
        Config::default(),
    ) {
        Ok(w) => w,
        Err(err) => {
            tracing::warn!(?err, "failed to create file watcher, skipping self-watch");
            return;
        }
    };

    if let Err(err) = watcher.watch(&bin_dir, RecursiveMode::NonRecursive) {
        tracing::warn!(?err, path = %bin_dir.display(), "failed to watch binary directory");
        return;
    }

    tracing::info!(binary = %bin_name, "watching for binary changes");

    std::thread::spawn(move || {
        let _watcher = watcher; // keep alive

        loop {
            let event = match event_rx.recv() {
                Ok(ev) => ev,
                Err(_) => return,
            };

            // Only react to events for our binary
            if !event_matches_binary(&event, &bin_name) {
                continue;
            }

            // Debounce: wait for rsync temp + rename to settle
            let deadline = Instant::now() + Duration::from_millis(DEBOUNCE_MS);
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match event_rx.recv_timeout(remaining) {
                    Ok(_) => {}
                    Err(_) => break,
                }
            }

            tracing::info!(binary = %bin_name, "binary changed on disk, restarting");
            exec_self();
        }
    });
}

/// Replace the current process with a fresh copy of itself.
fn exec_self() -> ! {
    let mut exe = std::env::current_exe().expect("cannot resolve binary for restart");

    // On Linux, /proc/self/exe gets " (deleted)" when the binary is replaced.
    let path_str = exe.to_string_lossy();
    if path_str.ends_with(" (deleted)") {
        exe = PathBuf::from(path_str.trim_end_matches(" (deleted)"));
    }

    let exe_cstr = CString::new(exe.as_os_str().as_encoded_bytes().to_vec())
        .expect("binary path contains null byte");

    let args: Vec<CString> = std::env::args()
        .map(|a| CString::new(a).expect("arg contains null byte"))
        .collect();

    tracing::info!(path = %exe.display(), "execv (self-restart)");

    match nix::unistd::execv(&exe_cstr, &args) {
        Ok(infallible) => match infallible {},
        Err(err) => panic!("execv failed: {err}"),
    }
}

fn event_matches_binary(event: &notify::Event, bin_name: &str) -> bool {
    use notify::EventKind;
    match &event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {}
        _ => return false,
    }
    event.paths.iter().any(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == bin_name)
    })
}
```

- [ ] **Step 3: Wire watcher into `SolaApp::run()`**

In `crates/sola-app/src/lib.rs`, add the module declaration and spawn the watcher. Add `pub mod watcher;` alongside the other module declarations (after line 15). Then in `run()`, call `watcher::watch_own_binary()` after logging is initialized but before the Wayland socket wait (after line 117, before line 119):

```rust
// After: tracing::info!("{} starting", self.app_id);
// Before: // Wayland socket wait

// Watch own binary for updates (auto-restart on deploy)
watcher::watch_own_binary();
```

- [ ] **Step 4: Build and verify**

```bash
cargo make build sola-terminal
```

Expected: builds successfully with the new watcher module.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-app/src/watcher.rs crates/sola-app/src/lib.rs crates/sola-app/Cargo.toml
git commit -m "Add self-restart binary watcher to sola-app"
```

---

### Task 2: Refactor install command to accept app targeting and flags

**Files:**
- Modify: `crates/sola-make/src/main.rs`

- [ ] **Step 1: Update the `Install` variant in `Commands` enum**

Change the `Install` variant to support an optional app name and `--watch`:

```rust
/// Install binaries locally to /opt/sola/bin.
Install {
    /// Specific app to install (e.g. "terminal").
    /// Omit to install all binaries.
    app: Option<String>,

    /// Watch for changes and reinstall automatically.
    #[arg(long)]
    watch: bool,
},
```

- [ ] **Step 2: Update `main()` match arm**

```rust
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
```

- [ ] **Step 3: Update `install` to accept an optional app filter**

Modify the existing `install` function to optionally install only a single app:

```rust
/// Build and install binaries locally to /opt/sola/bin.
fn install(app: Option<&str>) {
    let target = match app {
        Some(name) => Some(name.to_string()),
        None => None,
    };

    println!("Building...");
    build(target.clone(), false);

    println!("Preparing install...");
    run_or_exit("mkdir", &["-p", "/opt/sola/bin", "/opt/sola/log"]);

    let binaries: Vec<String> = if let Some(name) = app {
        // Resolve the crate name: apps use "sola-<name>" as their package name
        let crate_name = resolve_crate_name(name);
        vec![crate_name]
    } else {
        discover_binaries()
    };

    println!("Installing binaries...");
    for name in &binaries {
        let src = format!("target/debug/{name}");
        if std::path::Path::new(&src).exists() {
            run_or_exit(
                "cp",
                &["--remove-destination", &src, "/opt/sola/bin/"],
            );
            println!("  installed {name}");
        } else {
            eprintln!("  warning: binary not found: {src}");
        }
    }

    println!("Installed to /opt/sola/bin/");
}

/// Resolve a short app name (e.g. "terminal") to the crate's package name
/// (e.g. "sola-terminal"). Checks both `apps/<name>/Cargo.toml` and
/// `crates/<name>/Cargo.toml`. Falls back to "sola-<name>" if not found.
fn resolve_crate_name(name: &str) -> String {
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
```

- [ ] **Step 4: Update existing tests and add new ones**

Update existing tests for the new CLI shape and add validation tests:

```rust
#[test]
fn cli_parses_install() {
    let cli = Cli::try_parse_from(["sola-make", "install"]).unwrap();
    assert!(matches!(
        cli.command,
        Commands::Install { app: None, watch: false }
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
```

Remove the old `cli_parses_deploy` test since the shape has changed.

- [ ] **Step 5: Build and run tests**

```bash
cargo test -p sola-make
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/sola-make/src/main.rs
git commit -m "Refactor install command with optional app targeting"
```

---

### Task 3: Add watch module to sola-make

**Files:**
- Create: `crates/sola-make/src/watch.rs`
- Modify: `crates/sola-make/Cargo.toml`
- Modify: `crates/sola-make/src/main.rs`

- [ ] **Step 1: Add `notify` dependency to sola-make**

Add to `crates/sola-make/Cargo.toml`:

```toml
notify = "8"
```

- [ ] **Step 2: Create `crates/sola-make/src/watch.rs`**

```rust
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

const DEBOUNCE_MS: u64 = 500;

/// Watch app source directories and rebuild+install on changes.
///
/// Watches `apps/<app>/` and `crates/sola-app/` for file changes.
/// On change: debounce, build the app, install locally to /opt/sola/bin.
/// Errors don't kill the watcher — it continues watching.
pub fn watch_and_install(app: &str) {
    let app_dir = format!("apps/{app}");
    let framework_dir = "crates/sola-app";

    if !Path::new(&app_dir).exists() {
        eprintln!("error: app directory not found: {app_dir}");
        std::process::exit(1);
    }

    let crate_name = super::resolve_crate_name(app);

    println!("[watch] watching {app_dir}/, {framework_dir}/");

    // Initial build + install
    println!("[watch] initial build + install...");
    let ok = build_and_install(&crate_name);
    if ok {
        println!("[install] {crate_name} ✓");
    }

    let (event_tx, event_rx) = mpsc::channel::<notify::Event>();

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = event_tx.send(event);
            }
        },
        Config::default(),
    )
    .expect("failed to create file watcher");

    watcher
        .watch(Path::new(&app_dir), RecursiveMode::Recursive)
        .expect("failed to watch app directory");

    if Path::new(framework_dir).exists() {
        watcher
            .watch(Path::new(framework_dir), RecursiveMode::Recursive)
            .expect("failed to watch sola-app directory");
    }

    println!("[watch] waiting for changes...");

    loop {
        // Wait for first event
        let event = match event_rx.recv() {
            Ok(ev) => ev,
            Err(_) => return,
        };

        let changed_file = describe_change(&event);

        // Debounce: drain events for DEBOUNCE_MS
        let deadline = Instant::now() + Duration::from_millis(DEBOUNCE_MS);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match event_rx.recv_timeout(remaining) {
                Ok(_) => {}
                Err(_) => break,
            }
        }

        println!("[watch] changed: {changed_file}");
        println!("[watch] building {crate_name}...");

        if build_and_install(&crate_name) {
            println!("[install] {crate_name} ✓");
        }

        println!("[watch] waiting for changes...");
    }
}

/// Build a single crate and install it locally to /opt/sola/bin.
/// Returns true on success, false on failure (with error printed).
fn build_and_install(crate_name: &str) -> bool {
    // Build
    let status = Command::new("cargo")
        .args(["build", "-p", crate_name])
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!(
                "[build] FAILED (exit {})",
                s.code().unwrap_or(1)
            );
            return false;
        }
        Err(err) => {
            eprintln!("[build] FAILED: {err}");
            return false;
        }
    }

    // Install
    let src = format!("target/debug/{crate_name}");
    if !Path::new(&src).exists() {
        eprintln!("[install] FAILED: binary not found: {src}");
        return false;
    }

    let status = Command::new("cp")
        .args(["--remove-destination", &src, "/opt/sola/bin/"])
        .status();

    match status {
        Ok(s) if s.success() => true,
        Ok(s) => {
            eprintln!(
                "[install] FAILED (exit {})",
                s.code().unwrap_or(1)
            );
            false
        }
        Err(err) => {
            eprintln!("[install] FAILED: {err}");
            false
        }
    }
}

/// Extract a human-readable path from a file watcher event.
fn describe_change(event: &notify::Event) -> String {
    event
        .paths
        .first()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(unknown)".to_string())
}
```

- [ ] **Step 3: Add module declaration to `main.rs`**

Add `mod watch;` at the top of `crates/sola-make/src/main.rs` (after the existing imports). Also make `resolve_crate_name` accessible to `watch.rs` by keeping it as `pub(crate)`:

Change `fn resolve_crate_name` to `pub(crate) fn resolve_crate_name`.

- [ ] **Step 4: Build and verify**

```bash
cargo make build sola-make
```

Expected: builds successfully.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-make/src/watch.rs crates/sola-make/src/main.rs crates/sola-make/Cargo.toml
git commit -m "Add --watch flag to install command for live development"
```

---

### Task 4: End-to-end verification

- [ ] **Step 1: Verify the full CLI works**

```bash
cargo make install --help
```

Expected output should show `app` and `--watch` options.

- [ ] **Step 2: Verify single-app install still works**

```bash
cargo make install terminal
```

Expected: builds sola-terminal, installs to `/opt/sola/bin/`.

- [ ] **Step 3: Verify install-all still works**

```bash
cargo make install
```

Expected: builds all binaries, installs to `/opt/sola/bin/`.

- [ ] **Step 4: Test watch mode briefly**

```bash
# Start watch mode (Ctrl+C to stop)
cargo make install terminal --watch
```

Expected: initial build+install succeeds, then prints `[watch] waiting for changes...`. Touch a file in `apps/terminal/` and verify it triggers a rebuild+install.

- [ ] **Step 5: Run all sola-make tests**

```bash
cargo test -p sola-make
```

Expected: all tests pass.

- [ ] **Step 6: Final commit if any fixups needed**

Only if earlier steps required adjustments.
