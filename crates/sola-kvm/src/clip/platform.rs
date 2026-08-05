//! Local clipboard read/write (novus Linux).
//!
//! Order: `wl-copy`/`wl-paste` (most reliable under River) → `arboard`.
//! All CLI helpers are hard-capped so a hung compositor clipboard cannot
//! stall the clip worker (which would block Acks and kill the TCP peer).

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use tracing::{info, warn};

static WL_COPY: OnceLock<Option<PathBuf>> = OnceLock::new();
static WL_PASTE: OnceLock<Option<PathBuf>> = OnceLock::new();
/// Bumped on every write so abandoned helper threads don't clobber a newer offer.
static WRITE_GEN: AtomicU64 = AtomicU64::new(0);

/// Hard cap for any clipboard helper (read or write).
/// Leave-time compositor contention often makes `wl-copy` take 1–3s; budget
/// enough room for success while still bounding a hard hang.
const CLI_TIMEOUT: Duration = Duration::from_millis(5000);

/// One-shot probe at clip worker start — logs what works.
pub fn probe_and_log() {
    let wd = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "(unset)".into());
    let xdg = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "(unset)".into());
    info!(
        wayland_display = %wd,
        xdg_runtime_dir = %xdg,
        "clip platform probe"
    );

    let copy = resolve_wl("wl-copy");
    let paste = resolve_wl("wl-paste");
    info!(
        wl_copy = ?copy.as_ref().map(|p| p.display().to_string()),
        wl_paste = ?paste.as_ref().map(|p| p.display().to_string()),
        "clip CLI tools"
    );

    if arboard_safe() {
        match arboard::Clipboard::new() {
            Ok(mut c) => match c.get_text() {
                Ok(t) => info!(
                    bytes = t.len(),
                    preview = %preview(&t),
                    "arboard available (get_text ok)"
                ),
                Err(e) => info!(%e, "arboard open ok but get_text failed"),
            },
            Err(e) => info!(%e, "arboard Clipboard::new failed"),
        }
    } else {
        info!("arboard skipped (Wayland session — X11 path would hang)");
    }

    if let Some(t) = read_text() {
        info!(
            bytes = t.len(),
            preview = %preview(&t),
            "clip read_text probe ok"
        );
    } else {
        warn!("clip read_text probe returned empty/unavailable");
    }
}

fn preview(s: &str) -> String {
    let t: String = s.chars().take(48).collect();
    if s.chars().count() > 48 {
        format!("{t}…")
    } else {
        t
    }
}

fn resolve_wl(name: &str) -> Option<PathBuf> {
    let cell = if name == "wl-copy" { &WL_COPY } else { &WL_PASTE };
    cell.get_or_init(|| find_bin(name)).clone()
}

fn find_bin(name: &str) -> Option<PathBuf> {
    if let Some(p) = which(name) {
        return Some(p);
    }
    let fixed = [
        format!("/run/current-system/sw/bin/{name}"),
        format!("/usr/bin/{name}"),
    ];
    for f in fixed {
        let p = PathBuf::from(&f);
        if p.is_file() {
            return Some(p);
        }
    }
    // Nix store: wl-clipboard package (cheap glob via std::fs if dir readable).
    if let Ok(rd) = std::fs::read_dir("/nix/store") {
        for ent in rd.flatten() {
            let n = ent.file_name();
            let n = n.to_string_lossy();
            if n.contains("wl-clipboard") && !n.contains("dev") {
                let p = ent.path().join("bin").join(name);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Wait for a child with a hard deadline; kill on timeout.
///
/// Uses `try_wait` polling instead of blocking `wait()` so a hung compositor
/// cannot pin the clip worker forever (that was killing Mac→Linux clipboard).
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

/// arboard’s Linux backend prefers X11 and can block for a long X connection
/// timeout under pure Wayland — never call it when WAYLAND_DISPLAY is set.
fn arboard_safe() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_none()
}

/// Run a clipboard op on a helper thread with a hard wall-clock budget.
/// Even if `wl-copy`/`wl-paste` ignore signals, the clip worker stays live.
fn with_cli_budget<T: Send + 'static>(label: &'static str, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(CLI_TIMEOUT + Duration::from_millis(200)) {
        Ok(v) => Some(v),
        Err(_) => {
            warn!(
                label,
                timeout_ms = (CLI_TIMEOUT + Duration::from_millis(200)).as_millis() as u64,
                "clip platform op exceeded budget — abandoning (helper may still run)"
            );
            None
        }
    }
}

/// Read clipboard as UTF-8 text. `None` if empty or unavailable.
pub fn read_text() -> Option<String> {
    if let Some(Some(s)) = with_cli_budget("read_text", read_text_inner) {
        return Some(s);
    }
    None
}

fn read_text_inner() -> Option<String> {
    if let Some(s) = read_text_cli() {
        return Some(s);
    }
    if !arboard_safe() {
        warn!("clip read: wl-paste failed and arboard skipped (Wayland)");
        return None;
    }
    match arboard::Clipboard::new() {
        Ok(mut c) => match c.get_text() {
            Ok(s) if !s.is_empty() => {
                info!(
                    bytes = s.len(),
                    preview = %preview(&s),
                    "clip read via arboard"
                );
                Some(s)
            }
            Ok(_) => {
                info!("clip read arboard empty");
                None
            }
            Err(e) => {
                warn!(%e, "clip read arboard get_text failed");
                None
            }
        },
        Err(e) => {
            warn!(%e, "clip read: no wl-paste and arboard open failed");
            None
        }
    }
}

/// Write UTF-8 text to the clipboard.
pub fn write_text(text: &str) -> bool {
    let text = text.to_string();
    let epoch = WRITE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    with_cli_budget("write_text", move || write_text_inner(&text, epoch)).unwrap_or(false)
}

fn write_text_inner(text: &str, epoch: u64) -> bool {
    if write_text_cli(text, epoch) {
        return true;
    }
    // Stale: a newer write started while we were working.
    if WRITE_GEN.load(Ordering::SeqCst) != epoch {
        info!(epoch, "clip write abandoned (superseded)");
        return false;
    }
    if !arboard_safe() {
        warn!(
            bytes = text.len(),
            "clip write failed — wl-copy failed; arboard skipped (Wayland)"
        );
        return false;
    }
    match arboard::Clipboard::new() {
        Ok(mut c) => match c.set_text(text.to_string()) {
            Ok(()) => {
                if WRITE_GEN.load(Ordering::SeqCst) != epoch {
                    info!(epoch, "clip arboard write discarded (superseded)");
                    return false;
                }
                info!(
                    bytes = text.len(),
                    preview = %preview(text),
                    "clip write via arboard"
                );
                true
            }
            Err(e) => {
                warn!(%e, bytes = text.len(), "clip write arboard set_text failed");
                false
            }
        },
        Err(e) => {
            warn!(
                %e,
                bytes = text.len(),
                "clip write failed — no wl-copy and arboard open failed"
            );
            false
        }
    }
}

/// Clear clipboard (best-effort).
pub fn clear() -> bool {
    write_text("")
}

fn read_text_cli() -> Option<String> {
    let bin = resolve_wl("wl-paste")?;
    let mut cmd = Command::new(timeout_bin());
    cmd.args([
        "--signal=KILL",
        "5",
        bin.to_str().unwrap_or("wl-paste"),
        "-n",
        "-t",
        "text",
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    pass_wayland_env(&mut cmd);
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            warn!(%e, "wl-paste spawn failed");
            return None;
        }
    };
    let out = wait_cli(child, "wl-paste")?;
    if !out.status.success() && out.stdout.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        info!(
            status = ?out.status,
            stderr = %err.trim(),
            "wl-paste empty/fail"
        );
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).into_owned();
    if s.is_empty() {
        info!("wl-paste returned empty stdout");
        None
    } else {
        info!(
            bytes = s.len(),
            preview = %preview(&s),
            "clip read via wl-paste"
        );
        Some(s)
    }
}

fn timeout_bin() -> PathBuf {
    for p in [
        "/run/current-system/sw/bin/timeout",
        "/usr/bin/timeout",
        "timeout",
    ] {
        let pb = PathBuf::from(p);
        if p == "timeout" || pb.is_file() {
            return pb;
        }
    }
    PathBuf::from("timeout")
}

fn write_text_cli(text: &str, epoch: u64) -> bool {
    let Some(bin) = resolve_wl("wl-copy") else {
        return false;
    };
    if WRITE_GEN.load(Ordering::SeqCst) != epoch {
        info!(epoch, "clip write_cli skipped (superseded before start)");
        return false;
    }
    // A stuck previous `wl-copy` data-source process can block new offers for
    // tens of seconds. Clear first (fast) then set the new payload.
    {
        let mut clear = Command::new(timeout_bin());
        clear
            .args([
                "--signal=KILL",
                "1",
                bin.to_str().unwrap_or("wl-copy"),
                "--clear",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        pass_wayland_env(&mut clear);
        if let Ok(child) = clear.spawn() {
            let _ = wait_cli(child, "wl-copy --clear");
        }
    }
    if WRITE_GEN.load(Ordering::SeqCst) != epoch {
        info!(epoch, "clip write_cli skipped (superseded after clear)");
        return false;
    }

    // Default wl-copy forks a background data source and exits the parent.
    // Do NOT use --foreground (waits for a paste consumer).
    let mut cmd = Command::new(timeout_bin());
    cmd.args([
        "--signal=KILL",
        "5",
        bin.to_str().unwrap_or("wl-copy"),
        "-t",
        "text/plain;charset=utf-8",
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::piped());
    pass_wayland_env(&mut cmd);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            warn!(%e, path = %bin.display(), "wl-copy (via timeout) spawn failed");
            return false;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(text.as_bytes()) {
            warn!(%e, "wl-copy stdin write failed");
            let _ = child.kill();
            return false;
        }
        // Drop stdin so wl-copy sees EOF and the parent can exit.
    }
    match wait_cli(child, "wl-copy") {
        Some(out) if out.status.success() => {
            if WRITE_GEN.load(Ordering::SeqCst) != epoch {
                info!(epoch, "clip write_cli discarded (superseded after wl-copy)");
                return false;
            }
            info!(
                bytes = text.len(),
                preview = %preview(text),
                "clip write via wl-copy"
            );
            true
        }
        Some(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            warn!(
                status = ?out.status,
                stderr = %err.trim(),
                "wl-copy failed"
            );
            false
        }
        None => false,
    }
}

fn pass_wayland_env(cmd: &mut Command) {
    if let Ok(v) = std::env::var("WAYLAND_DISPLAY") {
        cmd.env("WAYLAND_DISPLAY", v);
    }
    if let Ok(v) = std::env::var("XDG_RUNTIME_DIR") {
        cmd.env("XDG_RUNTIME_DIR", v);
    }
    // Some compositors need this for data-device.
    if let Ok(v) = std::env::var("XDG_CURRENT_DESKTOP") {
        cmd.env("XDG_CURRENT_DESKTOP", v);
    }
}
