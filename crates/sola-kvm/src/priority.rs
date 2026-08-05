//! Best-effort CPU / I/O priority boost for the KVM path.
//!
//! Negative niceness and real-time scheduling need `CAP_SYS_NICE` (or root).
//! We always try; failures are logged at debug and the process continues at
//! default priority. UDP socket priority does not need elevated caps.

use std::io;
use std::os::fd::AsRawFd;
use std::os::fd::RawFd;

use tracing::{debug, info, warn};

/// Target niceness (lower = higher priority). -10 is a common interactive tier.
const TARGET_NICE: i32 = -10;

/// Linux `SO_PRIORITY` for interactive / high-priority traffic (0–6 without CAP_NET_ADMIN).
#[cfg(target_os = "linux")]
const SO_PRIORITY_VALUE: libc::c_int = 6;

/// IPTOS_LOWDELAY (0x10) — hint routers/NICs to prefer latency.
#[cfg(target_os = "linux")]
const IPTOS_LOWDELAY: libc::c_int = 0x10;

/// Raise this process's CPU scheduling priority as far as permitted.
pub fn boost_process() {
    match set_nice(TARGET_NICE) {
        Ok(n) => info!(nice = n, "process niceness raised for KVM latency"),
        Err(e) => {
            // Expected without CAP_SYS_NICE; still try a milder value.
            debug!(%e, target = TARGET_NICE, "setpriority(-10) failed");
            match set_nice(-5) {
                Ok(n) => info!(nice = n, "process niceness raised (mild)"),
                Err(e2) => debug!(%e2, "setpriority(-5) failed; staying at default nice"),
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Prefer the current thread for input/UDP work without requiring RT caps.
        // SCHED_BATCH would hurt us; leave SCHED_OTHER but ask for higher nice only.
        // Optionally try a soft real-time policy if permitted (often denied).
        if let Err(e) = try_sched_fifo_low() {
            debug!(%e, "SCHED_FIFO not available (need CAP_SYS_NICE); using CFS + nice");
        }
    }
}

/// Apply latency-oriented options on a connected/bound UDP socket.
pub fn boost_udp_socket(sock: &impl AsRawFd) {
    let fd = sock.as_raw_fd();
    #[cfg(target_os = "linux")]
    {
        if let Err(e) = set_sock_priority(fd, SO_PRIORITY_VALUE) {
            debug!(%e, "SO_PRIORITY failed");
        } else {
            debug!(prio = SO_PRIORITY_VALUE, "UDP SO_PRIORITY set");
        }
        if let Err(e) = set_ip_tos(fd, IPTOS_LOWDELAY) {
            debug!(%e, "IP_TOS LOWDELAY failed");
        } else {
            debug!("UDP IP_TOS=LOWDELAY set");
        }
        // Larger receive buffer is less important on the sender; still bump send
        // buffer slightly so a brief CFS stall does not drop outbound motion.
        if let Err(e) = set_sndbuf(fd, 256 * 1024) {
            debug!(%e, "SO_SNDBUF failed");
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = fd;
    }
}

fn set_nice(value: i32) -> io::Result<i32> {
    // SAFETY: setpriority on self (who=0, which=PRIO_PROCESS) is well-defined.
    let rc = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, value) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(current_nice())
}

fn current_nice() -> i32 {
    // getpriority can legitimately return -1 (nice=-1); errno must be checked.
    // SAFETY: getpriority on self.
    unsafe {
        *libc::__errno_location() = 0;
        let n = libc::getpriority(libc::PRIO_PROCESS, 0);
        if n == -1 && *libc::__errno_location() != 0 {
            0
        } else {
            n
        }
    }
}

#[cfg(target_os = "linux")]
fn try_sched_fifo_low() -> io::Result<()> {
    // Priority 1 is the lowest FIFO band — enough to stay ahead of normal
    // desktop work without starving the system if something goes wrong.
    let param = libc::sched_param {
        sched_priority: 1,
    };
    // SAFETY: sched_setscheduler on self with a valid sched_param.
    let rc = unsafe { libc::sched_setscheduler(0, libc::SCHED_FIFO, &param) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    info!("SCHED_FIFO priority=1 enabled for sola-kvm");
    Ok(())
}

#[cfg(target_os = "linux")]
fn set_sock_priority(fd: RawFd, prio: libc::c_int) -> io::Result<()> {
    // SAFETY: setsockopt with SO_PRIORITY on a valid UDP fd.
    let rc = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PRIORITY,
            &prio as *const _ as *const libc::c_void,
            std::mem::size_of_val(&prio) as libc::socklen_t,
        )
    };
    if rc != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn set_ip_tos(fd: RawFd, tos: libc::c_int) -> io::Result<()> {
    // SAFETY: setsockopt IP_TOS on a valid UDP fd.
    let rc = unsafe {
        libc::setsockopt(
            fd,
            libc::IPPROTO_IP,
            libc::IP_TOS,
            &tos as *const _ as *const libc::c_void,
            std::mem::size_of_val(&tos) as libc::socklen_t,
        )
    };
    if rc != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn set_sndbuf(fd: RawFd, bytes: libc::c_int) -> io::Result<()> {
    let rc = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            &bytes as *const _ as *const libc::c_void,
            std::mem::size_of_val(&bytes) as libc::socklen_t,
        )
    };
    if rc != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Log a one-line warning if we could not raise nice at all (operators can
/// grant `CAP_SYS_NICE` or run under a higher-priority cgroup).
pub fn warn_if_still_default() {
    let nice = current_nice();
    if nice >= 0 {
        warn!(
            nice,
            "sola-kvm running at default/low niceness — intermittent lag under load is more likely. \
             Optional: `sudo setcap cap_sys_nice+ep /opt/sola/bin/sola-kvm` then restart"
        );
    }
}
