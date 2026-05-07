//! Pre-CEF stderr filter that drops a small set of known-noise lines
//! emitted by C/C++/Rust code below the tracing layer.
//!
//! The current target is a single upstream wart: `cef-rs 147.1.0+147.0.10`
//! has a stray `eprintln!("Invalid UTF-16 string")` in `src/string.rs:508`
//! inside `impl From<&CefStringUserfreeUtf16> for CefStringUtf16`. It
//! fires whenever an inbound CEF userfree-UTF16 string happens to be
//! NULL — an empty/transferred string, not actually an error. We hit it
//! on every renderer-side MessageRouter dispatch where the IPC payload's
//! BROWSER_PAYLOAD slot decodes as an empty string.
//!
//! ## How it works
//!
//! 1. Open a pipe.
//! 2. `dup` the original `stderr` (fd 2) to a saved fd so we can still
//!    forward un-dropped lines to wherever the user's `stderr` is going
//!    (TTY, redirected file, etc.).
//! 3. `dup2` the pipe write end onto fd 2. Every process and every
//!    thread that writes to fd 2 from this point on writes into the
//!    pipe — including CEF subprocesses we fork+exec later, which
//!    inherit the dup2'd fd 2 across exec.
//! 4. Spawn a single reader thread that line-buffers the pipe, drops
//!    lines matching any of the patterns, and `write(2)`s everything
//!    else to the saved original-stderr fd.
//!
//! Because step 3 happens before `cef::initialize` forks workers, the
//! filter covers all CEF subprocess stderr too without each subprocess
//! needing to install its own filter.
//!
//! Atomicity: POSIX guarantees `write()` calls up to `PIPE_BUF` (4096
//! bytes on Linux) are atomic, so single-shot eprintln/fprintf lines
//! reach the reader as undivided units. Adversarial multi-write
//! concatenations could in theory interleave, but in practice none of
//! our targets emit that way.

use std::io::{BufRead, BufReader};
use std::os::fd::FromRawFd;
use std::sync::OnceLock;

/// Substrings that, when found in any whole stderr line, cause that
/// line to be dropped silently.
const NOISE_PATTERNS: &[&str] = &[
    // cef-rs 147.1.0+147.0.10 src/string.rs:508 — stray eprintln when
    // a CefStringUserfreeUtf16 inner pointer is NULL during the
    // userfree → owning conversion. Hit on every renderer-side
    // MessageRouter dispatch with an empty STRING payload.
    "Invalid UTF-16 string",
];

static INSTALLED: OnceLock<()> = OnceLock::new();

/// Install the filter once. Subsequent calls are no-ops. Call before
/// any CEF init so subprocess workers inherit the redirected fd 2.
pub fn install() {
    if INSTALLED.set(()).is_err() {
        return;
    }
    if let Err(e) = install_inner() {
        // Use the *original* stderr path here — the filter isn't up
        // yet, so plain eprintln goes to wherever stderr currently
        // points. Best-effort: if install fails, the noise just
        // continues showing as before.
        eprintln!("stderr_filter: install failed ({e}); continuing without filter");
    }
}

fn install_inner() -> std::io::Result<()> {
    let mut fds = [0i32; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let read_fd = fds[0];
    let write_fd = fds[1];

    let orig_stderr = unsafe { libc::dup(libc::STDERR_FILENO) };
    if orig_stderr < 0 {
        let err = std::io::Error::last_os_error();
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
        return Err(err);
    }

    if unsafe { libc::dup2(write_fd, libc::STDERR_FILENO) } < 0 {
        let err = std::io::Error::last_os_error();
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
            libc::close(orig_stderr);
        }
        return Err(err);
    }
    unsafe { libc::close(write_fd) };

    std::thread::Builder::new()
        .name("sola-kit-stderr-filter".into())
        .spawn(move || filter_loop(read_fd, orig_stderr))?;

    Ok(())
}

fn filter_loop(read_fd: i32, orig_stderr: i32) {
    // SAFETY: `read_fd` is a freshly-opened pipe fd that nothing else
    // references. We take ownership for the lifetime of this thread.
    let reader = unsafe { std::fs::File::from_raw_fd(read_fd) };
    let mut buf = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        match buf.read_line(&mut line) {
            Ok(0) => break, // pipe closed (process tearing down)
            Ok(_) => {
                let trimmed = line.trim_end_matches(['\n', '\r']);
                if NOISE_PATTERNS.iter().any(|p| trimmed.contains(p)) {
                    continue;
                }
                // SAFETY: `orig_stderr` was dup'd from the original
                // stderr fd at install time and isn't closed for the
                // lifetime of the process.
                unsafe {
                    libc::write(
                        orig_stderr,
                        line.as_ptr() as *const libc::c_void,
                        line.len(),
                    );
                }
            }
            Err(_) => break,
        }
    }
}
