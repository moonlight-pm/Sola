use std::collections::HashMap;
use std::collections::VecDeque;
use std::os::unix::io::IntoRawFd;
use std::os::unix::process::CommandExt;
use std::sync::{Arc, Mutex};

use base64::Engine;
use tracing::{debug, warn};

const OUTPUT_BUFFER_CAP: usize = 65536; // 64KB

/// Events emitted by PTY reader threads.
#[derive(Debug)]
pub enum PtyEvent {
    Data { pty_id: String, data: Vec<u8> },
    Scrollback { pty_id: String, data: Vec<u8> },
    Exit { pty_id: String },
}

struct OutputBuffer {
    buf: VecDeque<u8>,
}

impl OutputBuffer {
    fn new() -> Self {
        Self {
            buf: VecDeque::with_capacity(OUTPUT_BUFFER_CAP),
        }
    }

    fn push(&mut self, data: &[u8]) {
        self.buf.extend(data);
        if self.buf.len() > OUTPUT_BUFFER_CAP {
            let excess = self.buf.len() - OUTPUT_BUFFER_CAP;
            self.buf.drain(..excess);
        }
    }

    fn snapshot(&self) -> Vec<u8> {
        let (a, b) = self.buf.as_slices();
        let mut v = Vec::with_capacity(a.len() + b.len());
        v.extend_from_slice(a);
        v.extend_from_slice(b);
        v
    }
}

/// Manages PTY instances for terminal emulation.
///
/// Background reader threads push output via `tokio::sync::mpsc::UnboundedSender<PtyEvent>`.
pub struct PtyManager {
    ptys: HashMap<String, PtyInstance>,
}

pub struct PtyInstance {
    pub master_fd: i32,
    pub child_pid: u32,
    output_buffer: Arc<Mutex<OutputBuffer>>,
    pub tmux_session: Option<String>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            ptys: HashMap::new(),
        }
    }

    /// Spawn a new PTY running a tmux session.
    ///
    /// Returns the tmux session name on success. Starts a background thread
    /// that reads PTY output and sends `PtyEvent`s via the provided channel.
    /// If `tmux_session` is `Some`, reattaches to an existing session;
    /// otherwise creates a new session named after the PTY id.
    pub fn spawn_pty(
        &mut self,
        id: String,
        cols: u16,
        rows: u16,
        tmux_session: Option<String>,
        cwd: Option<String>,
        event_tx: tokio::sync::mpsc::UnboundedSender<PtyEvent>,
    ) -> Result<String, String> {
        let winsize = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        let pty =
            nix::pty::openpty(Some(&winsize), None).map_err(|e| format!("openpty failed: {e}"))?;

        let slave_fd = pty.slave.into_raw_fd();
        let is_reattach = tmux_session.is_some();
        let tmux_session_name = tmux_session.unwrap_or_else(|| crate::tmux::session_name(&id));

        let mut cmd = crate::tmux::tmux_cmd();
        cmd.args([
            "new-session",
            "-A",
            "-s",
            &tmux_session_name,
            "-x",
            &cols.to_string(),
            "-y",
            &rows.to_string(),
        ]);
        // Set start directory for new sessions (not reattach)
        if !is_reattach {
            if let Some(ref dir) = cwd {
                cmd.args(["-c", dir]);
            }
        }
        // SAFETY: pre_exec runs in the child process between fork and exec.
        // The libc calls here are async-signal-safe.
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
                .map_err(|e| format!("failed to spawn tmux: {e}"))?
        };

        // Close slave fd in parent -- child has its own copies
        unsafe { libc::close(slave_fd) };

        let master_fd = pty.master.into_raw_fd();
        let child_pid = child.id();

        debug!(
            "spawned PTY {id}: tmux={tmux_session_name}, pid={child_pid}, master_fd={master_fd}, reattach={is_reattach}"
        );

        let output_buffer = Arc::new(Mutex::new(OutputBuffer::new()));

        self.ptys.insert(
            id.clone(),
            PtyInstance {
                master_fd,
                child_pid,
                output_buffer: output_buffer.clone(),
                tmux_session: Some(tmux_session_name.clone()),
            },
        );

        // Capture scrollback BEFORE starting the reader thread so it's
        // guaranteed to be emitted to the frontend first (no race).
        if is_reattach {
            match crate::tmux::capture_scrollback(&tmux_session_name) {
                Ok(text) if !text.trim().is_empty() => {
                    let _ = event_tx.send(PtyEvent::Scrollback {
                        pty_id: id.clone(),
                        data: text.into_bytes(),
                    });
                }
                Ok(_) => {}
                Err(e) => {
                    warn!("Failed to capture scrollback: {e}");
                }
            }
        }

        // Spawn background reader thread
        let read_fd = unsafe { libc::dup(master_fd) };
        let pty_id = id.clone();
        let buffer_clone = output_buffer.clone();

        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                let n = unsafe {
                    libc::read(read_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
                };
                if n <= 0 {
                    break;
                }

                let chunk = &buf[..n as usize];

                match buffer_clone.lock() {
                    Ok(mut buf_lock) => buf_lock.push(chunk),
                    Err(e) => warn!("output buffer lock poisoned: {e}"),
                }

                let _ = event_tx.send(PtyEvent::Data {
                    pty_id: pty_id.clone(),
                    data: chunk.to_vec(),
                });
            }
            unsafe { libc::close(read_fd) };
            debug!("PTY reader thread exited for {pty_id}");

            // Notify that the shell exited
            let _ = event_tx.send(PtyEvent::Exit {
                pty_id: pty_id.clone(),
            });
        });

        Ok(tmux_session_name)
    }

    /// Write data to a PTY's master fd.
    pub fn write_pty(&self, id: &str, data: &[u8]) -> Result<(), String> {
        let instance = self.ptys.get(id).ok_or_else(|| format!("no PTY: {id}"))?;
        let n = unsafe {
            libc::write(
                instance.master_fd,
                data.as_ptr() as *const libc::c_void,
                data.len(),
            )
        };
        if n < 0 {
            Err(format!("write failed: {}", std::io::Error::last_os_error()))
        } else {
            Ok(())
        }
    }

    /// Resize a PTY.
    pub fn resize_pty(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let instance = self.ptys.get(id).ok_or_else(|| format!("no PTY: {id}"))?;
        let winsize = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let ret = unsafe {
            libc::ioctl(
                instance.master_fd,
                libc::TIOCSWINSZ,
                &winsize as *const libc::winsize,
            )
        };
        if ret < 0 {
            Err(format!(
                "ioctl TIOCSWINSZ failed: {}",
                std::io::Error::last_os_error()
            ))
        } else {
            // Also resize the tmux window so its internal geometry matches
            if let Some(ref session) = instance.tmux_session {
                crate::tmux::resize_window(session, cols, rows);
            }
            Ok(())
        }
    }

    /// Return a base64-encoded snapshot of the pty's output buffer.
    pub fn reconnect_pty(&self, id: &str) -> Result<String, String> {
        let instance = self.ptys.get(id).ok_or_else(|| format!("no PTY: {id}"))?;
        let snapshot = instance
            .output_buffer
            .lock()
            .map_err(|e| format!("buffer lock failed: {e}"))?
            .snapshot();
        Ok(base64::engine::general_purpose::STANDARD.encode(&snapshot))
    }

    /// Send SIGWINCH to the pty's process group, causing TUI apps to redraw.
    pub fn sigwinch_pty(&self, id: &str) -> Result<(), String> {
        let instance = self.ptys.get(id).ok_or_else(|| format!("no PTY: {id}"))?;
        let pid = instance.child_pid as i32;
        let ret = unsafe { libc::kill(-pid, libc::SIGWINCH) };
        if ret < 0 {
            Err(format!(
                "kill SIGWINCH failed: {}",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(())
        }
    }

    /// Get the tmux session name for a PTY, if it has one.
    pub fn tmux_session(&self, id: &str) -> Option<String> {
        self.ptys.get(id).and_then(|inst| inst.tmux_session.clone())
    }

    /// Close a PTY: close the master fd and kill the child process.
    pub fn close_pty(&mut self, id: &str) -> Result<(), String> {
        let instance = self
            .ptys
            .remove(id)
            .ok_or_else(|| format!("no PTY: {id}"))?;

        debug!("closing PTY {id}: pid={}", instance.child_pid);

        unsafe { libc::close(instance.master_fd) };

        // Kill the tmux session
        if let Some(ref session) = instance.tmux_session {
            crate::tmux::kill_session(session);
        }

        // Send SIGHUP then SIGKILL to the process group
        let pid = instance.child_pid as i32;
        unsafe {
            libc::kill(-pid, libc::SIGHUP);
        }

        // Give it a moment, then force-kill if needed
        let pid_owned = pid;
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            unsafe {
                // Reap or force-kill
                let mut status = 0;
                let ret = libc::waitpid(pid_owned, &mut status, libc::WNOHANG);
                if ret == 0 {
                    // Still alive
                    libc::kill(-pid_owned, libc::SIGKILL);
                    libc::waitpid(pid_owned, &mut status, 0);
                }
            }
        });

        Ok(())
    }
}

impl Drop for PtyManager {
    fn drop(&mut self) {
        // Always preserve tmux sessions on drop. The terminal persists state
        // to disk continuously, so sessions can always be restored. If a tab
        // was explicitly closed by the user, close_pty already killed its
        // tmux session. The only time Drop runs with live sessions is on
        // shutdown or crash -- in both cases we want them to survive.
        let ids: Vec<String> = self.ptys.keys().cloned().collect();
        for id in ids {
            if let Some(instance) = self.ptys.remove(&id) {
                unsafe { libc::close(instance.master_fd) };
                let pid = instance.child_pid as i32;
                unsafe {
                    libc::kill(pid, libc::SIGHUP);
                    libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG);
                }
                debug!("preserving tmux session for PTY {id}, killed client pid={pid}");
            }
        }
    }
}
