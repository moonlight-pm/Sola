mod river;

use sola_core::watcher;

use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use tracing::{error, info, warn};

use sola_bus::topics::Topic;

const MANAGED: &[&str] = &["sola-bus", "sola-river", "sola-shell", "sola-session"];

/// Minimum uptime before a restart is considered immediate (triggers backoff).
const MIN_UPTIME: Duration = Duration::from_secs(5);
/// Delay before restarting a process that crashed quickly.
const BACKOFF_DELAY: Duration = Duration::from_secs(2);

fn main() {
    sola_core::log::rotate();
    sola_core::log::init("sola");

    set_cursor_env();

    info!("sola process manager starting");

    // SIGINT (Ctrl-C on the TTY), SIGTERM, and SIGHUP all map to the same
    // graceful shutdown path the menu's "Quit Sola" already uses. Without
    // this, Ctrl-C kills sola abruptly and orphans every user app —
    // sola-session never gets a chance to stop their scope units.
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    for &sig in &[
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
    ] {
        signal_hook::flag::register(sig, Arc::clone(&shutdown_requested))
            .expect("register signal handler");
    }

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
    let mut river_sup = match river::RiverSupervisor::spawn(Path::new("/opt/sola/log/river.log")) {
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
        if shutdown_requested.load(Ordering::SeqCst) {
            info!("shutdown requested via signal");
            do_shutdown(&mut bus, &mut managed, &mut river_sup);
        }

        if !bus.is_connected() && last_reconnect_attempt.elapsed() >= reconnect_interval {
            last_reconnect_attempt = Instant::now();
            match bus.connect() {
                Ok(()) => {
                    let _ = bus.subscribe(&[sola_bus::topics::TopicKind::Shutdown]);
                    if reconnect_failures > 0 {
                        info!(failures = reconnect_failures, "bus reconnected");
                        reconnect_failures = 0;
                    }
                }
                Err(e) => {
                    reconnect_failures += 1;
                    if reconnect_failures == 1 || reconnect_failures.is_power_of_two() {
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
                    do_shutdown(&mut bus, &mut managed, &mut river_sup);
                }
                _ => {}
            }
        }

        // Every wayland client (sola-river, sola-shell, sola-session)
        // depends on River. If River dies, restarting any
        // individual client won't help — they need a fresh compositor.
        // Tear the whole session down and let the user re-launch sola.
        if let Ok(Some(status)) = river_sup.try_wait() {
            error!(?status, "river exited; shutting down sola");
            shutdown_all(&mut managed);
            std::process::exit(1);
        }

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

fn launch<'a>(
    bin_dir: &std::path::Path,
    name: &'a str,
    managed: &mut HashMap<&'a str, ManagedProcess>,
) {
    let bin = bin_dir.join(name);
    // SAFETY: pre_exec runs post-fork in the child before exec; the hook
    // is async-signal-safe.
    //
    // setsid() puts the child in its own session so a TTY Ctrl-C does
    // not deliver SIGINT to the whole managed set. Without this,
    // sola-bus and sola-session die instantly on Ctrl-C, before sola
    // can broadcast Topic::Shutdown — so sola-session never gets to
    // stop user-app scopes. pdeathsig still SIGTERMs everyone if sola
    // dies unexpectedly, so the supervision contract is unchanged.
    //
    // SOLA_NO_SELF_WATCH tells the kit's startup() to skip its own
    // self-restart watcher — sola is already restarting these on
    // binary change, so without this they'd double-restart (kit
    // exec_self followed instantly by sola kill + launch).
    let result = unsafe {
        Command::new(&bin)
            .env("SOLA_NO_SELF_WATCH", "1")
            .pre_exec(sola_core::process::set_pdeathsig_and_leader)
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

/// Single shutdown path: best-effort broadcast `Topic::Shutdown`, give
/// subscribers a moment to react (sola-session needs it to stop user-app
/// scopes), then tear down our managed children and River. Exits the
/// process — never returns.
fn do_shutdown(
    bus: &mut sola_bus::BusClient,
    managed: &mut HashMap<&str, ManagedProcess>,
    river_sup: &mut river::RiverSupervisor,
) -> ! {
    if bus.is_connected() {
        let _ = bus.emit(Topic::Shutdown);
    }
    // Subscribers tick at 500ms max; 200ms is enough for them to pull
    // the message off the bus and start their own teardown without
    // stalling our shutdown noticeably.
    thread::sleep(Duration::from_millis(200));
    shutdown_all(managed);
    river_sup.shutdown();
    std::process::exit(0);
}

/// Leak a String to get a &'static str for HashMap keys.
/// Only called on binary change events, which are rare.
fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// Point GTK / Wayland clients at our bundled cursor theme.
///
/// `XCURSOR_PATH` prepends `/opt/sola/share/cursors` so a vendored
/// theme there wins over anything the user might have installed
/// system-wide. The default fallback (`~/.icons:/usr/share/icons`)
/// is preserved when `XCURSOR_PATH` was unset.
///
/// `XCURSOR_THEME` defaults to `McMojave` (vendored under
/// `/opt/sola/share/cursors/McMojave/`). Don't override an existing
/// value — the user may have already picked something explicit.
fn set_cursor_env() {
    const BUNDLED: &str = "/opt/sola/share/cursors";
    let path = match std::env::var_os("XCURSOR_PATH") {
        Some(existing) => format!("{BUNDLED}:{}", existing.to_string_lossy()),
        None => format!("{BUNDLED}:~/.icons:/usr/share/icons"),
    };
    unsafe { std::env::set_var("XCURSOR_PATH", path) };
    if std::env::var_os("XCURSOR_THEME").is_none() {
        unsafe { std::env::set_var("XCURSOR_THEME", "McMojave") };
    }
}
