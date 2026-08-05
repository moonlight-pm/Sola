//! macOS pasteboard via `pbpaste` / `pbcopy` (v1 — no AppKit link required).
//!
//! Hard-capped so a hung pasteboard helper cannot stall the clip worker.

use std::io::{Read, Write};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tracing::{info, warn};

const CLI_TIMEOUT: Duration = Duration::from_millis(800);

fn preview(s: &str) -> String {
    let t: String = s.chars().take(48).collect();
    if s.chars().count() > 48 {
        format!("{t}…")
    } else {
        t
    }
}

fn wait_cli(mut child: Child, label: &str) -> Option<Output> {
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let deadline = Instant::now() + CLI_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(ref mut p) = stdout_pipe {
                    let _ = p.read_to_end(&mut stdout);
                }
                if let Some(ref mut p) = stderr_pipe {
                    let _ = p.read_to_end(&mut stderr);
                }
                return Some(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(15));
            }
            Ok(None) => {
                warn!(
                    label,
                    timeout_ms = CLI_TIMEOUT.as_millis() as u64,
                    "clip CLI hung — killing"
                );
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(e) => {
                warn!(%e, label, "clip child try_wait failed");
                let _ = child.kill();
                return None;
            }
        }
    }
}

pub fn read_text() -> Option<String> {
    let child = match Command::new("pbpaste")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            warn!(%e, "pbpaste spawn failed");
            return None;
        }
    };
    let out = match wait_cli(child, "pbpaste") {
        Some(o) => o,
        None => return None,
    };
    if !out.status.success() && out.stdout.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        info!(status = ?out.status, stderr = %err.trim(), "pbpaste empty/fail");
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).into_owned();
    if s.is_empty() {
        info!("pbpaste returned empty");
        None
    } else {
        info!(bytes = s.len(), preview = %preview(&s), "clip read via pbpaste");
        Some(s)
    }
}

pub fn write_text(text: &str) -> bool {
    let mut child = match Command::new("pbcopy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            warn!(%e, "pbcopy spawn failed");
            return false;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(text.as_bytes()) {
            warn!(%e, "pbcopy write failed");
            let _ = child.kill();
            return false;
        }
    }
    match wait_cli(child, "pbcopy") {
        Some(out) if out.status.success() => {
            info!(
                bytes = text.len(),
                preview = %preview(text),
                "clip write via pbcopy"
            );
            true
        }
        Some(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            warn!(status = ?out.status, stderr = %err.trim(), "pbcopy failed");
            false
        }
        None => false,
    }
}

pub fn clear() -> bool {
    write_text("")
}
