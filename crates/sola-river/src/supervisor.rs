//! Spawn /usr/bin/river as a child and own its lifecycle.
//!
//! `sola-river` cannot do useful work without River. This module encapsulates
//! that dependency: spawn River with stdio captured to a dedicated log file,
//! block until its `wayland-0` socket appears, and surface process exit.
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tracing::{error, info, warn};

pub struct RiverSupervisor {
    child: Child,
    socket_path: PathBuf,
}

// Closure runs post-fork in the River child: inherit SIGTERM on parent
// death, put River in its own process group.
fn child_setup() -> io::Result<()> {
    // SAFETY: libc calls in a post-fork child; must be async-signal-safe.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
        libc::setsid();
    }
    Ok(())
}

/// Remove `wayland-0` and `wayland-0.lock` from `runtime_dir` if they
/// exist. River refuses to bind a socket name that already has a lockfile;
/// this unblocks startup after a crashed prior run.
fn cleanup_stale_sockets(runtime_dir: &Path) {
    for name in ["wayland-0", "wayland-0.lock"] {
        let path = runtime_dir.join(name);
        if let Err(e) = std::fs::remove_file(&path) {
            if e.kind() != io::ErrorKind::NotFound {
                warn!(path = %path.display(), %e, "failed to remove stale socket");
            }
        } else {
            info!(path = %path.display(), "removed stale socket");
        }
    }
}

impl RiverSupervisor {
    /// Spawn `/usr/bin/river`, redirecting stdout/stderr to `log_path`.
    /// Does not wait for the wayland socket.
    pub fn spawn(log_path: &Path) -> io::Result<Self> {
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/run/user/1000"));
        let socket_path = runtime_dir.join("wayland-0");

        // Clean up stale socket + lock so River can bind wayland-0. If a live
        // compositor is running on wayland-0, this still wouldn't hurt — we
        // can't connect to a socket with no listener either way, and the
        // `wait_for_socket` step below probes connectivity, not file
        // existence, so stale cruft here would have masked the real problem.
        cleanup_stale_sockets(&runtime_dir);

        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;
        let log_err = log.try_clone()?;

        let mut cmd = Command::new("/usr/bin/river");
        // `-c :` runs the shell no-op as River's init, skipping its hunt
        // for ~/.config/river/init — we drive the session from outside.
        cmd.args(["-log-level", "info", "-c", ":"])
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err));
        // wlroots picks its backend by env: WAYLAND_DISPLAY → nested wayland,
        // DISPLAY → X11 (both failed on canto from a bare TTY), otherwise
        // drm + libinput (what we actually want). Strip both so River falls
        // through to the DRM backend.
        cmd.env_remove("WAYLAND_DISPLAY");
        cmd.env_remove("DISPLAY");
        // SAFETY: `child_setup` only invokes async-signal-safe libc calls.
        unsafe {
            cmd.pre_exec(child_setup);
        }
        let child = cmd.spawn()?;

        info!(pid = child.id(), "spawned river");
        Ok(Self { child, socket_path })
    }

    /// Block (with exponential backoff 10ms → 1s, total cap 30s) until
    /// River is accepting wayland connections.
    ///
    /// File existence is not enough: a stale socket file from a prior
    /// crashed session looks identical to a fresh one, so we actually
    /// connect and immediately drop to verify a live listener.
    pub fn wait_for_socket(&mut self) -> io::Result<()> {
        let start = Instant::now();
        let total_cap = Duration::from_secs(30);
        let mut delay = Duration::from_millis(10);
        let cap = Duration::from_secs(1);
        loop {
            // If River exited before the socket came up, bail fast instead
            // of spinning until the timeout.
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(io::Error::other(format!(
                    "river exited before opening a socket: {status:?}"
                )));
            }
            if self.socket_path.exists() {
                match std::os::unix::net::UnixStream::connect(&self.socket_path) {
                    Ok(_) => {
                        info!(path = %self.socket_path.display(), "river socket is live");
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::trace!(%e, "socket file present but not accepting yet");
                    }
                }
            }
            if start.elapsed() > total_cap {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "river socket {} did not become live within 30s",
                        self.socket_path.display()
                    ),
                ));
            }
            std::thread::sleep(delay);
            delay = std::cmp::min(delay * 2, cap);
        }
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    /// SIGTERM, wait up to 2s, then SIGKILL.
    pub fn shutdown(&mut self) {
        let pid = self.child.id() as i32;
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() >= deadline => {
                    warn!(pid, "river did not exit after SIGTERM; sending SIGKILL");
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => {
                    error!(%e, pid, "error waiting on river");
                    return;
                }
            }
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}
