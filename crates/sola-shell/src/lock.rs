//! The lock screen, and the desk's end, with Oath's login service
//! (`sola_core::login`; `docs/specs/2026-10-29-secure-access-design.md`).
//!
//! `sola-lock` locks. The shell starts it on Super+L (the flower menu's Lock
//! Screen), as the desk starts when a password is set and auto-login is off,
//! and when the idle fade is complete if Lock when idle is on. A lock that
//! dies is started again: the compositor keeps the screen locked meanwhile
//! and hands the lock to the new one.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use sola_core::login;
use tracing::{error, info, warn};

/// The flower menu's Lock Screen (Super+L).
pub const ACTION_LOCK: &str = "lock";

/// Lock now. `Err`: why not, as a toast.
pub fn lock(why: &'static str) -> Result<(), &'static str> {
    match login::status() {
        Ok(s) if s.password => {}
        Ok(_) => return Err("Set a password in Settings → Login to lock"),
        Err(login::Error::Unavailable) => return Err("Locking needs Oath's login service"),
        Err(e) => {
            warn!("lock: {e}");
            return Err("Could not lock the screen");
        }
    }
    spawn(why);
    Ok(())
}

/// As the desk starts: lock at once when a password is set and auto-login
/// is off. Once per compositor, so a restarted shell does not lock again.
pub fn at_start() {
    let Ok(status) = login::status() else { return };
    if !status.locks_at_start() {
        return;
    }
    let marker = sola_core::env::runtime_dir().join("sola-start-locked");
    if let Some(instance) = compositor_instance() {
        if std::fs::read_to_string(&marker).is_ok_and(|seen| seen == instance) {
            return;
        }
        let _ = std::fs::write(&marker, instance);
    }
    info!("auto-login is off: locking as the desk starts");
    spawn("start");
}

/// The compositor's socket by inode and change time: a new compositor
/// makes a new one.
fn compositor_instance() -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let display = std::env::var("WAYLAND_DISPLAY").ok()?;
    let path = if display.starts_with('/') {
        PathBuf::from(display)
    } else {
        sola_core::env::runtime_dir().join(display)
    };
    let m = std::fs::metadata(path).ok()?;
    Some(format!("{}-{}", m.ino(), m.ctime()))
}

/// Run `sola-lock`, again if it dies while locked (at most five starts a
/// minute).
fn spawn(why: &'static str) {
    let run = move || {
        let bin = sola_core::paths::bin_path("sola-lock");
        let mut starts: Vec<Instant> = Vec::new();
        loop {
            starts.retain(|t| t.elapsed() < Duration::from_secs(60));
            if starts.len() >= 5 {
                error!("sola-lock keeps failing; not starting it again");
                return;
            }
            starts.push(Instant::now());
            match std::process::Command::new(&bin).arg(why).status() {
                // Unlocked, or another sola-lock holds the lock.
                Ok(s) if s.success() => return,
                // No password, or no login service: nothing to lock with.
                Ok(s) if matches!(s.code(), Some(2 | 3)) => {
                    warn!(?s, "sola-lock did not lock");
                    return;
                }
                Ok(s) => warn!(?s, "sola-lock ended while locked; starting it again"),
                Err(e) => {
                    error!(bin = %bin.display(), "sola-lock: {e}");
                    return;
                }
            }
        }
    };
    if let Err(e) = std::thread::Builder::new().name("sola-lock".into()).spawn(run) {
        error!("sola-lock thread: {e}");
    }
}

/// Exit Sola's last step on Oath: once sola-session has closed the apps,
/// tell Oath the desk ended on purpose, so the console gets the seat.
pub fn desk_down_after_session() {
    if !login::available() {
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && session_running() {
        std::thread::sleep(Duration::from_millis(100));
    }
    match login::desk_down() {
        Ok(()) => info!("told Oath the desk ended: the console has the seat"),
        Err(e) => warn!("could not tell Oath the desk ended: {e}"),
    }
}

/// sola-session is still closing the apps.
fn session_running() -> bool {
    let Ok(procs) = std::fs::read_dir("/proc") else {
        return false;
    };
    procs.flatten().any(|p| {
        std::fs::read_to_string(p.path().join("comm")).is_ok_and(|c| c.trim() == "sola-session")
    })
}
