use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use tracing::{info, warn};

use sola_bus::BusClient;
use sola_bus::topics::{
    LaunchAppPayload, LaunchResultPayload, Topic, TopicKind, UserAppExitedPayload,
};

use crate::env;

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
        Self { bus, children: HashMap::new() }
    }

    fn emit_launch_result(
        &mut self,
        app_id: &str,
        command: &str,
        ok: bool,
        error: Option<String>,
    ) {
        let _ = self.bus.emit(Topic::LaunchResult(LaunchResultPayload {
            app_id: app_id.to_string(),
            command: command.to_string(),
            ok,
            error,
        }));
    }

    fn emit_exited(
        &mut self,
        app_id: &str,
        command: &str,
        status: std::process::ExitStatus,
    ) {
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

        // SAFETY: pre_exec runs in the child after fork. PR_SET_PDEATHSIG asks
        // the kernel to kill the child if sola-session dies, preventing orphans.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
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

    pub fn close(&mut self, _app_id: &str) {
        // Implemented in Task 4.5.
    }

    pub fn tick(&mut self) {
        // Implemented in Tasks 4.4/4.5.
    }
}

pub fn run() {
    let mut session = Session::new();

    // Connect (retry until up).
    loop {
        match session.bus.connect() {
            Ok(()) => break,
            Err(e) => {
                warn!(%e, "bus connect failed, retrying");
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }

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
