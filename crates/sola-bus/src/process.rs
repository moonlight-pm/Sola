//! The process at the other end of a connection: who it is, and when it
//! ends. The bus retracts what a process wrote when it ends for good.

use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

/// The pid of the process that opened `stream`, from the kernel
/// (`SO_PEERCRED`). `None` if the kernel cannot say.
pub fn peer_pid(stream: &UnixStream) -> Option<i32> {
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` are valid for writes of `ucred`'s size, and
    // the fd is open for the call (borrowed from `stream`).
    let r = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    (r == 0 && cred.pid > 0).then_some(cred.pid)
}

/// Block until process `pid` ends (a pidfd becomes readable when it
/// exits). Returns at once if it has already gone.
pub fn wait_for_exit(pid: i32) -> io::Result<()> {
    // SAFETY: pidfd_open takes a pid and flags and returns a new fd or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as i32;
    if fd < 0 {
        let e = io::Error::last_os_error();
        return match e.raw_os_error() {
            Some(libc::ESRCH) => Ok(()),
            _ => Err(e),
        };
    }
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let result = loop {
        // SAFETY: one valid pollfd, waiting without a timeout.
        let r = unsafe { libc::poll(&mut poll, 1, -1) };
        if r >= 0 {
            break Ok(());
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            break Err(e);
        }
    };
    // SAFETY: `fd` is ours and closed once.
    unsafe { libc::close(fd) };
    result
}
