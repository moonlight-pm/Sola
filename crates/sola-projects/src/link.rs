//! A link to another computer Projects has projects on.
//!
//! While Projects has a project on a host, it keeps one ssh connection
//! there of its own (the keepalive: `ssh -N -R`). That connection carries
//! the host's hook socket back here: a socket there
//! (`~/.config/sola/remote/hooks-<this computer>.sock`) forwarded to one
//! here for that host alone (`sola-ws-hooks-<host>.sock`,
//! [`crate::hooks::listen_host`]). The agents in its tabs post their hooks
//! to it (`SOLA_WS_HOOKS_SOCK`), with the same scripts as here, installed
//! there by [`connect`] — so its tabs get marks, islands and waits as tabs
//! here do, and every hook knows which computer it came from.
//!
//! [`connect`] runs on a thread of its own: it learns the host (its home,
//! its agents' homes), installs the hooks, starts the keepalive, waits for
//! the socket there, says [`LinkEvent::Up`], then waits on the keepalive
//! until it ends ([`LinkEvent::Lost`]). The keepalive dies with Projects
//! (`PR_SET_PDEATHSIG`, bound to that thread, which outlives it).

use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use iced::Subscription;
use iced::futures::Stream;
use sola_core::ssh;

use crate::hooks::HookPaths;
use crate::machine::{Machine, Remote};

/// How long the socket there may take to appear once the keepalive runs.
const FORWARD_WAIT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone)]
pub enum LinkEvent {
    /// `host` is reachable and its hooks come here.
    Up {
        host: String,
        remote: Arc<Remote>,
        /// The keepalive's pid (to stop it, and to tell this link's
        /// events from an older one's).
        pid: u32,
    },
    /// `host` could not be linked.
    Failed { host: String, why: String },
    /// `host`'s keepalive `pid` ended.
    Lost { host: String, pid: u32, why: String },
}

/// What Projects knows of a host's link.
#[derive(Debug, Clone)]
pub enum Link {
    Connecting,
    Up { machine: Machine, pid: u32 },
    Down { why: String, since: Instant },
}

/// Link `host`, its hooks posting to `local_sock` here: on a thread, which
/// reports through [`subscription`].
pub fn spawn(host: String, local_sock: PathBuf) {
    let tx = events_tx();
    let name = format!("link-{host}");
    let spawned = std::thread::Builder::new().name(name).spawn(move || {
        connect(&host, &local_sock, &tx);
    });
    if let Err(e) = spawned {
        tracing::error!("link thread: {e}");
    }
}

/// Stop the keepalive `pid` (its link then says [`LinkEvent::Lost`]).
pub fn stop(pid: u32) {
    // SAFETY: plain kill(2) on a pid this process started.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
}

fn connect(host: &str, local_sock: &Path, tx: &mpsc::Sender<LinkEvent>) {
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
    let remote_sock = remote_socket(&remote);
    if let Err(e) = crate::hooks::install::install_on(&m, &HookPaths::remote(&remote, remote_sock.clone())) {
        // Tabs still work there; only their marks would not.
        tracing::warn!(%host, "remote hook install failed: {e}");
    }
    // A socket left by a link that died would refuse the forward.
    let _ = m.run(None, &[], &["rm", "-f", "--", &remote_sock.to_string_lossy()], None);
    let mut child = match keepalive(host, &remote_sock, local_sock) {
        Ok(c) => c,
        Err(e) => return failed(format!("ssh: {e}")),
    };
    let pid = child.id();
    if let Err(why) = wait_for_socket(&m, &remote_sock, &mut child) {
        stop(pid);
        let _ = child.wait();
        return failed(why);
    }
    tracing::info!(%host, pid, sock = %remote_sock.display(), "linked; hooks come here");
    let _ = tx.send(LinkEvent::Up {
        host: host.to_string(),
        remote,
        pid,
    });
    let why = match child.wait_with_output() {
        Ok(out) if out.status.success() => "the connection closed".to_string(),
        Ok(out) => ssh::reason(&out.stderr),
        Err(e) => format!("ssh: {e}"),
    };
    tracing::warn!(%host, pid, "link lost: {why}");
    let _ = tx.send(LinkEvent::Lost {
        host: host.to_string(),
        pid,
        why,
    });
}

/// `host`'s home and its agents' homes (it has tmux, or tabs cannot run).
fn learn(host: &str) -> Result<Remote, String> {
    let script = "echo \"$HOME\"; echo \"${GROK_HOME:-$HOME/.grok}\"; \
        echo \"${CODEX_HOME:-$HOME/.codex}\"; echo \"${CLAUDE_CONFIG_DIR:-$HOME/.claude}\"; \
        command -v tmux >/dev/null || exit 4; command -v curl >/dev/null || echo nocurl";
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
    if lines.contains(&"nocurl") {
        tracing::warn!(%host, "no curl there: its agents' hooks cannot post (no marks)");
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

/// The hook socket there for this computer.
pub fn remote_socket(r: &Remote) -> PathBuf {
    r.home
        .join(".config/sola/remote")
        .join(format!("hooks-{}.sock", crate::spawn::slug(&crate::machine::this_host())))
}

fn keepalive(host: &str, remote_sock: &Path, local_sock: &Path) -> std::io::Result<Child> {
    use std::os::unix::process::CommandExt;
    let mut cmd = ssh::forward(
        host,
        &remote_sock.to_string_lossy(),
        &local_sock.to_string_lossy(),
    );
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
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

/// Until the forward's socket is there; the keepalive ending first fails.
fn wait_for_socket(m: &Machine, sock: &Path, child: &mut Child) -> Result<(), String> {
    let deadline = Instant::now() + FORWARD_WAIT;
    let sock = sock.to_string_lossy();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let mut err = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                use std::io::Read;
                let _ = pipe.read_to_string(&mut err);
            }
            let why = ssh::reason(err.as_bytes());
            return Err(if err.trim().is_empty() {
                format!("ssh ended ({status})")
            } else {
                why
            });
        }
        let there = m
            .run(None, &[], &["test", "-S", &sock], None)
            .is_ok_and(|o| o.status.success());
        if there {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err("the hook socket did not come up there".into());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
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
