//! Per-tab PTY backend wired to the alacritty emulator.
//!
//! Each tab owns one [`PtyBackend`] (master fd + child pid + tmux session
//! name). The openpty + `tmux new-session -A` child-spawn logic is the same
//! one the legacy WebView path used; only the OUTPUT path changed: instead of
//! base64-encoding bytes into a bus event, the reader thread drives an
//! `alacritty_terminal` `Processor` straight into the tab's shared
//! `Term<Listener>` grid.
//!
//! Threading model (one reader + one writer thread per tab)
//! -------------------------------------------------------
//! - `App` state owns the `Emulator` (for the renderer + `resize`). The reader
//!   thread does NOT call `Emulator::advance` -- that would need `&mut` to
//!   state-owned data. Instead, at attach time we clone the term handle
//!   (`emulator.term()` -> `Arc<FairMutex<Term<Listener>>>`) and move that
//!   clone plus a FRESH `Processor` into the reader thread. The reader loop
//!   locks the term, advances bytes, and notifies iced.
//! - Keyboard / wheel / paste input and terminal replies (DSR / DA / …) are
//!   **never written from the iced UI thread**. Every write is enqueued on a
//!   per-tab `mpsc` and drained by a dedicated writer thread. That keeps the
//!   UI responsive when a mouse-tracking TUI fills the PTY input buffer
//!   (blocking `write(2)` would otherwise freeze tab switching).
//! - Terminal replies flow through the `Listener`'s process-wide channel as
//!   `(tab_id, bytes)`. The drain thread looks up the tab's write-queue sender
//!   in a global registry and enqueues — same path as keyboard input, so one
//!   writer serialises all bytes for that master fd.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::io::{IntoRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Instant;

use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
use tracing::{debug, warn};

use crate::emulator::Listener;
use crate::osc9999;
use crate::perf;

// -- Process-wide pty-write drain ----------------------------------------------
//
// The `Listener` inside every `Term` sends replies as `(tab_id, bytes)` on a
// single process-wide channel. One drain thread looks up the tab's write-queue
// sender in a global registry and enqueues. Registering/unregistering is done
// by `PtyBackend` at attach/close.

/// Global pty-write sender, cloned for every tab's `Listener`.
static PTY_WRITE_TX: OnceLock<mpsc::Sender<(String, Vec<u8>)>> = OnceLock::new();

/// Map of `tab_id -> write-queue sender`, populated at attach and cleared at close.
/// The drain thread reads it; `PtyBackend` mutates it.
static WRITE_REGISTRY: OnceLock<Mutex<HashMap<String, mpsc::Sender<Vec<u8>>>>> = OnceLock::new();

fn write_registry() -> &'static Mutex<HashMap<String, mpsc::Sender<Vec<u8>>>> {
    WRITE_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Initialise the pty-write channel + drain thread exactly once.
fn ensure_pty_write_drain() {
    PTY_WRITE_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<(String, Vec<u8>)>();
        std::thread::spawn(move || {
            // Blocking recv -- exits when every sender is dropped (process end).
            while let Ok((tab_id, bytes)) = rx.recv() {
                let sender = {
                    // Poison-tolerant: a panic elsewhere must not kill the one
                    // process-wide drain thread (that would hang every TUI).
                    let map = write_registry().lock().unwrap_or_else(|e| e.into_inner());
                    map.get(&tab_id).cloned()
                };
                match sender {
                    Some(tx) => {
                        // Enqueue only — the per-tab writer thread owns the fd.
                        // If the tab is mid-teardown the receiver may be gone;
                        // drop the reply and keep draining. Never panic.
                        if tx.send(bytes).is_err() {
                            debug!(
                                tab_id = %tab_id,
                                "pty-write drain: writer queue closed (tab gone)"
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

fn register_writer(tab_id: &str, tx: mpsc::Sender<Vec<u8>>) {
    write_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(tab_id.to_string(), tx);
}

fn unregister_writer(tab_id: &str) {
    write_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(tab_id);
    // Tab teardown: drop any tracked modifyOtherKeys state so a reused tab id
    // can't inherit it.
    crate::extkeys::clear(tab_id);
}

/// Write every byte in `buf` to `fd`, retrying short writes and EINTR.
///
/// Only called from the per-tab writer thread. May block when the PTY input
/// buffer is full — that is intentional; the iced UI enqueues and never waits.
fn write_all_blocking(fd: RawFd, mut buf: &[u8], tab_id: &str) {
    let total = buf.len();
    let t0 = Instant::now();
    while !buf.is_empty() {
        let n = unsafe { libc::write(fd, buf.as_ptr() as *const libc::c_void, buf.len()) };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == ErrorKind::Interrupted {
                continue;
            }
            // EAGAIN should not happen on a blocking fd; treat like any other
            // error and drop the rest of this chunk so one stuck write can't
            // hang the queue forever if flags were ever flipped.
            warn!(tab_id = %tab_id, "pty writer: write failed: {err}");
            perf::write_block(t0.elapsed(), total);
            return;
        }
        if n == 0 {
            warn!(tab_id = %tab_id, "pty writer: write returned 0");
            perf::write_block(t0.elapsed(), total);
            return;
        }
        buf = &buf[n as usize..];
    }
    perf::write_block(t0.elapsed(), total);
}

// -- PtyBackend -- per-tab handle ----------------------------------------------

/// Per-tab PTY handle: master fd + child pid + tmux session name + write queue.
///
/// Drop semantics intentionally match the legacy `PtyManager`: a plain drop
/// (shutdown / crash) closes the master fd (once, via `OwnedFd`), unregisters
/// it from the write registry, and SIGHUPs the tmux *client* but LEAVES THE
/// TMUX SESSION ALIVE so it can be restored. Only an explicit
/// [`close`](Self::close) tears down the tmux session.
pub struct PtyBackend {
    tab_id: String,
    /// The pty master. Owned exactly once here: dropping the `OwnedFd` is the
    /// single close of this fd (see `Drop`). `close()` must NOT close it.
    /// Reader and writer threads hold their own `dup`s.
    master_fd: OwnedFd,
    child_pid: i32,
    tmux_session: String,
    /// Input path for keyboard / wheel / paste. Sends into the writer thread;
    /// never blocks the iced UI on a full PTY buffer.
    write_tx: mpsc::Sender<Vec<u8>>,
}

impl PtyBackend {
    /// Open a pty, exec `tmux new-session -A -s <tmux_session>`, register the
    /// write queue for pty-write replies, and start the reader + writer threads.
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
        cursor: Arc<std::sync::RwLock<crate::emulator::CursorSnap>>,
        notify: mpsc::Sender<String>,
        exit: mpsc::Sender<String>,
    ) -> std::io::Result<Self> {
        Self::spawn_or_attach_with_env(
            tab_id,
            tmux_session,
            cols,
            rows,
            cwd,
            term,
            cursor,
            notify,
            exit,
            &[],
        )
    }

    /// Like [`Self::spawn_or_attach`], then stamps `env` onto the tmux
    /// session (`new-session -e` plus `set-environment` so reattach
    /// still inherits).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_or_attach_with_env(
        tab_id: &str,
        tmux_session: &str,
        cols: u16,
        rows: u16,
        cwd: Option<&str>,
        term: Arc<FairMutex<Term<Listener>>>,
        cursor: Arc<std::sync::RwLock<crate::emulator::CursorSnap>>,
        notify: mpsc::Sender<String>,
        exit: mpsc::Sender<String>,
        env: &[(&str, &str)],
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
        for (key, val) in env {
            cmd.args(["-e", &format!("{key}={val}")]);
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

        for (key, val) in env {
            crate::tmux::set_environment(tmux_session, key, val);
        }

        // Writer thread: owns a dup'd master fd + drains the per-tab queue.
        // Keyboard/wheel/paste and Listener replies all serialise here so the
        // UI never calls write(2) and concurrent writers never interleave
        // mid-sequence on the same fd.
        let (write_tx, write_rx) = mpsc::channel::<Vec<u8>>();
        // SAFETY: `libc::dup` returns a fresh, owned fd; the writer thread is
        // its sole owner and the OwnedFd closes it when the loop exits.
        let write_fd = unsafe { OwnedFd::from_raw_fd(libc::dup(master_fd.as_raw_fd())) };
        let writer_tab_id = tab_id.to_string();
        std::thread::spawn(move || {
            while let Ok(mut bytes) = write_rx.recv() {
                // Coalesce a burst of enqueued chunks (e.g. rapid wheel reports
                // or multi-key auto-repeat) into one write_all to cut syscalls
                // while a TUI is slow to drain.
                while let Ok(more) = write_rx.try_recv() {
                    bytes.extend(more);
                }
                write_all_blocking(write_fd.as_raw_fd(), &bytes, &writer_tab_id);
            }
            drop(write_fd);
            debug!(tab_id = %writer_tab_id, "PTY writer thread exited");
        });
        register_writer(tab_id, write_tx.clone());

        // Reader thread: own a dup'd fd + the term Arc + a FRESH Processor.
        // SAFETY: `libc::dup` returns a fresh, owned fd distinct from the
        // master; the reader thread is its sole owner and the OwnedFd closes it
        // exactly once when the loop exits.
        let read_fd = unsafe { OwnedFd::from_raw_fd(libc::dup(master_fd.as_raw_fd())) };
        let reader_tab_id = tab_id.to_string();
        std::thread::spawn(move || {
            let mut processor: Processor<StdSyncHandler> = Processor::new();
            // Observe the same byte stream for tmux's modifyOtherKeys
            // enable/disable (XTMODKEYS), which alacritty_terminal parses but
            // discards — `input` reads the tracked level to encode Shift+Enter
            // and friends as CSI-u. See `crate::extkeys`.
            let mut extkeys_scanner = crate::extkeys::Scanner::new();
            let mut osc_scanner = osc9999::OscScanner::new();
            // Larger read + pending buffer so a TUI full-repaint is advanced in
            // fewer lock acquisitions (alacritty-style batching).
            // 16 KiB reads (was 4 KiB) → fewer lock acquisitions per TUI repaint.
            // Unfair lock so the UI's fair snapshot isn't stuck behind us in the
            // waiter queue after we drop.
            let mut buf = [0u8; 16 * 1024];
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
                let raw = &buf[..n as usize];
                let (clean, osc_payloads) = osc_scanner.feed(raw);
                if let Some(tx) = osc9999::try_sender() {
                    for payload in osc_payloads {
                        let _ = tx.send((reader_tab_id.clone(), payload));
                    }
                }
                let chunk = clean.as_slice();
                if let Some(level) = extkeys_scanner.feed(chunk) {
                    crate::extkeys::set_level(&reader_tab_id, level);
                }
                let t0 = Instant::now();
                // Unfair: barge past a fair waiter. Publish cursor while we
                // still hold the lock so the UI never needs to lock just to
                // blink the caret.
                let mut guard = term.lock_unfair();
                processor.advance(&mut *guard, chunk);
                crate::emulator::publish_cursor(&*guard, &cursor);
                drop(guard);
                perf::reader_advance(t0.elapsed(), chunk.len());
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
            write_tx,
        })
    }

    /// Enqueue bytes for the master fd (keyboard / wheel / paste → shell).
    ///
    /// Returns immediately. The per-tab writer thread performs the actual
    /// `write(2)`, so a full PTY input buffer cannot stall the iced UI.
    pub fn write(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        perf::write_enqueue(bytes.len());
        if self.write_tx.send(bytes.to_vec()).is_err() {
            warn!(
                tab_id = %self.tab_id,
                "pty write queue closed (writer gone)"
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

    /// Explicitly tear down: unregister the write queue, kill the tmux session,
    /// and SIGHUP->SIGKILL the child process group. This is the ONLY path that
    /// kills the tmux session (plain drop preserves it).
    ///
    /// Note: `close()` does NOT close the master fd. The fd is owned by
    /// `master_fd: OwnedFd` and closed exactly once when the backend drops;
    /// closing it here would risk a double-close (the kernel can recycle the
    /// freed fd number into another thread's resource). `kill_session` -- not
    /// closing the master -- is what hangs up the pty and unblocks the reader.
    pub fn close(&self) {
        debug!(tab_id = %self.tab_id, child_pid = self.child_pid, "closing PTY");
        unregister_writer(&self.tab_id);
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
        // Unregister the writer FIRST so the drain thread can never enqueue to
        // a half-dead tab. Dropping `write_tx` then ends the writer thread
        // (after it drains any remaining chunks). The master fd itself is
        // closed exactly once -- automatically -- when the `master_fd: OwnedFd`
        // field drops at the end of this method.
        unregister_writer(&self.tab_id);
        let pid = self.child_pid;
        unsafe {
            libc::kill(pid, libc::SIGHUP);
            libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG);
        }
        debug!(tab_id = %self.tab_id, "PtyBackend dropped; tmux session preserved");
    }
}
