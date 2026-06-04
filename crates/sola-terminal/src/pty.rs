//! Per-tab PTY backend wired to the alacritty emulator.
//!
//! Each tab owns one [`PtyBackend`] (master fd + child pid + tmux session
//! name). The openpty + `tmux new-session -A` child-spawn logic is the same
//! one the legacy WebView path used; only the OUTPUT path changed: instead of
//! base64-encoding bytes into a bus event, the reader thread drives an
//! `alacritty_terminal` `Processor` straight into the tab's shared
//! `Term<Listener>` grid.
//!
//! Threading model (one reader thread per tab)
//! -------------------------------------------
//! - `App` state owns the `Emulator` (for the renderer + `resize`). The reader
//!   thread does NOT call `Emulator::advance` -- that would need `&mut` to
//!   state-owned data. Instead, at attach time we clone the term handle
//!   (`emulator.term()` -> `Arc<FairMutex<Term<Listener>>>`) and move that
//!   clone plus a FRESH `Processor` into the reader thread. The reader loop
//!   locks the term, advances bytes, and notifies iced.
//! - Terminal replies (DSR / cursor-position / DA) flow out through the
//!   `Listener`'s `pty_write` channel as `(tab_id, bytes)`. A SINGLE
//!   process-wide drain thread reads that channel and writes the bytes back to
//!   the tab's master fd, looked up in a global fd registry. A tab whose fd is
//!   gone (closed) is dropped silently -- the drain thread never panics.

use std::collections::HashMap;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::io::{IntoRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::sync::{Arc, Mutex, OnceLock, mpsc};

use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
use tracing::{debug, warn};

use crate::emulator::Listener;

// -- Process-wide pty-write drain ----------------------------------------------
//
// The `Listener` inside every `Term` sends replies as `(tab_id, bytes)` on a
// single process-wide channel. One drain thread looks up the tab's master fd in
// a global registry and writes the bytes. Registering/unregistering the fd is
// done by `PtyBackend` at attach/close.

/// Global pty-write sender, cloned for every tab's `Listener`.
static PTY_WRITE_TX: OnceLock<mpsc::Sender<(String, Vec<u8>)>> = OnceLock::new();

/// Map of `tab_id -> master fd`, populated at attach and cleared at close.
/// The drain thread reads it; `PtyBackend` mutates it.
static FD_REGISTRY: OnceLock<Mutex<HashMap<String, RawFd>>> = OnceLock::new();

fn fd_registry() -> &'static Mutex<HashMap<String, RawFd>> {
    FD_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Initialise the pty-write channel + drain thread exactly once.
fn ensure_pty_write_drain() {
    PTY_WRITE_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<(String, Vec<u8>)>();
        std::thread::spawn(move || {
            // Blocking recv -- exits when every sender is dropped (process end).
            while let Ok((tab_id, bytes)) = rx.recv() {
                let fd = {
                    // Poison-tolerant: a panic elsewhere must not kill the one
                    // process-wide drain thread (that would hang every TUI).
                    let map = fd_registry().lock().unwrap_or_else(|e| e.into_inner());
                    map.get(&tab_id).copied()
                };
                match fd {
                    Some(fd) => {
                        let n = unsafe {
                            libc::write(fd, bytes.as_ptr() as *const libc::c_void, bytes.len())
                        };
                        if n < 0 {
                            // fd closed out from under us, or transient error --
                            // drop the reply and keep draining. Never panic.
                            warn!(
                                tab_id = %tab_id,
                                "pty-write drain: write failed: {}",
                                std::io::Error::last_os_error()
                            );
                        }
                    }
                    // Tab is gone (closed). Drop the bytes, keep going.
                    None => {}
                }
            }
        });
        tx
    });
}

/// Returns a clone of the process-wide pty-write sender. Hand one to every
/// `Listener` so terminal replies reach the drain thread.
pub fn pty_write_sender() -> mpsc::Sender<(String, Vec<u8>)> {
    ensure_pty_write_drain();
    PTY_WRITE_TX.get().unwrap().clone()
}

fn register_fd(tab_id: &str, fd: RawFd) {
    // Poison-tolerant: see the drain thread's lock above.
    fd_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(tab_id.to_string(), fd);
}

fn unregister_fd(tab_id: &str) {
    // Poison-tolerant: see the drain thread's lock above.
    fd_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(tab_id);
}

// -- PtyBackend -- per-tab handle ----------------------------------------------

/// Per-tab PTY handle: master fd + child pid + tmux session name.
///
/// Drop semantics intentionally match the legacy `PtyManager`: a plain drop
/// (shutdown / crash) closes the master fd (once, via `OwnedFd`), unregisters
/// it from the drain registry, and SIGHUPs the tmux *client* but LEAVES THE
/// TMUX SESSION ALIVE so it can be restored. Only an explicit
/// [`close`](Self::close) tears down the tmux session.
pub struct PtyBackend {
    tab_id: String,
    /// The pty master. Owned exactly once here: dropping the `OwnedFd` is the
    /// single close of this fd (see `Drop`). `close()` must NOT close it.
    master_fd: OwnedFd,
    child_pid: i32,
    tmux_session: String,
}

impl PtyBackend {
    /// Open a pty, exec `tmux new-session -A -s <tmux_session>`, register the
    /// master fd for pty-write replies, and start the reader thread that drives
    /// `term` from the master fd.
    ///
    /// `cwd` sets the new session's start directory (ignored by tmux when the
    /// session already exists, i.e. on reattach). The reader thread sends
    /// `tab_id` on `notify` after each parse and on `exit` (EOF) when the
    /// shell dies.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_or_attach(
        tab_id: &str,
        tmux_session: &str,
        cols: u16,
        rows: u16,
        cwd: Option<&str>,
        term: Arc<FairMutex<Term<Listener>>>,
        notify: mpsc::Sender<String>,
        exit: mpsc::Sender<String>,
    ) -> std::io::Result<Self> {
        let winsize = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        let pty = nix::pty::openpty(Some(&winsize), None)
            .map_err(|e| std::io::Error::other(format!("openpty failed: {e}")))?;
        let slave_fd = pty.slave.into_raw_fd();

        let mut cmd = crate::tmux::tmux_cmd();
        cmd.args([
            "new-session",
            "-A",
            "-s",
            tmux_session,
            "-x",
            &cols.to_string(),
            "-y",
            &rows.to_string(),
        ]);
        // Start directory only applies when tmux actually creates the session;
        // on reattach (`-A` finds an existing session) tmux ignores `-c`.
        if let Some(dir) = cwd {
            cmd.args(["-c", dir]);
        }

        // SAFETY: pre_exec runs in the child between fork and exec; the libc
        // calls here are async-signal-safe.
        let child = unsafe {
            cmd.env("TERM", "xterm-256color")
                .pre_exec(move || {
                    libc::setsid();
                    libc::dup2(slave_fd, 0);
                    libc::dup2(slave_fd, 1);
                    libc::dup2(slave_fd, 2);
                    if slave_fd > 2 {
                        libc::close(slave_fd);
                    }
                    libc::ioctl(0, libc::TIOCSCTTY, 0);
                    Ok(())
                })
                .spawn()
                .map_err(|e| std::io::Error::other(format!("failed to spawn tmux: {e}")))?
        };

        // Parent: close slave, keep master.
        unsafe { libc::close(slave_fd) };
        let master_raw = pty.master.into_raw_fd();
        // SAFETY: `master_raw` comes straight from openpty (via nix's
        // OwnedFd -> into_raw_fd) and is not owned anywhere else. This is the
        // ONE place ownership of the master fd is taken; the OwnedFd closes it
        // exactly once when this `PtyBackend` drops.
        let master_fd = unsafe { OwnedFd::from_raw_fd(master_raw) };
        let child_pid = child.id() as i32;

        debug!(
            tab_id = %tab_id,
            tmux_session = %tmux_session,
            child_pid,
            master_fd = master_fd.as_raw_fd(),
            "spawned PTY"
        );

        // Register the raw fd so terminal replies can be written back. The
        // registry stores a bare int; the entry is removed (in close()/Drop)
        // before this OwnedFd drops, so the drain thread never sees a stale fd.
        register_fd(tab_id, master_fd.as_raw_fd());

        // Reader thread: own a dup'd fd + the term Arc + a FRESH Processor.
        // SAFETY: `libc::dup` returns a fresh, owned fd distinct from the
        // master; the reader thread is its sole owner and the OwnedFd closes it
        // exactly once when the loop exits.
        let read_fd = unsafe { OwnedFd::from_raw_fd(libc::dup(master_fd.as_raw_fd())) };
        let reader_tab_id = tab_id.to_string();
        std::thread::spawn(move || {
            let mut processor: Processor<StdSyncHandler> = Processor::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = unsafe {
                    libc::read(
                        read_fd.as_raw_fd(),
                        buf.as_mut_ptr() as *mut libc::c_void,
                        buf.len(),
                    )
                };
                // n == 0 is EOF (pty hung up by kill_session); n < 0 is error.
                // Either way the loop ends and `read_fd` (OwnedFd) drops here,
                // closing the dup'd fd exactly once.
                if n <= 0 {
                    break;
                }
                let chunk = &buf[..n as usize];
                {
                    let mut term = term.lock();
                    processor.advance(&mut *term, chunk);
                }
                let _ = notify.send(reader_tab_id.clone());
            }
            drop(read_fd);
            debug!(tab_id = %reader_tab_id, "PTY reader thread exited (EOF)");
            // Shell exited -- signal so `App` can close the tab.
            let _ = exit.send(reader_tab_id.clone());
        });

        Ok(Self {
            tab_id: tab_id.to_string(),
            master_fd,
            child_pid,
            tmux_session: tmux_session.to_string(),
        })
    }

    /// Write bytes to the master fd (keyboard input -> shell).
    pub fn write(&self, bytes: &[u8]) {
        let n = unsafe {
            libc::write(
                self.master_fd.as_raw_fd(),
                bytes.as_ptr() as *const libc::c_void,
                bytes.len(),
            )
        };
        if n < 0 {
            warn!(
                tab_id = %self.tab_id,
                "pty write failed: {}",
                std::io::Error::last_os_error()
            );
        }
    }

    /// Resize the pty (TIOCSWINSZ) and the tmux window to match.
    pub fn resize(&self, cols: u16, rows: u16) {
        let winsize = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let ret = unsafe {
            libc::ioctl(
                self.master_fd.as_raw_fd(),
                libc::TIOCSWINSZ,
                &winsize as *const libc::winsize,
            )
        };
        if ret < 0 {
            warn!(
                tab_id = %self.tab_id,
                "ioctl TIOCSWINSZ failed: {}",
                std::io::Error::last_os_error()
            );
        }
        crate::tmux::resize_window(&self.tmux_session, cols, rows);
    }

    /// Send SIGWINCH to the child's process group so TUIs redraw.
    pub fn sigwinch(&self) {
        let ret = unsafe { libc::kill(-self.child_pid, libc::SIGWINCH) };
        if ret < 0 {
            warn!(
                tab_id = %self.tab_id,
                "kill SIGWINCH failed: {}",
                std::io::Error::last_os_error()
            );
        }
    }

    /// Explicitly tear down: unregister the fd, kill the tmux session, and
    /// SIGHUP->SIGKILL the child process group. This is the ONLY path that
    /// kills the tmux session (plain drop preserves it).
    ///
    /// Note: `close()` does NOT close the master fd. The fd is owned by
    /// `master_fd: OwnedFd` and closed exactly once when the backend drops;
    /// closing it here would risk a double-close (the kernel can recycle the
    /// freed fd number into another thread's resource). `kill_session` -- not
    /// closing the master -- is what hangs up the pty and unblocks the reader.
    pub fn close(&self) {
        debug!(tab_id = %self.tab_id, child_pid = self.child_pid, "closing PTY");
        unregister_fd(&self.tab_id);
        crate::tmux::kill_session(&self.tmux_session);

        let pid = self.child_pid;
        unsafe {
            libc::kill(-pid, libc::SIGHUP);
        }
        // Give it a moment, then force-kill + reap on a detached thread.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            unsafe {
                let mut status = 0;
                let ret = libc::waitpid(pid, &mut status, libc::WNOHANG);
                if ret == 0 {
                    libc::kill(-pid, libc::SIGKILL);
                    libc::waitpid(pid, &mut status, 0);
                }
            }
        });
    }
}

impl Drop for PtyBackend {
    fn drop(&mut self) {
        // Preserve the tmux session (shutdown / crash path). SIGHUP the tmux
        // *client* pid so the session survives for restore.
        //
        // Unregister the fd FIRST so the drain thread can never write to it
        // after this point (a stale registry entry would be a recycle hazard:
        // the fd number is about to be freed when `master_fd` drops). The
        // master fd itself is closed exactly once -- automatically -- when the
        // `master_fd: OwnedFd` field drops at the end of this method. No manual
        // `libc::close` here; that was the old double-close bug.
        unregister_fd(&self.tab_id);
        let pid = self.child_pid;
        unsafe {
            libc::kill(pid, libc::SIGHUP);
            libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG);
        }
        debug!(tab_id = %self.tab_id, "PtyBackend dropped; tmux session preserved");
    }
}
