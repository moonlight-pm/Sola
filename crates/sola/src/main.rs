mod watcher;

use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use tracing::{error, info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use sola_bus::topics::Topic;

const MANAGED: &[&str] = &[
    "sola-bus",
    "sola-river",
    "sola-shell",
    "sola-terminal",
];

/// Minimum uptime before a restart is considered immediate (triggers backoff).
const MIN_UPTIME: Duration = Duration::from_secs(5);
/// Delay before restarting a process that crashed quickly.
const BACKOFF_DELAY: Duration = Duration::from_secs(2);

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
    let mut managed: HashMap<&str, ManagedProcess> = HashMap::new();
    for name in MANAGED {
        launch(&bin_dir, name, &mut managed);
    }

    // User apps spawned on LaunchApp bus messages. Fire-and-forget: we reap
    // them in the supervision loop so they don't zombie, but we don't
    // restart or track them beyond that.
    let mut user_apps: Vec<UserApp> = Vec::new();

    // Connect to bus (retry until available)
    let mut bus = sola_bus::BusClient::new();
    bus.set_app_id("sola");

    // Supervise — block on bus messages, fall through every 500ms
    // to check process health and binary changes.
    let poll_interval = Duration::from_millis(500);

    loop {
        // Try to connect to bus if not connected
        if !bus.is_connected() {
            let _ = bus.connect();
        }

        // Block until a bus message arrives or the supervision interval expires.
        let mut messages = Vec::new();
        if let Some(msg) = bus.recv_timeout(poll_interval) {
            messages.push(msg);
            while let Some(msg) = bus.try_recv() {
                messages.push(msg);
            }
        }

        for msg in &messages {
            tracing::debug!(topic = %msg.topic, "bus message received");
            let Some(topic) = Topic::parse(msg) else {
                continue;
            };
            match topic {
                Topic::Shutdown => {
                    info!("shutdown requested via bus");
                    shutdown_all(&mut managed);
                    std::process::exit(0);
                }
                Topic::LaunchApp(command) => {
                    launch_user_app(&command, &mut user_apps);
                }
                _ => {}
            }
        }

        reap_user_apps(&mut user_apps);

        // Check for binary changes
        while let Ok(changed) = change_rx.try_recv() {
            if changed == "sola" {
                info!("sola binary changed, restarting self");
                shutdown_all(&mut managed);
                watcher::exec_self();
            } else if let Some(proc) = managed.get_mut(changed.as_str()) {
                info!(process = %changed, "binary changed, restarting");
                let _ = proc.child.kill();
                let _ = proc.child.wait();
                launch(&bin_dir, leak_str(&changed), &mut managed);
            }
        }

        // Check for exited or missing processes
        for name in MANAGED {
            if let Some(proc) = managed.get_mut(name) {
                let exited = proc.child.try_wait().ok().flatten().is_some();

                if exited {
                    let uptime = proc.started_at.elapsed();
                    if uptime < MIN_UPTIME {
                        warn!(
                            process = name,
                            ?uptime,
                            "crashed quickly, waiting before restart"
                        );
                        thread::sleep(BACKOFF_DELAY);
                    } else {
                        warn!(process = name, ?uptime, "exited, restarting");
                    }
                    launch(&bin_dir, name, &mut managed);
                }
            } else {
                // Initial launch failed — retry
                launch(&bin_dir, name, &mut managed);
            }
        }
    }
}

struct ManagedProcess {
    child: Child,
    started_at: Instant,
}

struct UserApp {
    command: String,
    child: Child,
}

fn launch_user_app(command: &str, user_apps: &mut Vec<UserApp>) {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        warn!("LaunchApp with empty command, ignoring");
        return;
    }
    let mut parts = trimmed.split_whitespace();
    let Some(program) = parts.next() else {
        warn!(command, "LaunchApp with no program, ignoring");
        return;
    };
    let args: Vec<&str> = parts.collect();

    // SAFETY: pre_exec runs in the child after fork. PR_SET_PDEATHSIG asks
    // the kernel to kill the child if sola dies, preventing orphans.
    let result = unsafe {
        Command::new(program)
            .args(&args)
            .pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            })
            .spawn()
    };
    match result {
        Ok(child) => {
            info!(command = trimmed, pid = child.id(), "user app launched");
            user_apps.push(UserApp {
                command: trimmed.to_string(),
                child,
            });
        }
        Err(e) => {
            warn!(command = trimmed, "failed to launch user app: {e}");
        }
    }
}

fn reap_user_apps(user_apps: &mut Vec<UserApp>) {
    user_apps.retain_mut(|app| match app.child.try_wait() {
        Ok(Some(status)) => {
            info!(command = %app.command, pid = app.child.id(), ?status, "user app exited");
            false
        }
        Ok(None) => true,
        Err(e) => {
            warn!(command = %app.command, "wait failed: {e}");
            false
        }
    });
}

fn launch<'a>(
    bin_dir: &std::path::Path,
    name: &'a str,
    managed: &mut HashMap<&'a str, ManagedProcess>,
) {
    let bin = bin_dir.join(name);
    // SAFETY: pre_exec runs in the child after fork, before exec.
    // PR_SET_PDEATHSIG asks the kernel to send SIGTERM to this child
    // when the parent process (sola) dies, preventing orphaned processes.
    let result = unsafe {
        Command::new(&bin)
            .pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            })
            .spawn()
    };
    match result {
        Ok(child) => {
            info!(process = name, pid = child.id(), "launched");
            managed.insert(
                name,
                ManagedProcess {
                    child,
                    started_at: Instant::now(),
                },
            );
        }
        Err(e) => {
            error!(process = name, path = %bin.display(), "failed to launch: {e}");
        }
    }
}

fn shutdown_all(managed: &mut HashMap<&str, ManagedProcess>) {
    for (name, mut proc) in managed.drain() {
        info!(process = name, "stopping");
        let _ = proc.child.kill();
        let _ = proc.child.wait();
    }
}

/// Leak a String to get a &'static str for HashMap keys.
/// Only called on binary change events, which are rare.
fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}
