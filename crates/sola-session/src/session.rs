use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use tracing::{info, warn};

use sola_bus::BusClient;
use sola_bus::topics::{
    LaunchAppPayload, LaunchResultPayload, Topic, TopicKind, UserAppExitedPayload,
};

use sola_core::env;

const GRACEFUL: Duration = Duration::from_secs(5);
const FORCE_AFTER_TERM: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub enum CloseState {
    Live,
    Closing { since: Instant },
    Terminated { since: Instant },
    Killed,
}

pub struct ChildRecord {
    pub app_id: String,
    pub command: String,
    pub child: Child,
    #[allow(dead_code)]
    pub launched_at: Instant,
    pub state: CloseState,
}

pub struct Session {
    bus: BusClient,
    children: HashMap<String, Vec<ChildRecord>>,
}

impl Session {
    pub fn new() -> Self {
        let mut bus = BusClient::new();
        bus.set_app_id("sola-session");
        Self {
            bus,
            children: HashMap::new(),
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

        let mut cmd = Command::new(program);
        cmd.args(&args);
        if let Some(name) = env::wayland_socket() {
            cmd.env("WAYLAND_DISPLAY", name);
        } else {
            warn!("no WAYLAND_DISPLAY available");
        }
        if let Some(display) = env::x_display() {
            cmd.env("DISPLAY", display);
        }

        // SAFETY: pre_exec runs post-fork in the child before exec; the hook
        // is async-signal-safe.
        unsafe {
            cmd.pre_exec(sola_core::process::set_pdeathsig_sigterm);
        }

        match cmd.spawn() {
            Ok(child) => {
                info!(%app_id, pid = child.id(), "user app launched");
                self.children
                    .entry(app_id.clone())
                    .or_default()
                    .push(ChildRecord {
                        app_id: app_id.clone(),
                        command: command.clone(),
                        child,
                        launched_at: Instant::now(),
                        state: CloseState::Live,
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
            Topic::Shutdown => std::process::exit(0),
            _ => {}
        }
    }

    pub fn close(&mut self, app_id: &str) {
        let Some(records) = self.children.get_mut(app_id) else {
            info!(%app_id, "CloseApp: no live children");
            return;
        };
        for r in records.iter_mut() {
            if matches!(r.state, CloseState::Live) {
                info!(%app_id, pid = r.child.id(), "CloseApp: graceful period started");
                r.state = CloseState::Closing {
                    since: Instant::now(),
                };
            }
        }
    }

    pub fn tick(&mut self) {
        self.reap_exited();
        self.run_close_timers();
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

    fn run_close_timers(&mut self) {
        let now = Instant::now();
        for (_app_id, records) in self.children.iter_mut() {
            for r in records.iter_mut() {
                match r.state {
                    CloseState::Closing { since } if now.duration_since(since) >= GRACEFUL => {
                        info!(pid = r.child.id(), app_id = %r.app_id, "sending SIGTERM");
                        unsafe {
                            libc::kill(r.child.id() as i32, libc::SIGTERM);
                        }
                        r.state = CloseState::Terminated { since: now };
                    }
                    CloseState::Terminated { since }
                        if now.duration_since(since) >= FORCE_AFTER_TERM =>
                    {
                        info!(pid = r.child.id(), app_id = %r.app_id, "sending SIGKILL");
                        unsafe {
                            libc::kill(r.child.id() as i32, libc::SIGKILL);
                        }
                        r.state = CloseState::Killed;
                    }
                    _ => {}
                }
            }
        }
    }
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
