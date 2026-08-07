//! Install apply pipeline — dry-run for UI dogfood, real for live media.
//!
//! Real apply shells out to `sudo sola-install-apply` (image helper) which
//! partitions, runs `nixos-install` from a prebuilt system path, and writes
//! `/etc/sola/install-user` on the target.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

/// One step shown on the Installing screen.
#[derive(Debug, Clone)]
pub struct ProgressStep {
    pub label: &'static str,
    pub dwell: Duration,
}

/// Fixed policy applied after a real install (documented for the UI).
pub const HOSTNAME: &str = "sola";
pub const LOCALE: &str = "en_US.UTF-8";
pub const KEYBOARD: &str = "us (Mac)";
/// Interim fixed zone (America/Denver) until auto-detect lands.
pub const TIMEZONE: &str = "US/Mountain";

/// Path written into the installer image (see `nix/image/install-tools.nix`).
pub const INSTALL_SYSTEM_PATH: &str = "/etc/sola/install-system";

/// Progress labels for both dry-run and real apply (real maps script indices).
pub fn progress_labels() -> Vec<&'static str> {
    vec![
        "Preparing disk…",
        "Mounting…",
        "Writing system…",
        "Creating user…",
        "Installing bootloader…",
        "Finishing…",
    ]
}

/// Progress steps for the dry-run installer (timed simulation).
pub fn dry_run_steps(_username: &str, _disk_path: &str) -> Vec<ProgressStep> {
    progress_labels()
        .into_iter()
        .zip([700, 400, 1400, 600, 700, 400])
        .map(|(label, ms)| ProgressStep {
            label,
            dwell: Duration::from_millis(ms),
        })
        .collect()
}

/// True when the live image has a prebuilt system path for offline install.
pub fn real_apply_available() -> bool {
    install_system_path().is_some() && which_apply().is_some()
}

/// Read the prebuilt nixos system toplevel path from the image.
pub fn install_system_path() -> Option<PathBuf> {
    let raw = fs::read_to_string(INSTALL_SYSTEM_PATH).ok()?;
    let p = PathBuf::from(raw.trim());
    if p.exists() {
        Some(p)
    } else {
        tracing::warn!(path = %p.display(), "install-system path missing on disk");
        None
    }
}

fn which_apply() -> Option<PathBuf> {
    // Prefer PATH (image puts sola-install-apply on systemPackages).
    if let Ok(out) = Command::new("sh")
        .args(["-c", "command -v sola-install-apply"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return Some(PathBuf::from(s));
            }
        }
    }
    let candidates = [
        "/run/current-system/sw/bin/sola-install-apply",
        "/opt/sola/bin/sola-install-apply",
    ];
    candidates
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

/// Events from a running real apply.
#[derive(Debug, Clone)]
pub enum ApplyEvent {
    Progress { index: usize, label: String },
    Done,
    Failed(String),
}

/// Spawn privileged apply on a background thread; events on the channel.
pub fn start_real_apply(
    username: String,
    disk_path: String,
) -> Result<Receiver<ApplyEvent>, String> {
    let system = install_system_path().ok_or_else(|| {
        "No install system path (/etc/sola/install-system)".to_string()
    })?;
    let apply =
        which_apply().ok_or_else(|| "sola-install-apply not found".to_string())?;
    // Resolve symlinks so the path matches the sudoers store path exactly.
    let apply = apply
        .canonicalize()
        .unwrap_or(apply);

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        if let Err(e) = run_apply_process(&tx, &apply, &username, &disk_path, &system) {
            let _ = tx.send(ApplyEvent::Failed(e));
        }
    });
    Ok(rx)
}

fn run_apply_process(
    tx: &Sender<ApplyEvent>,
    apply: &Path,
    username: &str,
    disk: &str,
    system: &Path,
) -> Result<(), String> {
    // Ensure progress dir exists for the helper (also streams stdout).
    let _ = fs::create_dir_all("/run/sola");

    let mut cmd = Command::new("sudo");
    cmd.args([
        "-n",
        apply.to_str().unwrap_or("sola-install-apply"),
        "--disk",
        disk,
        "--username",
        username,
        "--system",
        system.to_str().unwrap_or(""),
    ]);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    tracing::info!(
        disk,
        username,
        system = %system.display(),
        apply = %apply.display(),
        "starting real apply"
    );

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn sudo apply: {e}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "no stdout from apply".to_string())?;
    let reader = BufReader::new(stdout);

    for line in reader.lines() {
        let line = line.map_err(|e| format!("read apply stdout: {e}"))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        tracing::info!(%line, "apply");
        if let Some(rest) = line.strip_prefix("PROGRESS ") {
            let mut parts = rest.splitn(2, char::is_whitespace);
            let idx = parts
                .next()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            let label = parts.next().unwrap_or("Working…").to_string();
            let _ = tx.send(ApplyEvent::Progress {
                index: idx,
                label,
            });
        } else if line == "DONE" {
            let status = child
                .wait()
                .map_err(|e| format!("wait apply: {e}"))?;
            if status.success() {
                let _ = tx.send(ApplyEvent::Done);
                return Ok(());
            }
            return Err(format!("apply exited {}", status));
        } else if let Some(msg) = line.strip_prefix("ERROR ") {
            let _ = child.wait();
            return Err(msg.to_string());
        }
    }

    let status = child
        .wait()
        .map_err(|e| format!("wait apply: {e}"))?;
    if status.success() {
        let _ = tx.send(ApplyEvent::Done);
        Ok(())
    } else {
        // Pull stderr for the UI.
        let err = child.stderr.as_mut().and_then(|s| {
            let mut buf = String::new();
            use std::io::Read;
            let _ = s.read_to_string(&mut buf);
            if buf.is_empty() {
                None
            } else {
                Some(buf)
            }
        });
        Err(err.unwrap_or_else(|| format!("apply failed: {status}")))
    }
}

/// Request reboot (product path after Done). Best-effort.
pub fn request_reboot() -> Result<(), String> {
    // Prefer the image helper (sudoers allowlist is exact-path).
    for bin in [
        "sola-install-reboot",
        "/run/current-system/sw/bin/sola-install-reboot",
    ] {
        let status = Command::new("sudo").args(["-n", bin]).status();
        if let Ok(st) = status {
            if st.success() {
                return Ok(());
            }
        }
    }
    let status = Command::new("sudo")
        .args(["-n", "systemctl", "reboot"])
        .status()
        .map_err(|e| format!("reboot: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("reboot failed: {status}"))
    }
}
