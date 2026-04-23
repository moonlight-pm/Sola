//! Small helpers for spawning and tearing down child processes the way
//! Sola components consistently want: die with the parent, shut down
//! gracefully with a SIGTERM→SIGKILL escalation.

use std::io;
use std::process::Child;
use std::time::{Duration, Instant};

use tracing::warn;

/// `pre_exec` hook: ask the kernel to send SIGTERM to this child when
/// its parent dies. Use with [`std::os::unix::process::CommandExt::pre_exec`].
///
/// # Safety
///
/// `pre_exec` runs post-fork in the child; the body must be async-signal-safe.
/// `prctl(PR_SET_PDEATHSIG, ...)` is.
pub fn set_pdeathsig_sigterm() -> io::Result<()> {
    // SAFETY: async-signal-safe libc call in a post-fork child.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
    }
    Ok(())
}

/// `pre_exec` hook that combines [`set_pdeathsig_sigterm`] with
/// `setsid()`, putting the child in its own process group. Useful for
/// long-lived subprocesses that should be SIGKILLed as a group and
/// should not receive the parent's terminal signals.
///
/// # Safety
///
/// Same contract as [`set_pdeathsig_sigterm`].
pub fn set_pdeathsig_and_leader() -> io::Result<()> {
    // SAFETY: both calls are async-signal-safe.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
        libc::setsid();
    }
    Ok(())
}

/// Send SIGTERM to `child`, poll for exit for up to `sigterm_timeout`,
/// then SIGKILL + blocking wait if it's still alive. Safe to call on an
/// already-exited child (SIGTERM to a dead pid is just ESRCH).
pub fn graceful_shutdown(child: &mut Child, sigterm_timeout: Duration) {
    let pid = child.id() as i32;
    // SAFETY: kill(2) is safe to invoke from Rust; we just need to call it via libc.
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    let deadline = Instant::now() + sigterm_timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if Instant::now() >= deadline => {
                warn!(pid, "did not exit after SIGTERM; sending SIGKILL");
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                warn!(%e, pid, "error waiting on child during shutdown");
                return;
            }
        }
    }
}
