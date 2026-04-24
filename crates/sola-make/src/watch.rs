use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

use crate::install;

const DEBOUNCE_MS: u64 = 500;

/// Watch a crate's source directory and rebuild+install on changes.
///
/// Watches `crates/<name>/` for file changes.
/// On change: debounce, build, install.
/// Errors don't kill the watcher — it continues watching.
pub fn watch_and_install(name: &str) {
    let crate_dir = format!("crates/{name}");

    if !Path::new(&crate_dir).exists() {
        eprintln!("error: crate directory not found: {crate_dir}");
        std::process::exit(1);
    }

    let crate_name = super::resolve_crate_name(name);

    println!("[watch] watching {crate_dir}/");

    // Initial build + install
    println!("[watch] initial build + install...");
    if build_and_install(&crate_name) {
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
        .watch(Path::new(&crate_dir), RecursiveMode::Recursive)
        .expect("failed to watch crate directory");

    println!("[watch] waiting for changes...");

    loop {
        // Wait for a meaningful event (create, modify, or remove)
        let changed_file = loop {
            let event = match event_rx.recv() {
                Ok(ev) => ev,
                Err(_) => return,
            };
            if let Some(path) = meaningful_change(&event) {
                break path;
            }
        };

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

/// Build a single crate and install locally.
/// Returns true on success, false on failure (with error printed).
fn build_and_install(crate_name: &str) -> bool {
    // Build
    let status = Command::new("cargo")
        .args(["build", "-p", crate_name])
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("[build] FAILED (exit {})", s.code().unwrap_or(1));
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

    if let Err(e) = install::ensure_dirs() {
        eprintln!("[install] FAILED: {e}");
        return false;
    }

    if let Err(e) = install::install_binary(&src) {
        eprintln!("[install] FAILED: {e}");
        return false;
    }

    true
}

/// If the event is a meaningful file change (create, modify, remove),
/// return a human-readable path. Returns None for access, metadata, and
/// other events that shouldn't trigger a rebuild.
fn meaningful_change(event: &notify::Event) -> Option<String> {
    use notify::EventKind;
    match &event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {}
        _ => return None,
    }
    Some(
        event
            .paths
            .first()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(unknown)".to_string()),
    )
}
