//! The Cloudflare Tunnel connector: `cloudflared tunnel run` as a child of
//! Design (it dies with Design), restarted with backoff if it exits. The
//! tunnel's ingress — set on Cloudflare — points the public host at the
//! share listener. The token goes in through the environment, not argv.
//! Its pid is kept in the runtime dir: Design re-execs itself on install
//! (the child survives an exec), so a fresh start stops the old connector.

use std::fs::OpenOptions;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LOG: &str = "/opt/sola/log/design-tunnel.log";
const BACKOFF_MAX: Duration = Duration::from_secs(60);

pub struct Tunnel {
    child: Arc<Mutex<Option<Child>>>,
    stop: Arc<AtomicBool>,
}

impl Tunnel {
    pub fn start(token: &str, bin: Option<&Path>) -> io::Result<Self> {
        let bin = match bin {
            Some(b) => b.to_path_buf(),
            None => sola_core::process::resolve_binary("cloudflared")?,
        };
        stop_previous();
        let child: Arc<Mutex<Option<Child>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let first = spawn(&bin, token)?;
        tracing::info!(pid = first.id(), bin = %bin.display(), log = LOG, "share tunnel connector up");
        *child.lock().unwrap() = Some(first);
        let (watch_child, watch_stop, token) = (child.clone(), stop.clone(), token.to_string());
        std::thread::Builder::new()
            .name("design-tunnel".into())
            .spawn(move || supervise(&bin, &token, &watch_child, &watch_stop))?;
        Ok(Self { child, stop })
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(mut c) = self.child.lock().unwrap().take() {
            sola_core::process::graceful_shutdown(&mut c, Duration::from_secs(3));
        }
    }
}

fn pid_path() -> PathBuf {
    let runtime =
        std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
    runtime.join("sola").join("design-tunnel.pid")
}

/// A connector left by an earlier Design (before an exec): stop it.
fn stop_previous() {
    let Some(pid) = std::fs::read_to_string(pid_path())
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
    else {
        return;
    };
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    if !String::from_utf8_lossy(&cmdline).contains("cloudflared") {
        return;
    }
    tracing::info!(pid, "stopping the previous share tunnel connector");
    // SAFETY: kill(2) on a pid we checked is a cloudflared.
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Path::new(&format!("/proc/{pid}")).exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn spawn(bin: &PathBuf, token: &str) -> io::Result<Child> {
    let log = OpenOptions::new().create(true).append(true).open(LOG)?;
    let err = log.try_clone()?;
    let mut cmd = Command::new(bin);
    cmd.args(["tunnel", "--no-autoupdate", "run"])
        .env("TUNNEL_TOKEN", token)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(err);
    // SAFETY: prctl is async-signal-safe (see sola_core::process).
    unsafe {
        cmd.pre_exec(sola_core::process::set_pdeathsig_sigterm);
    }
    let child = cmd.spawn()?;
    let path = pid_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, child.id().to_string());
    Ok(child)
}

/// Respawn the connector when it exits, backing off while it keeps dying.
fn supervise(bin: &PathBuf, token: &str, child: &Mutex<Option<Child>>, stop: &AtomicBool) {
    let mut backoff = Duration::from_secs(2);
    let mut up_since = Instant::now();
    loop {
        std::thread::sleep(Duration::from_secs(1));
        if stop.load(Ordering::Acquire) {
            return;
        }
        let exited = {
            let mut guard = child.lock().unwrap();
            match guard.as_mut().map(Child::try_wait) {
                Some(Ok(Some(status))) => {
                    guard.take();
                    Some(status)
                }
                _ => None,
            }
        };
        let Some(status) = exited else {
            continue;
        };
        // A connector that ran a good while gets a fresh, short backoff.
        if up_since.elapsed() > BACKOFF_MAX {
            backoff = Duration::from_secs(2);
        }
        tracing::warn!(%status, retry_in = ?backoff, "share tunnel connector exited");
        std::thread::sleep(backoff);
        if stop.load(Ordering::Acquire) {
            return;
        }
        match spawn(bin, token) {
            Ok(c) => {
                tracing::info!(pid = c.id(), "share tunnel connector restarted");
                *child.lock().unwrap() = Some(c);
                up_since = Instant::now();
            }
            Err(e) => tracing::error!(error = %e, "share tunnel connector respawn"),
        }
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}
