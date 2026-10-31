//! A link to another computer Projects has sessions on.
//!
//! While Projects has a session open on a host, it keeps one ssh
//! connection there of its own: it follows that computer's hook log
//! (`tail -F`, [`crate::hooks::log`]), so the hooks of every agent there —
//! whichever computer started its session — reach this one, and every
//! event knows which computer it came from.
//!
//! [`connect`] runs on a thread of its own: it learns the host (its home,
//! its agents' homes), installs the hooks there, starts following, says
//! [`LinkEvent::Up`], then reads the log until the connection ends
//! ([`LinkEvent::Lost`]). The follower dies with Projects
//! (`PR_SET_PDEATHSIG`, bound to that thread, which outlives it).

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use iced::Subscription;
use iced::futures::Stream;
use sola_core::ssh;

use crate::hooks::HookPaths;
use crate::hooks::log;
use crate::machine::{Machine, Remote};

/// How long the follower must stay up to count as started.
const SETTLE: Duration = Duration::from_millis(400);

#[derive(Debug, Clone)]
pub enum LinkEvent {
    /// `host` is reachable and its hooks come here.
    Up {
        host: String,
        remote: Arc<Remote>,
        /// The follower's pid (to stop it, and to tell this link's
        /// events from an older one's).
        pid: u32,
    },
    /// `host` could not be linked.
    Failed { host: String, why: String },
    /// `host`'s follower `pid` ended.
    Lost { host: String, pid: u32, why: String },
}

/// What Projects knows of a host's link.
#[derive(Debug, Clone)]
pub enum Link {
    Connecting,
    Up { machine: Machine, pid: u32 },
    Down { why: String, since: Instant },
}

/// Link `host`: on a thread, which reports through [`subscription`].
pub fn spawn(host: String) {
    let tx = events_tx();
    let name = format!("link-{host}");
    let spawned = std::thread::Builder::new().name(name).spawn(move || {
        connect(&host, &tx);
    });
    if let Err(e) = spawned {
        tracing::error!("link thread: {e}");
    }
}

/// Stop the follower `pid` (its link then says [`LinkEvent::Lost`]).
pub fn stop(pid: u32) {
    // SAFETY: plain kill(2) on a pid this process started.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
}

fn connect(host: &str, tx: &mpsc::Sender<LinkEvent>) {
    let failed = |why: String| {
        tracing::warn!(%host, "link failed: {why}");
        let _ = tx.send(LinkEvent::Failed {
            host: host.to_string(),
            why,
        });
    };
    let remote = match learn(host) {
        Ok(r) => Arc::new(r),
        Err(why) => return failed(why),
    };
    let m = Machine::Remote(remote.clone());
    if let Err(e) = crate::hooks::install::install_on(&m, &HookPaths::remote(&remote)) {
        // Sessions still work there; only their marks would not.
        tracing::warn!(%host, "remote hook install failed: {e}");
    }
    let mut child = match follower(host) {
        Ok(c) => c,
        Err(e) => return failed(format!("ssh: {e}")),
    };
    let pid = child.id();
    std::thread::sleep(SETTLE);
    if let Ok(Some(status)) = child.try_wait() {
        let why = stderr_of(&mut child);
        return failed(if why.is_empty() { format!("ssh ended ({status})") } else { why });
    }
    tracing::info!(%host, pid, "linked; following its hook log");
    let _ = tx.send(LinkEvent::Up {
        host: host.to_string(),
        remote,
        pid,
    });
    let events = crate::hooks::sender();
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines() {
            let Ok(line) = line else {
                break;
            };
            if let Some(ev) = log::parse(&line, Some(host)) {
                let _ = events.send(ev);
            }
        }
    }
    let why = match child.wait() {
        Ok(status) if status.success() => "the connection closed".to_string(),
        Ok(status) => {
            let why = stderr_of(&mut child);
            if why.is_empty() { format!("ssh ended ({status})") } else { why }
        }
        Err(e) => format!("ssh: {e}"),
    };
    tracing::warn!(%host, pid, "link lost: {why}");
    let _ = tx.send(LinkEvent::Lost {
        host: host.to_string(),
        pid,
        why,
    });
}

fn stderr_of(child: &mut Child) -> String {
    let mut err = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        use std::io::Read;
        let _ = pipe.read_to_string(&mut err);
    }
    ssh::reason(err.as_bytes())
}

/// `host`'s home and its agents' homes (it has tmux, or tabs cannot run).
fn learn(host: &str) -> Result<Remote, String> {
    let script = "echo \"$HOME\"; echo \"${GROK_HOME:-$HOME/.grok}\"; \
        echo \"${CODEX_HOME:-$HOME/.codex}\"; echo \"${CLAUDE_CONFIG_DIR:-$HOME/.claude}\"; \
        command -v tmux >/dev/null || exit 4; command -v base64 >/dev/null || echo nobase64";
    let out = ssh::run(ssh::batch(host, script), None)?;
    if out.status.code() == Some(4) {
        return Err(format!("tmux is not installed on {host}"));
    }
    if !out.status.success() {
        return Err(ssh::reason(&out.stderr));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    let [home, grok, codex, claude, ..] = lines[..] else {
        return Err(format!("{host} did not say where its home is"));
    };
    if lines.contains(&"nobase64") {
        tracing::warn!(%host, "no base64 there: its agents' hooks cannot log (no marks)");
    }
    let path = |p: &str| PathBuf::from(p.trim());
    Ok(Remote {
        host: host.to_string(),
        home: path(home),
        grok_home: path(grok),
        codex_home: path(codex),
        claude_home: path(claude),
    })
}

/// `ssh host tail -F` on that computer's hook log, from its end.
fn follower(host: &str) -> std::io::Result<Child> {
    use std::os::unix::process::CommandExt;
    let script = format!(
        "l=\"$HOME/{rel}\"; mkdir -p \"${{l%/*}}\" && touch \"$l\" && exec tail -n0 -F \"$l\"",
        rel = log::REL
    );
    let mut cmd = ssh::follow(host, &script);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: prctl is async-signal-safe; it runs between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    cmd.spawn()
}

static TX: OnceLock<mpsc::Sender<LinkEvent>> = OnceLock::new();
static RX: Mutex<Option<mpsc::Receiver<LinkEvent>>> = Mutex::new(None);

fn events_tx() -> mpsc::Sender<LinkEvent> {
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        *RX.lock().unwrap_or_else(|p| p.into_inner()) = Some(rx);
        tx
    })
    .clone()
}

pub fn subscription() -> Subscription<LinkEvent> {
    Subscription::run(event_stream)
}

fn event_stream() -> impl Stream<Item = LinkEvent> {
    let _ = events_tx();
    let rx = RX.lock().unwrap_or_else(|p| p.into_inner()).take();
    let (iced_tx, iced_rx) = iced::futures::channel::mpsc::unbounded();
    if let Some(rx) = rx {
        std::thread::spawn(move || {
            while let Ok(ev) = rx.recv() {
                if iced_tx.unbounded_send(ev).is_err() {
                    break;
                }
            }
        });
    }
    iced_rx
}
