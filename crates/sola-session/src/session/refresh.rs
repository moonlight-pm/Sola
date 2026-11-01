//! Apps whose package moved start again, alone, on the new program
//! ([`crate::moved`]). The session manager keeps its children through its
//! own restart, so an apply no longer reopens every app: each one whose
//! program changed is stopped and started again here, and the others —
//! and apps that aren't Sola's, a game among them — keep running. Their
//! windows come back to the places they left (the shell keeps a closed
//! window's place for 30 s).

use std::path::Path;
use std::time::{Duration, Instant};

use sola_bus::topics::LaunchAppPayload;
use tracing::{info, warn};

use super::{Session, stop_app_record};
use crate::{moved, tag};

/// How often the running apps are checked.
pub(super) const CHECK_EVERY: Duration = Duration::from_secs(2);
/// How long after the session manager starts before the first check: a
/// desk restart restarts the shell too, and it places and pairs the kept
/// windows first (an app restarted under it while it does lands anywhere).
const SETTLE: Duration = Duration::from_secs(10);
/// How long an app this process did not start has to exit after SIGTERM.
const EXIT_GRACE: Duration = Duration::from_secs(10);

/// An app this process did not start (found running at its start), asked
/// to exit so it can start again.
pub(super) struct Moving {
    app_id: String,
    command: String,
    pid: u32,
    since: Instant,
    killed: bool,
}

impl Session {
    /// Stop each of Sola's apps that runs an old program; it starts again
    /// once it has exited ([`Self::relaunch_moved`], [`Self::step_moving`]).
    pub(super) fn refresh_moved(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.started_at) < SETTLE
            || self.checked_at.is_some_and(|t| now.duration_since(t) < CHECK_EVERY)
        {
            return;
        }
        self.checked_at = Some(now);
        let mut stopped = Vec::new();
        for r in self.children.values_mut().flatten() {
            // A scope's process is systemd-run, not the app.
            if r.closing || !r.unit.starts_with("direct-") || tag::is_foreign(&r.command) {
                continue;
            }
            if moved::moved(&r.command, r.child.id()) {
                info!(app_id = %r.app_id, pid = r.child.id(), "its package moved: starting it again on the new program");
                stop_app_record(r);
                r.closing = true;
                stopped.push((r.app_id.clone(), r.command.clone()));
            }
        }
        self.relaunch.extend(stopped);
        let mut kept = Vec::new();
        for a in std::mem::take(&mut self.adopted) {
            let pid = self
                .windows
                .iter()
                .find(|w| w.app_id == a.app_id)
                .and_then(|w| w.pid);
            match pid {
                Some(pid) if !tag::is_foreign(&a.command) && moved::moved(&a.command, pid) => {
                    info!(app_id = %a.app_id, pid, "its package moved: starting it again on the new program");
                    // SAFETY: kill(2) on a pid read from its window.
                    unsafe {
                        libc::kill(pid as i32, libc::SIGTERM);
                    }
                    self.moving.push(Moving {
                        app_id: a.app_id,
                        command: a.command,
                        pid,
                        since: now,
                        killed: false,
                    });
                }
                _ => kept.push(a),
            }
        }
        self.adopted = kept;
    }

    /// An app this process started exited: start it again if it was
    /// stopped for a new program (and none of its processes is left).
    pub(super) fn relaunch_moved(&mut self, app_id: &str) {
        if self.children.contains_key(app_id) {
            return;
        }
        if let Some(command) = self.relaunch.remove(app_id) {
            self.launch(LaunchAppPayload {
                app_id: app_id.to_string(),
                command,
            });
        }
    }

    /// The apps found running at start that were asked to exit: start each
    /// again once it is gone (SIGKILL after [`EXIT_GRACE`]).
    pub(super) fn step_moving(&mut self) {
        let mut gone = Vec::new();
        for m in &mut self.moving {
            if !Path::new(&format!("/proc/{}", m.pid)).exists() {
                gone.push((m.app_id.clone(), m.command.clone()));
            } else if !m.killed && m.since.elapsed() >= EXIT_GRACE {
                warn!(app_id = %m.app_id, pid = m.pid, "still running after SIGTERM; SIGKILL");
                // SAFETY: kill(2) on the pid asked to exit above.
                unsafe {
                    libc::kill(m.pid as i32, libc::SIGKILL);
                }
                m.killed = true;
            }
        }
        if gone.is_empty() {
            return;
        }
        self.moving.retain(|m| !gone.iter().any(|(id, _)| *id == m.app_id));
        for (app_id, command) in gone {
            self.launch(LaunchAppPayload { app_id, command });
        }
    }
}
