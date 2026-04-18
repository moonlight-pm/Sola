mod river;
mod watcher;

use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use tracing::{error, info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use sola_bus::topics::{LaunchResultPayload, Topic, UserAppExitedPayload};

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

    // Spawn River first and wait for its wayland socket. Every other
    // managed process is a wayland client that depends on it; without
    // this synchronous wait they'd flap through the backoff path until
    // the socket came up.
    let mut river_sup = match river::RiverSupervisor::spawn(Path::new("/opt/sola/log/river.log"))
    {
        Ok(s) => s,
        Err(e) => {
            error!(%e, "failed to spawn river");
            std::process::exit(1);
        }
    };
    if let Err(e) = river_sup.wait_for_socket() {
        error!(%e, "river socket never appeared");
        river_sup.shutdown();
        std::process::exit(1);
    }
    // River starts XWayland asynchronously after the wayland socket is
    // live. Poll briefly for the X socket so sola-spawned X apps can
    // pick up DISPLAY. Absent XWayland → silent no-op.
    river_sup.wait_for_xwayland(Duration::from_secs(3));

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
    // Keep reconnect attempts from hot-looping when the bus is down: the
    // client's recv_timeout returns immediately while rx is None, so
    // without this we'd call UnixStream::connect several thousand times
    // per second.
    let reconnect_interval = Duration::from_secs(1);
    let mut last_reconnect_attempt = Instant::now() - reconnect_interval;
    let mut reconnect_failures: u32 = 0;

    loop {
        if !bus.is_connected() && last_reconnect_attempt.elapsed() >= reconnect_interval {
            last_reconnect_attempt = Instant::now();
            match bus.connect() {
                Ok(()) => {
                    if reconnect_failures > 0 {
                        info!(failures = reconnect_failures, "bus reconnected");
                        reconnect_failures = 0;
                    }
                }
                Err(e) => {
                    reconnect_failures += 1;
                    if reconnect_failures == 1
                        || reconnect_failures.is_power_of_two()
                    {
                        warn!(%e, attempts = reconnect_failures, "bus reconnect failed");
                    }
                }
            }
        }

        // Block until a bus message arrives or the supervision interval expires.
        // When disconnected, rx is None and recv_timeout returns immediately —
        // substitute an explicit sleep so supervision still ticks at a sane rate.
        let mut messages = Vec::new();
        if bus.is_connected() {
            if let Some(msg) = bus.recv_timeout(poll_interval) {
                messages.push(msg);
                while let Some(msg) = bus.try_recv() {
                    messages.push(msg);
                }
            }
        } else {
            thread::sleep(poll_interval);
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
                    river_sup.shutdown();
                    std::process::exit(0);
                }
                Topic::LaunchApp(payload) => {
                    launch_user_app(&payload.app_id, &payload.command, &mut user_apps, &mut bus);
                }
                _ => {}
            }
        }

        // Every wayland client (sola-river, sola-shell, sola-terminal,
        // user apps) depends on River. If River dies, restarting any
        // individual client won't help — they need a fresh compositor.
        // Tear the whole session down and let the user re-launch sola.
        if let Ok(Some(status)) = river_sup.try_wait() {
            error!(?status, "river exited; shutting down sola");
            shutdown_all(&mut managed);
            std::process::exit(1);
        }

        reap_user_apps(&mut user_apps, &mut bus);

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
    app_id: String,
    command: String,
    child: Child,
}

fn launch_user_app(
    app_id: &str,
    command: &str,
    user_apps: &mut Vec<UserApp>,
    bus: &mut sola_bus::BusClient,
) {
    let trimmed = command.trim();
    info!(app_id, command = trimmed, "LaunchApp received");
    if trimmed.is_empty() {
        warn!("LaunchApp with empty command, ignoring");
        emit_launch_result(bus, app_id, trimmed, false, Some("empty command".into()));
        return;
    }
    let mut parts = trimmed.split_whitespace();
    let Some(program) = parts.next() else {
        warn!(app_id, command, "LaunchApp with no program, ignoring");
        emit_launch_result(bus, app_id, trimmed, false, Some("no program".into()));
        return;
    };
    let args: Vec<&str> = parts.collect();

    let mut cmd = Command::new(program);
    cmd.args(&args);
    // Sola launches from a TTY where neither WAYLAND_DISPLAY nor DISPLAY
    // is set, so user apps would try bare connects and fail. Point them
    // at the sockets sola-river published.
    if let Some(name) = resolve_wayland_socket() {
        tracing::debug!(%name, "setting WAYLAND_DISPLAY for user app");
        cmd.env("WAYLAND_DISPLAY", name);
    } else {
        warn!("sola-wayland not populated yet; user app may fail to connect");
    }
    if let Some(x_display) = resolve_x_display() {
        tracing::debug!(display = %x_display, "setting DISPLAY for user app");
        cmd.env("DISPLAY", x_display);
    }

    // SAFETY: pre_exec runs in the child after fork. PR_SET_PDEATHSIG asks
    // the kernel to kill the child if sola dies, preventing orphans.
    let result = unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        })
        .spawn()
    };
    match result {
        Ok(child) => {
            info!(app_id, command = trimmed, pid = child.id(), "user app launched");
            user_apps.push(UserApp {
                app_id: app_id.to_string(),
                command: trimmed.to_string(),
                child,
            });
            emit_launch_result(bus, app_id, trimmed, true, None);
        }
        Err(e) => {
            warn!(app_id, command = trimmed, "failed to launch user app: {e}");
            emit_launch_result(bus, app_id, trimmed, false, Some(e.to_string()));
        }
    }
}

fn emit_launch_result(
    bus: &mut sola_bus::BusClient,
    app_id: &str,
    command: &str,
    ok: bool,
    error: Option<String>,
) {
    let payload = LaunchResultPayload {
        app_id: app_id.to_string(),
        command: command.to_string(),
        ok,
        error,
    };
    if let Err(e) = bus.emit(Topic::LaunchResult(payload)) {
        warn!(%e, "failed to emit LaunchResult");
    }
}

fn reap_user_apps(user_apps: &mut Vec<UserApp>, bus: &mut sola_bus::BusClient) {
    use std::os::unix::process::ExitStatusExt;
    user_apps.retain_mut(|app| match app.child.try_wait() {
        Ok(Some(status)) => {
            let code = status.code();
            let signal = status.signal();
            info!(
                command = %app.command,
                pid = app.child.id(),
                ?code,
                ?signal,
                "user app exited",
            );
            let payload = UserAppExitedPayload {
                app_id: app.app_id.clone(),
                command: app.command.clone(),
                code,
                signal,
            };
            if let Err(e) = bus.emit(Topic::UserAppExited(payload)) {
                warn!(%e, "failed to emit UserAppExited");
            }
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

/// Read the wayland socket name that sola-river published to
/// `$XDG_RUNTIME_DIR/sola-wayland`. Returns `None` if the file isn't
/// there yet (sola-river still starting, or not running).
fn resolve_wayland_socket() -> Option<String> {
    read_runtime_name("sola-wayland")
}

/// Resolve the X11 display user apps should target. Prefers the value
/// sola-river published to `$XDG_RUNTIME_DIR/sola-display`; if absent
/// (XWayland started lazily or our startup probe missed it), falls back
/// to a live probe of `/tmp/.X11-unix/X*` at the time of the call.
fn resolve_x_display() -> Option<String> {
    if let Some(name) = read_runtime_name("sola-display") {
        return Some(name);
    }
    river::probe_live_x_display()
}

fn read_runtime_name(file: &str) -> Option<String> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    let path = std::path::Path::new(&runtime_dir).join(file);
    let raw = std::fs::read_to_string(&path).ok()?;
    let name = raw.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}
