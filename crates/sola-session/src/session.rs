use std::collections::{HashMap, HashSet};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tracing::{info, warn};

use sola_bus::BusClient;
use sola_bus::topics::{
    LaunchAppPayload, LaunchResultPayload, SessionApp, Topic, TopicKind, UserAppExitedPayload,
};

/// How long `restore_session` drains startup replays (the persisted
/// `SessionApps` set and the sticky `Windows` list) before deciding what to
/// relaunch. Also gives the compositor a moment to come up at cold boot.
const RESTORE_SETTLE: Duration = Duration::from_millis(750);

use sola_core::env;

/// Hard ceiling for the per-shutdown poll loop: 5s `TimeoutStopSec` set
/// on each scope plus 1s slack.
const SHUTDOWN_POLL_BUDGET: Duration = Duration::from_secs(6);

pub struct AppRecord {
    pub app_id: String,
    pub command: String,
    /// Transient scope unit owning this app's cgroup, e.g. `sola-app-steam-3.scope`.
    pub unit: String,
    /// `systemd-run --scope` client process — sticks around for the
    /// lifetime of the scope and proxies exit status. `try_wait()` on
    /// this is how we detect the user app exiting.
    pub child: Child,
    #[allow(dead_code)]
    pub launched_at: Instant,
    /// True once `systemctl stop` has been issued; suppresses re-stops.
    pub closing: bool,
}

pub struct Session {
    bus: BusClient,
    children: HashMap<String, Vec<AppRecord>>,
    /// Monotonic, never reused, so concurrent launches of the same
    /// app_id get distinct scope unit names.
    launch_counter: u64,
}

impl Session {
    pub fn new() -> Self {
        let mut bus = BusClient::new();
        bus.set_app_id("sola-session");
        Self {
            bus,
            children: HashMap::new(),
            launch_counter: 0,
        }
    }

    fn emit_launch_result(&mut self, app_id: &str, command: &str, ok: bool, error: Option<String>) {
        let _ = self.bus.emit(Topic::LaunchResult(LaunchResultPayload {
            app_id: app_id.to_string(),
            command: command.to_string(),
            ok,
            error,
        }));
    }

    fn emit_exited(&mut self, app_id: &str, command: &str, status: std::process::ExitStatus) {
        use std::os::unix::process::ExitStatusExt;
        let payload = UserAppExitedPayload {
            app_id: app_id.to_string(),
            command: command.to_string(),
            code: status.code(),
            signal: status.signal(),
        };
        let _ = self.bus.emit(Topic::UserAppExited(payload));
    }

    /// Publish the current open-app set as the persistent `SessionApps`
    /// topic so it can be restored on the next start. One entry per
    /// `app_id` (a multi-instance app collapses to one), skipping apps
    /// that are already closing. Called on every child-set change — never
    /// standalone at startup, so a stale-empty `children` (after a bare
    /// restart) can't clobber the persisted set.
    fn emit_session_apps(&mut self) {
        let pairs = self.children.iter().filter_map(|(app_id, recs)| {
            recs.iter()
                .find(|r| !r.closing)
                .map(|r| (app_id.clone(), r.command.clone()))
        });
        let apps = session_apps_from_pairs(pairs);
        let _ = self.bus.emit(Topic::SessionApps(apps));
    }

    /// One-shot session restore, run once at startup after subscribing.
    ///
    /// Drains the initial replays for `RESTORE_SETTLE` — the persisted
    /// `SessionApps` (last session's open set) and the sticky `Windows`
    /// (whatever is currently running) — then relaunches every persisted
    /// app that isn't already running. The "minus what's running" filter
    /// makes this self-correcting: a cold boot relaunches everything; a
    /// bare `sola-session` restart relaunches nothing (apps still up).
    pub fn restore_session(&mut self) {
        let deadline = Instant::now() + RESTORE_SETTLE;
        let mut persisted: Vec<SessionApp> = Vec::new();
        let mut running: HashSet<String> = HashSet::new();

        while Instant::now() < deadline {
            while let Some(msg) = self.bus.try_recv() {
                if let Some(topic) = Topic::parse(&msg) {
                    self.absorb_restore(topic, &mut persisted, &mut running);
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            if let Some(msg) = self.bus.recv_timeout(remaining) {
                if let Some(topic) = Topic::parse(&msg) {
                    self.absorb_restore(topic, &mut persisted, &mut running);
                }
            }
        }

        let children_keys: HashSet<String> = self.children.keys().cloned().collect();
        let plan = restore_plan(&persisted, &running, &children_keys);
        if plan.is_empty() {
            info!(
                persisted = persisted.len(),
                running = running.len(),
                "session restore: nothing to relaunch"
            );
            return;
        }
        info!(count = plan.len(), "session restore: relaunching apps");
        for app in plan {
            self.launch(LaunchAppPayload {
                app_id: app.app_id,
                command: app.command,
            });
        }
    }

    /// Fold one message into the restore accumulators. `SessionApps` and
    /// `Windows` update the snapshots; anything else (a launch/close that
    /// arrives during settle) is handled normally so it isn't dropped.
    fn absorb_restore(
        &mut self,
        topic: Topic,
        persisted: &mut Vec<SessionApp>,
        running: &mut HashSet<String>,
    ) {
        match topic {
            Topic::SessionApps(apps) => *persisted = apps,
            Topic::Windows(ws) => {
                *running = ws.into_iter().map(|w| w.app_id).collect();
            }
            other => self.handle(other),
        }
    }

    pub fn launch(&mut self, payload: LaunchAppPayload) {
        let LaunchAppPayload { app_id, command } = payload;
        info!(%app_id, %command, "launch");

        let trimmed = command.trim();
        if trimmed.is_empty() {
            warn!(%app_id, "empty command");
            self.emit_launch_result(&app_id, &command, false, Some("empty command".into()));
            return;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(program) = parts.next() else {
            warn!(%app_id, %command, "no program");
            self.emit_launch_result(&app_id, &command, false, Some("no program".into()));
            return;
        };
        let args: Vec<&str> = parts.collect();

        self.launch_counter += 1;
        let unit = format!(
            "sola-app-{}-{}.scope",
            sanitize_unit_segment(&app_id),
            self.launch_counter
        );

        // Prefer systemd-run --user scopes when a user manager is up (normal
        // TTY dogfood). Loginless install seats often have no systemd --user
        // (runuser without PAM/logind) — scopes fail immediately and apps
        // look like they "launch then quit". Fall back to a direct spawn.
        let use_scope = user_systemd_available();
        let mut cmd = if use_scope {
            let mut c = Command::new("systemd-run");
            c.args([
                "--user",
                "--scope",
                "--quiet",
                "--collect",
                &format!("--unit={unit}"),
                &format!("--description=Sola app: {app_id}"),
                "--property=TimeoutStopSec=5s",
                "--property=KillSignal=SIGTERM",
                "--",
                program,
            ]);
            c.args(&args);
            c
        } else {
            info!(%app_id, "user systemd unavailable — direct spawn (no scope)");
            let mut c = Command::new(program);
            c.args(&args);
            c
        };

        if let Some(name) = env::wayland_socket() {
            cmd.env("WAYLAND_DISPLAY", name);
        } else {
            warn!("no WAYLAND_DISPLAY available");
        }
        if let Some(display) = env::x_display() {
            cmd.env("DISPLAY", display);
        }
        // Ensure seat apps inherit a runtime dir for wayland sockets, etc.
        if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
            cmd.env("XDG_RUNTIME_DIR", runtime);
        }

        // sola launches us (a MANAGED process) with SOLA_NO_SELF_WATCH=1
        // so we don't double-restart — sola already respawns its managed
        // set on binary change. But `systemd-run --scope` execs the user
        // app as our child, so it would inherit that flag and skip its
        // own self-watch. User apps are NOT in sola's managed set, so
        // nothing else restarts them on redeploy — they must self-watch.
        // Strip the flag so the kit's startup() arms watch_own_binary and
        // `cargo make install <app>` reloads the running app.
        cmd.env_remove("SOLA_NO_SELF_WATCH");

        // Capture the app's own stdout+stderr to a durable per-app log at
        // `/opt/sola/log/app-<id>.log`. Without this the scope's output is
        // lost, so an app that dies during restore before it can log anything
        // itself (a cold-boot GPU/wayland race, a panic before its own tracing
        // hook is armed, an `iced::Result` error) leaves nothing to diagnose.
        // Always on — no flag to remember. Falls back to inheriting
        // sola-session's stdio if the log file can't be opened.
        cmd.stdin(Stdio::null());
        if let Some(out) = app_log_file(&app_id) {
            match out.try_clone() {
                Ok(err) => {
                    cmd.stdout(Stdio::from(out));
                    cmd.stderr(Stdio::from(err));
                }
                Err(e) => warn!(%app_id, %e, "app capture: fd clone failed, inheriting stdio"),
            }
        }

        let unit = if use_scope {
            unit
        } else {
            // Marker so close/shutdown kill the process instead of systemctl.
            format!("direct-{}", sanitize_unit_segment(&app_id))
        };

        match cmd.spawn() {
            Ok(child) => {
                info!(%app_id, %unit, pid = child.id(), "user app launched");
                self.children
                    .entry(app_id.clone())
                    .or_default()
                    .push(AppRecord {
                        app_id: app_id.clone(),
                        command: command.clone(),
                        unit,
                        child,
                        launched_at: Instant::now(),
                        closing: false,
                    });
                self.emit_launch_result(&app_id, &command, true, None);
                self.emit_session_apps();
            }
            Err(e) => {
                warn!(%app_id, %e, "spawn failed");
                self.emit_launch_result(&app_id, &command, false, Some(e.to_string()));
            }
        }
    }

    pub fn handle(&mut self, topic: Topic) {
        match topic {
            Topic::LaunchApp(p) => self.launch(p),
            Topic::CloseApp(app_id) => self.close(&app_id),
            Topic::Shutdown => {
                self.shutdown_all_apps();
                std::process::exit(0);
            }
            _ => {}
        }
    }

    pub fn close(&mut self, app_id: &str) {
        let Some(records) = self.children.get_mut(app_id) else {
            info!(%app_id, "CloseApp: no live children");
            return;
        };
        for r in records.iter_mut() {
            if r.closing {
                continue;
            }
            r.closing = true;
            info!(%app_id, unit = %r.unit, "CloseApp: stopping");
            stop_app_record(r);
        }
        // Drop the now-closing app from the persisted set so a restart
        // doesn't relaunch something the user just closed.
        self.emit_session_apps();
    }

    /// Stop every scope, then poll for exit up to [`SHUTDOWN_POLL_BUDGET`].
    /// Anything still alive after that gets a hard kill on the systemd-run
    /// client and a second `systemctl stop` on the unit. Belt-and-braces:
    /// systemd's own SIGTERM→SIGKILL escalation will already have cleared
    /// the cgroup in practice, but we don't want to wait forever.
    fn shutdown_all_apps(&mut self) {
        for records in self.children.values_mut() {
            for r in records.iter_mut() {
                if r.closing {
                    continue;
                }
                r.closing = true;
                info!(app_id = %r.app_id, unit = %r.unit, "shutdown: stopping");
                stop_app_record(r);
            }
        }

        let deadline = Instant::now() + SHUTDOWN_POLL_BUDGET;
        loop {
            let mut alive = 0usize;
            for records in self.children.values_mut() {
                for r in records.iter_mut() {
                    match r.child.try_wait() {
                        Ok(Some(_)) => {}
                        Ok(None) => alive += 1,
                        Err(e) => {
                            warn!(app_id = %r.app_id, %e, "shutdown: try_wait failed");
                        }
                    }
                }
            }
            if alive == 0 || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        for records in self.children.values_mut() {
            for r in records.iter_mut() {
                if let Ok(None) = r.child.try_wait() {
                    warn!(app_id = %r.app_id, unit = %r.unit, pid = r.child.id(), "shutdown: forcing down");
                    // SAFETY: kill(2) is unconditionally safe to call.
                    unsafe { libc::kill(r.child.id() as i32, libc::SIGKILL) };
                    if !r.unit.starts_with("direct-") {
                        stop_scope(&r.unit);
                    }
                }
            }
        }
    }

    pub fn tick(&mut self) {
        self.reap_exited();
    }

    fn reap_exited(&mut self) {
        let mut to_emit: Vec<(String, String, std::process::ExitStatus)> = Vec::new();
        for (_app_id, records) in self.children.iter_mut() {
            records.retain_mut(|r| match r.child.try_wait() {
                Ok(Some(status)) => {
                    to_emit.push((r.app_id.clone(), r.command.clone(), status));
                    false
                }
                Ok(None) => true,
                Err(e) => {
                    warn!(app_id = %r.app_id, pid = r.child.id(), %e, "try_wait failed");
                    true
                }
            });
        }
        self.children.retain(|_, v| !v.is_empty());
        let reaped_any = !to_emit.is_empty();
        for (app_id, command, status) in to_emit {
            info!(%app_id, ?status, "user app exited");
            self.emit_exited(&app_id, &command, status);
        }
        if reaped_any {
            self.emit_session_apps();
        }
    }
}

/// Stop a launched app: systemd scope or direct child process.
fn stop_app_record(r: &AppRecord) {
    if r.unit.starts_with("direct-") {
        // Direct spawn — SIGTERM the process (and process group if possible).
        // SAFETY: kill(2) is safe for a pid we own.
        unsafe {
            libc::kill(r.child.id() as i32, libc::SIGTERM);
        }
        return;
    }
    stop_scope(&r.unit);
}

/// `systemctl --user stop --no-block <unit>`. Returns immediately — we
/// poll the systemd-run client for the scope's actual exit elsewhere.
fn stop_scope(unit: &str) {
    match Command::new("systemctl")
        .args(["--user", "stop", "--no-block", unit])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            // Reap immediately so we don't leave a zombie. systemctl
            // with --no-block returns within a few ms.
            let _ = child.wait();
        }
        Err(e) => warn!(%unit, %e, "failed to spawn systemctl stop"),
    }
}

/// True when this process has a working `systemd --user` manager (needed
/// for `systemd-run --user --scope`). Loginless install seats often do not.
fn user_systemd_available() -> bool {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        let private = std::path::Path::new(&dir).join("systemd/private");
        if private.exists() {
            return true;
        }
    }
    // Probe without relying on path layout (some setups use different sockets).
    match Command::new("systemctl")
        .args(["--user", "is-system-running"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        // 0 = running; 1 often means degraded but still usable.
        Ok(st) => st.success() || st.code() == Some(1),
        Err(_) => false,
    }
}

/// Replace anything outside `[A-Za-z0-9_-]` with `_` so the result is a
/// safe systemd unit-name segment. App IDs are usually plain ASCII
/// already; this is just defense.
fn sanitize_unit_segment(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Cap for a per-app capture log before it's truncated on the next launch, so
/// a chatty long-lived app (steam, an editor) can't grow it without bound.
/// Crash-on-launch output is tiny and is written *after* this truncation, so
/// bounding here never races away an imminent failure's log.
const APP_LOG_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Open the durable per-app capture log at `/opt/sola/log/app-<id>.log`
/// (create + append), truncating first if it has grown past
/// [`APP_LOG_MAX_BYTES`]. Returns `None` — leaving the caller on inherited
/// stdio — if the directory or file can't be opened.
fn app_log_file(app_id: &str) -> Option<std::fs::File> {
    let dir = std::path::Path::new("/opt/sola/log");
    if let Err(e) = std::fs::create_dir_all(dir) {
        warn!(%app_id, %e, "app capture: could not create /opt/sola/log");
        return None;
    }
    let path = dir.join(format!("app-{}.log", sanitize_unit_segment(app_id)));
    let oversized = std::fs::metadata(&path)
        .map(|m| m.len() > APP_LOG_MAX_BYTES)
        .unwrap_or(false);

    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).write(true);
    if oversized {
        opts.truncate(true);
    } else {
        opts.append(true);
    }
    match opts.open(&path) {
        Ok(f) => Some(f),
        Err(e) => {
            warn!(%app_id, %e, "app capture: could not open {}", path.display());
            None
        }
    }
}

/// Collapse `(app_id, command)` pairs into one `SessionApp` per `app_id`,
/// sorted by `app_id` for a stable (churn-free) persisted value.
fn session_apps_from_pairs(
    pairs: impl IntoIterator<Item = (String, String)>,
) -> Vec<SessionApp> {
    let mut apps: Vec<SessionApp> = pairs
        .into_iter()
        .map(|(app_id, command)| SessionApp { app_id, command })
        .collect();
    apps.sort_by(|a, b| a.app_id.cmp(&b.app_id));
    apps.dedup_by(|a, b| a.app_id == b.app_id);
    apps
}

/// The subset of `persisted` to relaunch on restore: apps that aren't
/// already running (per the sticky `Windows` list) and aren't already a
/// tracked child. Keeps restore from duplicating live apps.
fn restore_plan(
    persisted: &[SessionApp],
    running: &HashSet<String>,
    children_keys: &HashSet<String>,
) -> Vec<SessionApp> {
    persisted
        .iter()
        .filter(|a| !running.contains(&a.app_id) && !children_keys.contains(&a.app_id))
        .cloned()
        .collect()
}

/// Topics sola-session needs. Re-applied after every bus reconnect so a
/// mid-session `sola-bus` restart does not leave LaunchApp undelivered
/// (launcher toast with no process — the arcade/games launch failure mode).
const SESSION_TOPICS: &[TopicKind] = &[
    TopicKind::LaunchApp,
    TopicKind::CloseApp,
    TopicKind::Shutdown,
    // For session restore: the persisted open-app set plus the sticky
    // current-window list (to skip apps already running).
    TopicKind::SessionApps,
    TopicKind::Windows,
];

fn connect_and_subscribe(bus: &mut BusClient) {
    bus.connect_blocking(Duration::from_secs(1));
    if let Err(e) = bus.subscribe(SESSION_TOPICS) {
        warn!(%e, "sola-session subscribe failed");
    }
    info!("sola-session connected to bus");
}

pub fn run() {
    let mut session = Session::new();

    connect_and_subscribe(&mut session.bus);

    // Relaunch last session's apps (once, before the steady-state loop).
    session.restore_session();

    let poll = Duration::from_millis(500);
    loop {
        // sola-bus restarts drop the reader; kit apps reconnect via their
        // poller. Session is the LaunchApp owner — must reconnect itself or
        // launcher/arcade spawns vanish (toast only).
        if !session.bus.is_connected() {
            warn!("sola-session bus disconnected; reconnecting");
            connect_and_subscribe(&mut session.bus);
        }

        while let Some(msg) = session.bus.try_recv() {
            if let Some(topic) = Topic::parse(&msg) {
                session.handle(topic);
            }
        }
        if let Some(msg) = session.bus.recv_timeout(poll) {
            if let Some(topic) = Topic::parse(&msg) {
                session.handle(topic);
            }
        }
        session.tick();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, cmd: &str) -> SessionApp {
        SessionApp { app_id: id.into(), command: cmd.into() }
    }

    fn set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn session_apps_from_pairs_one_per_app_sorted() {
        let apps = session_apps_from_pairs([
            ("sola-terminal".to_string(), "/opt/sola/bin/sola-terminal".to_string()),
            ("helium".to_string(), "helium".to_string()),
            // Duplicate app_id (a second instance) collapses to one entry.
            ("sola-terminal".to_string(), "/opt/sola/bin/sola-terminal".to_string()),
        ]);
        assert_eq!(apps, vec![
            app("helium", "helium"),
            app("sola-terminal", "/opt/sola/bin/sola-terminal"),
        ]);
    }

    #[test]
    fn restore_plan_excludes_running_and_children() {
        let persisted = vec![app("a", "a"), app("b", "b"), app("c", "c")];
        let running = set(&["b"]);
        let children = set(&["c"]);
        let plan = restore_plan(&persisted, &running, &children);
        assert_eq!(plan, vec![app("a", "a")]);
    }

    #[test]
    fn restore_plan_empty_when_all_running() {
        // The bare-restart case: every persisted app is already up, so the
        // restore relaunches nothing (no duplicates).
        let persisted = vec![app("a", "a"), app("b", "b")];
        let running = set(&["a", "b"]);
        let plan = restore_plan(&persisted, &running, &HashSet::new());
        assert!(plan.is_empty());
    }

    #[test]
    fn restore_plan_relaunches_all_on_cold_boot() {
        let persisted = vec![app("a", "a"), app("b", "b")];
        let plan = restore_plan(&persisted, &HashSet::new(), &HashSet::new());
        assert_eq!(plan, persisted);
    }
}
