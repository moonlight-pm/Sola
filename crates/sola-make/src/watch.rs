use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

const DEBOUNCE_MS: u64 = 500;

/// Watch app source directories and rebuild+deploy on changes.
///
/// Watches `apps/<app>/` and `crates/sola-app/` for file changes.
/// On change: debounce, build the app in release mode, deploy to canto.
/// Errors don't kill the watcher — it continues watching.
pub fn watch_and_deploy(app: &str) {
    let app_dir = format!("apps/{app}");
    let framework_dir = "crates/sola-app";

    if !Path::new(&app_dir).exists() {
        eprintln!("error: app directory not found: {app_dir}");
        std::process::exit(1);
    }

    let crate_name = super::resolve_crate_name(app);

    println!("[watch] watching {app_dir}/, {framework_dir}/");

    // Initial build + deploy
    println!("[watch] initial build + deploy...");
    if build_and_deploy(&crate_name) {
        println!("[deploy] {crate_name} → canto ✓");
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

        if build_and_deploy(&crate_name) {
            println!("[deploy] {crate_name} → canto ✓");
        }

        println!("[watch] waiting for changes...");
    }
}

/// Build a single crate in release mode and deploy to canto.
/// Returns true on success, false on failure (with error printed).
fn build_and_deploy(crate_name: &str) -> bool {
    // Build
    let status = Command::new("cargo")
        .args(["build", "-p", crate_name, "--release"])
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

    // Deploy
    let src = format!("target/release/{crate_name}");
    if !Path::new(&src).exists() {
        eprintln!("[deploy] FAILED: binary not found: {src}");
        return false;
    }

    // Ensure target directory exists
    let status = Command::new("ssh")
        .args(["canto", "mkdir -p /opt/sola/bin"])
        .status();

    if let Err(err) = status {
        eprintln!("[deploy] FAILED: ssh: {err}");
        return false;
    }

    let status = Command::new("rsync")
        .args(["-az", "--progress", &src, "canto:/opt/sola/bin/"])
        .status();

    match status {
        Ok(s) if s.success() => true,
        Ok(s) => {
            eprintln!(
                "[deploy] FAILED (exit {})",
                s.code().unwrap_or(1)
            );
            false
        }
        Err(err) => {
            eprintln!("[deploy] FAILED: {err}");
            false
        }
    }
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
