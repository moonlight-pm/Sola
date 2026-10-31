//! Make the daemon hard to read (P7): no ptrace, no `/proc/<pid>/mem`, no
//! core dump from the same user; memory locked out of swap where the
//! memlock limit allows it.

use tracing::{info, warn};

pub fn apply() {
    // SAFETY: prctl with integer arguments only.
    let rc = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
    if rc == 0 {
        info!("vault: not dumpable (no ptrace, no core)");
    } else {
        warn!(error = %std::io::Error::last_os_error(), "vault: PR_SET_DUMPABLE refused");
    }
    lock_memory();
}

/// `mlockall(MCL_CURRENT | MCL_FUTURE)` only under an unlimited memlock
/// limit: under a finite one, MCL_FUTURE makes any allocation past it
/// fail, which would crash the daemon later instead of now.
fn lock_memory() {
    let mut lim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit writes the struct we pass.
    if unsafe { libc::getrlimit(libc::RLIMIT_MEMLOCK, &mut lim) } != 0 {
        return warn!(error = %std::io::Error::last_os_error(), "vault: memlock limit unreadable; memory not locked");
    }
    if lim.rlim_cur != libc::RLIM_INFINITY {
        return info!(
            limit_bytes = lim.rlim_cur,
            "vault: memlock limit is finite; memory not locked (raise LimitMEMLOCK to lock it)"
        );
    }
    // SAFETY: flags only.
    if unsafe { libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE) } == 0 {
        info!("vault: memory locked");
    } else {
        warn!(error = %std::io::Error::last_os_error(), "vault: mlockall refused; memory not locked");
    }
}
