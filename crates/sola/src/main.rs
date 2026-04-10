mod watcher;

use std::collections::HashMap;
use std::process::{Child, Command};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use tracing::{error, info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const MANAGED: &[&str] = &["sola-bus", "sola-compositor"];

fn main() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "sola=info".into());

    let log_dir = "/opt/sola/log";
    let _ = std::fs::create_dir_all(log_dir);
    let file_appender = tracing_appender::rolling::never(log_dir, "sola.log");

    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file_appender);

    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    info!("sola process manager starting");

    let bin_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .expect("failed to determine binary directory");

    // Watch all binaries (including ourselves) for changes
    let mut all_watched: Vec<&str> = vec!["sola"];
    all_watched.extend_from_slice(MANAGED);
    let (change_tx, change_rx) = mpsc::channel();
    watcher::watch_binaries(&bin_dir, &all_watched, change_tx);

    // Launch managed processes
    let mut managed: HashMap<&str, Child> = HashMap::new();
    for name in MANAGED {
        launch(&bin_dir, name, &mut managed);
    }

    // Supervise
    loop {
        // Check for binary changes
        while let Ok(changed) = change_rx.try_recv() {
            if changed == "sola" {
                info!("sola binary changed, restarting self");
                // Kill all children before execv — they'll be relaunched by the new sola
                for (name, mut child) in managed.drain() {
                    info!(process = name, "stopping for sola restart");
                    let _ = child.kill();
                    let _ = child.wait();
                }
                watcher::exec_self();
            } else if let Some(child) = managed.get_mut(changed.as_str()) {
                info!(process = %changed, "binary changed, restarting");
                let _ = child.kill();
                let _ = child.wait();
                launch(&bin_dir, leak_str(&changed), &mut managed);
            }
        }

        // Check for crashed processes
        for name in MANAGED {
            let needs_restart = managed
                .get_mut(name)
                .and_then(|child| child.try_wait().ok().flatten())
                .is_some();

            if needs_restart {
                warn!(process = name, "exited, restarting");
                launch(&bin_dir, name, &mut managed);
            }
        }

        thread::sleep(Duration::from_millis(500));
    }
}

fn launch<'a>(
    bin_dir: &std::path::Path,
    name: &'a str,
    managed: &mut HashMap<&'a str, Child>,
) {
    let bin = bin_dir.join(name);
    match Command::new(&bin).spawn() {
        Ok(child) => {
            info!(process = name, pid = child.id(), "launched");
            managed.insert(name, child);
        }
        Err(e) => {
            error!(process = name, path = %bin.display(), "failed to launch: {e}");
        }
    }
}

/// Leak a String to get a &'static str for HashMap keys.
/// Only called on binary change events, which are rare.
fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}
