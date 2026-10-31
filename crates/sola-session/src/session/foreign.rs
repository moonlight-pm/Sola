//! The session's side of apps that aren't Sola's: tagged at launch
//! ([`crate::tag`]), quit when they have no window ([`crate::watch`]), and
//! quit the whole way — their own quit command, then every process they
//! started ([`crate::quit`]).

use std::process::{Child, Command};
use std::time::Instant;

use sola_bus::topics::{LaunchAppPayload, Window};
use tracing::{info, warn};

use super::{Session, prepare, stop_app_record};
use crate::quit::{self, Phase, Quit, Step};
use crate::tag;
use crate::watch::Foreign;

impl Session {
    /// A launch that isn't Sola's. A second launch of a running app (Steam
    /// handing itself to the running one) keeps the watch it has.
    pub(super) fn watch_foreign(&mut self, app_id: &str, command: &str) {
        self.foreign
            .entry(app_id.to_string())
            .or_insert_with(|| Foreign::launched(command, Instant::now()));
    }

    /// Picked while it quits: start it again once it is gone.
    pub(super) fn defer_while_quitting(&mut self, payload: LaunchAppPayload) -> Option<LaunchAppPayload> {
        let quit = self.foreign.get_mut(&payload.app_id).and_then(|f| f.quit.as_mut());
        let Some(quit) = quit else {
            return Some(payload);
        };
        info!(app_id = %payload.app_id, "launch: starts once the quitting one is gone");
        quit.then_launch = Some(payload);
        None
    }

    /// A live window list (not a retraction).
    pub(super) fn note_foreign_windows(&mut self, windows: &[Window]) {
        if self.foreign.is_empty() {
            return;
        }
        let open = self.owners.apps_with_windows(windows);
        let now = Instant::now();
        for (app_id, f) in self.foreign.iter_mut() {
            f.note(open.contains(app_id), now);
        }
    }

    /// Quit `app_id` the whole way. False when it isn't one of these. The
    /// caller drops it from the saved set when it should (not at Exit Sola).
    pub(super) fn quit_foreign(&mut self, app_id: &str, reason: &str) -> bool {
        let Some(f) = self.foreign.get(app_id) else {
            return false;
        };
        if f.quit.is_some() {
            return true;
        }
        let command = f.command.clone();
        for r in self.children.get_mut(app_id).into_iter().flatten() {
            r.closing = true;
        }
        self.adopted.retain(|a| a.app_id != app_id);
        // Its own quit command only while it runs: Steam's starts Steam
        // when there is none to hand the command to.
        let helper = match quit::clean_command(&command) {
            Some((program, args)) if !tag::pids(app_id).is_empty() => {
                self.spawn_quit_command(app_id, &program, &args)
            }
            _ => None,
        };
        info!(app_id, reason, own_quit = helper.is_some(), "quitting");
        let q = Quit::start(app_id, helper, Instant::now());
        if q.phase == Phase::Term {
            self.stop_records(app_id);
        }
        if let Some(f) = self.foreign.get_mut(app_id) {
            f.quit = Some(q);
        }
        true
    }

    /// Each tick: quit the ones that are due, and move the quits on.
    pub(super) fn step_foreign(&mut self) {
        let now = Instant::now();
        let due: Vec<(String, &'static str)> = self
            .foreign
            .iter()
            .filter_map(|(id, f)| f.due(now).map(|why| (id.clone(), why)))
            .collect();
        for (app_id, why) in due {
            if tag::pids(&app_id).is_empty() && !self.children.contains_key(&app_id) {
                info!(%app_id, "no process carries its tag; no longer watched");
                self.foreign.remove(&app_id);
                continue;
            }
            self.quit_foreign(&app_id, why);
            // Closed: not one to bring back next time.
            self.emit_session_apps();
        }
        self.step_quits(now);
    }

    /// Move each quit on a step.
    pub(super) fn step_quits(&mut self, now: Instant) {
        let quitting: Vec<String> = self
            .foreign
            .iter()
            .filter(|(_, f)| f.quit.is_some())
            .map(|(id, _)| id.clone())
            .collect();
        for app_id in quitting {
            let Some(q) = self.foreign.get_mut(&app_id).and_then(|f| f.quit.as_mut()) else {
                continue;
            };
            let before = q.phase;
            let step = q.step(&app_id, now);
            let to_term = before == Phase::Clean && q.phase == Phase::Term;
            match step {
                Step::Going if to_term => self.stop_records(&app_id),
                Step::Going => {}
                Step::Gone => {
                    info!(%app_id, "quit: every process gone");
                    let next = self.foreign.remove(&app_id).and_then(|f| f.quit?.then_launch);
                    if let Some(payload) = next {
                        self.launch(payload);
                    }
                }
                Step::Stuck(pids) => {
                    warn!(%app_id, ?pids, "quit: processes outlived SIGKILL; giving up on them");
                    self.foreign.remove(&app_id);
                }
            }
        }
    }

    /// Whether any of them is still quitting.
    pub(super) fn foreign_quitting(&self) -> bool {
        self.foreign.values().any(|f| f.quit.is_some())
    }

    /// The desk is ending: SIGKILL what is left of them.
    pub(super) fn kill_foreign(&mut self) {
        for app_id in self.foreign.drain().map(|(id, _)| id) {
            let left = tag::pids(&app_id);
            if !left.is_empty() {
                warn!(%app_id, left = left.len(), "shutdown: forcing down");
                tag::signal(&left, libc::SIGKILL);
            }
        }
    }

    /// Its quit command, tagged like the app: a Steam it starts by mistake
    /// (the running one went meanwhile) goes with the rest.
    fn spawn_quit_command(&self, app_id: &str, program: &str, args: &[String]) -> Option<Child> {
        let mut cmd = Command::new(program);
        cmd.args(args);
        prepare(&mut cmd, app_id, true);
        match cmd.spawn() {
            Ok(child) => {
                info!(app_id, program, ?args, pid = child.id(), "quit: its own quit command");
                Some(child)
            }
            Err(e) => {
                warn!(app_id, program, %e, "quit: its own quit command did not start");
                None
            }
        }
    }

    /// SIGTERM its launch records (a systemd scope stops as a whole).
    fn stop_records(&self, app_id: &str) {
        for r in self.children.get(app_id).into_iter().flatten() {
            stop_app_record(r);
        }
    }
}
