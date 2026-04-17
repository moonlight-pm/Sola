//! Spawn /usr/bin/river as a child and own its lifecycle.
//!
//! River is the wayland compositor every sola wayland client depends on.
//! This module encapsulates that dependency: spawn River with stdio captured
//! to a dedicated log file, block until its `wayland-N` socket appears,
//! publish the socket name and XWayland DISPLAY, and surface process exit.
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tracing::{error, info, warn};

pub struct RiverSupervisor {
    child: Child,
    runtime_dir: PathBuf,
}

/// Filename (inside XDG_RUNTIME_DIR) where sola publishes the name of
/// the live wayland socket so sola components don't have to guess.
/// Contents are just the socket name, e.g. `wayland-1`.
pub const SOLA_WAYLAND_NAME_FILE: &str = "sola-wayland";

/// Filename (inside XDG_RUNTIME_DIR) where sola publishes the X11
/// DISPLAY that River's XWayland is serving. Contents look like `:0`.
/// File is absent when XWayland isn't active.
pub const SOLA_DISPLAY_NAME_FILE: &str = "sola-display";

const X_UNIX_DIR: &str = "/tmp/.X11-unix";

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

/// SIGKILL any `/usr/bin/river` process running as our uid, then wait
/// up to 1s for them all to reap — the kernel doesn't immediately
/// release flock/bind when a signal is sent, it happens after the
/// process actually exits.
fn kill_orphan_rivers() {
    let pids = find_river_pids();
    if pids.is_empty() {
        return;
    }
    for &pid in &pids {
        warn!(pid, "killing orphan river");
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if find_river_pids().is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    warn!(remaining = ?find_river_pids(), "river processes still alive after kill");
}

fn find_river_pids() -> Vec<i32> {
    let Ok(proc) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let our_uid = unsafe { libc::getuid() };
    let mut pids = Vec::new();
    for entry in proc.flatten() {
        let Some(name) = entry.file_name().to_str().map(String::from) else {
            continue;
        };
        let Ok(pid) = name.parse::<i32>() else { continue };
        let Ok(target) = std::fs::read_link(entry.path().join("exe")) else {
            continue;
        };
        if target != Path::new("/usr/bin/river") {
            continue;
        }
        let Ok(status) = std::fs::read_to_string(entry.path().join("status")) else {
            continue;
        };
        let owned = status
            .lines()
            .find_map(|l| l.strip_prefix("Uid:"))
            .and_then(|l| l.split_whitespace().next())
            .and_then(|s| s.parse::<u32>().ok())
            == Some(our_uid);
        if owned {
            pids.push(pid);
        }
    }
    pids
}

/// Find the first `wayland-N` (N != 'x*') socket in `runtime_dir` that
/// accepts a connection. Returns the socket *name* (e.g. `"wayland-1"`),
/// not the full path.
fn find_live_wayland_socket(runtime_dir: &Path) -> Option<String> {
    let dir = std::fs::read_dir(runtime_dir).ok()?;
    let mut candidates: Vec<String> = dir
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            if !name.starts_with("wayland-") {
                return None;
            }
            if name.starts_with("wayland-x") || name.ends_with(".lock") {
                return None;
            }
            Some(name)
        })
        .collect();
    // Lowest N first so we're deterministic if multiple servers are live.
    candidates.sort();
    for name in candidates {
        let path = runtime_dir.join(&name);
        if std::os::unix::net::UnixStream::connect(&path).is_ok() {
            return Some(name);
        }
    }
    None
}

/// Write the discovered socket name to `$XDG_RUNTIME_DIR/sola-wayland`
/// so other sola components can find it.
fn publish_socket_name(runtime_dir: &Path, name: &str) -> io::Result<()> {
    let path = runtime_dir.join(SOLA_WAYLAND_NAME_FILE);
    std::fs::write(&path, name)?;
    info!(path = %path.display(), %name, "published wayland socket name");
    Ok(())
}

/// Return the lowest-numbered X11 display (e.g. `":0"`) whose
/// `/tmp/.X11-unix/X<N>` socket accepts a connection, or `None` if none
/// do. We probe liveness rather than trusting the filesystem entry —
/// X sockets are often left behind by dead X servers.
pub fn probe_live_x_display() -> Option<String> {
    let dir = std::fs::read_dir(X_UNIX_DIR).ok()?;
    let mut candidates: Vec<(u32, PathBuf)> = dir
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_owned();
            let n = name.strip_prefix('X')?.parse::<u32>().ok()?;
            Some((n, e.path()))
        })
        .collect();
    candidates.sort_by_key(|(n, _)| *n);
    for (n, path) in candidates {
        if std::os::unix::net::UnixStream::connect(&path).is_ok() {
            return Some(format!(":{n}"));
        }
    }
    None
}

/// Write `:N` to `$XDG_RUNTIME_DIR/sola-display` so other sola components
/// know which X11 display River's XWayland is serving.
fn publish_display_name(runtime_dir: &Path, display_name: &str) -> io::Result<()> {
    let path = runtime_dir.join(SOLA_DISPLAY_NAME_FILE);
    std::fs::write(&path, display_name)?;
    info!(path = %path.display(), display = %display_name, "published DISPLAY");
    Ok(())
}

/// Remove every `wayland-N` / `wayland-N.lock` in the runtime dir that is
/// not actively held. `wayland-x0` (XWayland) is preserved. Files that a
/// live process holds via `flock` cannot be cleaned up by file removal
/// alone, but removing the directory entry at least prevents stale inodes
/// from fooling our connect probes.
fn cleanup_stale_sockets(runtime_dir: &Path) {
    let Ok(dir) = std::fs::read_dir(runtime_dir) else {
        return;
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with("wayland-") || name.starts_with("wayland-x") {
            continue;
        }
        let path = entry.path();
        match std::fs::remove_file(&path) {
            Ok(()) => info!(path = %path.display(), "removed stale socket"),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => warn!(path = %path.display(), %e, "failed to remove stale socket"),
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

        // Kill any orphan River process first so we don't fight a previous
        // instance for socket names. Also delete the stale `sola-wayland`
        // name file — it'll be rewritten by `publish_socket_name` below.
        kill_orphan_rivers();
        cleanup_stale_sockets(&runtime_dir);
        let _ = std::fs::remove_file(runtime_dir.join(SOLA_WAYLAND_NAME_FILE));
        let _ = std::fs::remove_file(runtime_dir.join(SOLA_DISPLAY_NAME_FILE));

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
        Ok(Self {
            child,
            runtime_dir,
        })
    }

    /// Poll `/tmp/.X11-unix/` for a live X server (one whose socket accepts
    /// a connection) for up to `timeout`, and publish the corresponding
    /// `:N` to `$XDG_RUNTIME_DIR/sola-display`. Stale socket files left by
    /// dead X servers are ignored — we only trust a live connect. Missing
    /// XWayland within the window is silent; the display file is absent
    /// and consumers fall back to their own probe.
    pub fn wait_for_xwayland(&self, timeout: Duration) {
        let start = Instant::now();
        loop {
            if let Some(display) = probe_live_x_display() {
                if let Err(e) = publish_display_name(&self.runtime_dir, &display) {
                    warn!(%e, "failed to publish DISPLAY");
                }
                return;
            }
            if start.elapsed() >= timeout {
                info!("no XWayland display appeared; skipping DISPLAY publish");
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Block (with exponential backoff 10ms → 1s, total cap 30s) until
    /// River is accepting wayland connections on *some* `wayland-N`.
    ///
    /// Returns the name River actually opened (e.g. `wayland-1`). On
    /// success, also writes that name to `$XDG_RUNTIME_DIR/sola-wayland`
    /// so other sola components can discover it without guessing.
    ///
    /// File existence isn't enough: a stale socket file from a prior
    /// crashed session looks identical to a fresh one, so we actually
    /// connect and immediately drop to verify a live listener.
    pub fn wait_for_socket(&mut self) -> io::Result<String> {
        let start = Instant::now();
        let total_cap = Duration::from_secs(30);
        let mut delay = Duration::from_millis(10);
        let cap = Duration::from_secs(1);
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(io::Error::other(format!(
                    "river exited before opening a socket: {status:?}"
                )));
            }
            if let Some(name) = find_live_wayland_socket(&self.runtime_dir) {
                let path = self.runtime_dir.join(&name);
                info!(path = %path.display(), "river socket is live");
                publish_socket_name(&self.runtime_dir, &name)?;
                return Ok(name);
            }
            if start.elapsed() > total_cap {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "no wayland-N socket went live within 30s",
                ));
            }
            std::thread::sleep(delay);
            delay = std::cmp::min(delay * 2, cap);
        }
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

}
