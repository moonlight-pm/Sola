use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tracing::{info, warn};

use sola_bus::BusClient;
use sola_bus::topics::{
    LaunchAppPayload, LaunchResultPayload, Topic, TopicKind, UserAppExitedPayload,
};

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

        let mut cmd = Command::new("systemd-run");
        cmd.args([
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
        cmd.args(&args);

        if let Some(name) = env::wayland_socket() {
            cmd.env("WAYLAND_DISPLAY", name);
        } else {
            warn!("no WAYLAND_DISPLAY available");
        }
        if let Some(display) = env::x_display() {
            cmd.env("DISPLAY", display);
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

        // Inherit stdio: systemd-run proxies the scope's stdio to its
        // own, so this lands in the journal alongside sola-session.
        cmd.stdin(Stdio::null());

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
            info!(%app_id, unit = %r.unit, "CloseApp: stopping scope");
            stop_scope(&r.unit);
        }
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
                info!(app_id = %r.app_id, unit = %r.unit, "shutdown: stopping scope");
                stop_scope(&r.unit);
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
                    warn!(app_id = %r.app_id, unit = %r.unit, pid = r.child.id(), "shutdown: forcing scope down");
                    // SAFETY: kill(2) is unconditionally safe to call.
                    unsafe { libc::kill(r.child.id() as i32, libc::SIGKILL) };
                    stop_scope(&r.unit);
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
        for (app_id, command, status) in to_emit {
            info!(%app_id, ?status, "user app exited");
            self.emit_exited(&app_id, &command, status);
        }
    }
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

pub fn run() {
    let mut session = Session::new();

    // Connect (retry until up).
    session.bus.connect_blocking(Duration::from_secs(1));

    let _ = session.bus.subscribe(&[
        TopicKind::LaunchApp,
        TopicKind::CloseApp,
        TopicKind::Shutdown,
    ]);

    info!("sola-session connected to bus");

    let poll = Duration::from_millis(500);
    loop {
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
