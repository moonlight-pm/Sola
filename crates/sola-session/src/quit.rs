//! Quitting an app that isn't Sola's (Super+Q, its last window gone, the
//! desk ending): ask it the way it asks itself — its own quit command when
//! it has one (`steam -shutdown`, what Steam → Exit does) — give it
//! [`CLEAN`] to go, then SIGTERM whatever still carries its tag
//! ([`crate::tag`]), and SIGKILL what is left [`TERM`] after that.

use std::path::Path;
use std::process::Child;
use std::time::{Duration, Instant};

use sola_bus::topics::LaunchAppPayload;
use tracing::{info, warn};

use crate::tag;

/// Apps whose running instance quits on a command of its own: the
/// program's file name, and the arguments that ask it to.
const CLEAN_QUIT: &[(&str, &[&str])] = &[("steam", &["-shutdown"])];

/// How long an app has to quit on its own command.
pub const CLEAN: Duration = Duration::from_secs(15);
/// How long it has after SIGTERM.
pub const TERM: Duration = Duration::from_secs(5);
/// How long SIGKILL has to take before the rest is given up on.
const KILL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Its own quit command was sent.
    Clean,
    /// SIGTERM was sent.
    Term,
    /// SIGKILL was sent.
    Kill,
}

pub enum Step {
    Going,
    Gone,
    /// Still there after SIGKILL.
    Stuck(Vec<u32>),
}

pub struct Quit {
    pub phase: Phase,
    pub until: Instant,
    /// The quit command while it runs.
    helper: Option<Child>,
    /// Picked again while it quit: started once it is gone.
    pub then_launch: Option<LaunchAppPayload>,
}

/// The command that asks `command`'s running app to quit, if it has one:
/// the same program with its quit arguments.
pub fn clean_command(command: &str) -> Option<(String, Vec<String>)> {
    let program = command.split_whitespace().next()?;
    let name = Path::new(program).file_name()?.to_str()?;
    let (_, args) = CLEAN_QUIT.iter().find(|(n, _)| *n == name)?;
    Some((program.to_string(), args.iter().map(|a| a.to_string()).collect()))
}

impl Quit {
    /// Begin. `helper` is its quit command, already started; without one,
    /// whatever carries the tag gets SIGTERM now.
    pub fn start(app_id: &str, helper: Option<Child>, now: Instant) -> Self {
        let phase = match helper {
            Some(_) => Phase::Clean,
            None => {
                tag::signal(&tag::pids(app_id), libc::SIGTERM);
                Phase::Term
            }
        };
        let until = now + if phase == Phase::Clean { CLEAN } else { TERM };
        Self {
            phase,
            until,
            helper,
            then_launch: None,
        }
    }

    /// Move on: gone once nothing carries the tag.
    pub fn step(&mut self, app_id: &str, now: Instant) -> Step {
        if let Some(h) = self.helper.as_mut() {
            if !matches!(h.try_wait(), Ok(None)) {
                self.helper = None;
            }
        }
        let alive = tag::pids(app_id);
        if alive.is_empty() {
            return Step::Gone;
        }
        if now < self.until {
            return Step::Going;
        }
        match self.phase {
            Phase::Clean => {
                info!(app_id, left = alive.len(), "quit: still running after its own quit; SIGTERM");
                tag::signal(&alive, libc::SIGTERM);
                self.phase = Phase::Term;
                self.until = now + TERM;
            }
            Phase::Term => {
                warn!(app_id, left = alive.len(), "quit: still running after SIGTERM; SIGKILL");
                tag::signal(&alive, libc::SIGKILL);
                self.phase = Phase::Kill;
                self.until = now + KILL;
            }
            Phase::Kill => return Step::Stuck(alive),
        }
        Step::Going
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    #[test]
    fn steam_quits_on_its_own_command() {
        assert_eq!(
            clean_command("/bin/steam"),
            Some(("/bin/steam".into(), vec!["-shutdown".into()]))
        );
        assert_eq!(clean_command("/bin/firefox --new-window"), None);
        assert_eq!(clean_command(""), None);
    }

    /// A grandchild in a session of its own, under PID 1 (Steam's
    /// srt-logger, Wine's servers): the tag still finds it.
    #[test]
    fn a_grandchild_that_left_the_tree_goes_too() {
        let tag = format!("quit-orphan-{}", std::process::id());
        let mut sh = Command::new("sh")
            .args(["-c", "setsid sh -c 'sleep 30 &' ; sleep 30"])
            .env(tag::VAR, &tag)
            .stdin(Stdio::null())
            .spawn()
            .expect("spawn sh");
        std::thread::sleep(Duration::from_millis(300));
        let before = tag::pids(&tag);
        let orphans = before.iter().filter(|&&p| p != sh.id() && tag::parent_of(p) != Some(sh.id()));
        assert_eq!(orphans.count(), 1, "the orphan is out of the tree: {before:?}");
        let t0 = Instant::now();
        let mut q = Quit::start(&tag, None, t0);
        let _ = sh.wait();
        std::thread::sleep(Duration::from_millis(300));
        assert!(matches!(q.step(&tag, t0), Step::Gone), "left: {:?}", tag::pids(&tag));
    }

    /// A tagged shell that ignores SIGTERM, and its child: SIGTERM, then
    /// SIGKILL, clears both.
    #[test]
    fn what_ignores_sigterm_gets_sigkill() {
        let tag = format!("quit-test-{}", std::process::id());
        let mut sh = Command::new("sh")
            .args(["-c", "trap '' TERM; sleep 30 & wait; wait"])
            .env(tag::VAR, &tag)
            .stdin(Stdio::null())
            .spawn()
            .expect("spawn sh");
        std::thread::sleep(Duration::from_millis(300));
        let t0 = Instant::now();
        let mut q = Quit::start(&tag, None, t0);
        assert_eq!(q.phase, Phase::Term);
        std::thread::sleep(Duration::from_millis(300));
        let _ = sh.try_wait();
        assert!(matches!(q.step(&tag, t0), Step::Going), "the trapping shell is still up");
        assert!(matches!(q.step(&tag, t0 + TERM), Step::Going));
        assert_eq!(q.phase, Phase::Kill);
        std::thread::sleep(Duration::from_millis(300));
        let _ = sh.wait();
        assert!(matches!(q.step(&tag, t0 + TERM), Step::Gone));
    }
}
