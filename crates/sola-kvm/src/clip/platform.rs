//! Local clipboard read/write (novus Linux).
//!
//! Order: `wl-copy`/`wl-paste` (most reliable under River) → `arboard`.
//! All CLI helpers are hard-capped so a hung compositor clipboard cannot
//! stall the clip worker (which would block Acks and kill the TCP peer).
//!
//! **Stuck helpers:** `wl-copy` forks a background data-source process. If we
//! abandon a write mid-flight (or the compositor wedges the offer), that
//! daemon can linger and block future offers. We:
//! 1. Put each CLI spawn in its own process group and SIGKILL the whole group
//! 2. Use timeouts that fit **inside** the wall-clock budget (clear + write)
//! 3. Reap leftover `wl-copy` processes after a timed-out op before the next write

use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};
use wl_clipboard_rs::copy::{self, MimeType as CopyMime, Source};
use wl_clipboard_rs::paste::{
    self, ClipboardType, Error as PasteError, MimeType as PasteMime, Seat,
};

use super::proto::{MIME_PNG, MIME_TEXT_UTF8, hash_bytes};

/// What we will put on CLIP1. Prefer PNG when the compositor offers it.
#[derive(Debug, Clone)]
pub enum LocalClip {
    Empty,
    Text(String),
    Png(Vec<u8>),
}

impl LocalClip {
    pub fn mime(&self) -> u8 {
        match self {
            Self::Png(_) => MIME_PNG,
            _ => MIME_TEXT_UTF8,
        }
    }

    pub fn hash(&self) -> u32 {
        match self {
            Self::Empty => hash_bytes(b""),
            Self::Text(s) => hash_bytes(s.as_bytes()),
            Self::Png(b) => hash_bytes(b),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Text(s) => s.len(),
            Self::Png(b) => b.len(),
        }
    }

    pub fn body(&self) -> &[u8] {
        match self {
            Self::Empty => b"",
            Self::Text(s) => s.as_bytes(),
            Self::Png(b) => b,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

static WL_COPY: OnceLock<Option<PathBuf>> = OnceLock::new();
static WL_PASTE: OnceLock<Option<PathBuf>> = OnceLock::new();
/// Bumped on every write so abandoned helper threads don't clobber a newer offer.
static WRITE_GEN: AtomicU64 = AtomicU64::new(0);
/// Set when a platform op is abandoned mid-flight — next write reaps orphans.
static NEED_REAP: AtomicBool = AtomicBool::new(false);

/// Hard cap for a single clipboard **write** (after clear).
const WRITE_TIMEOUT: Duration = Duration::from_millis(4000);
/// Cap for `wl-copy --clear` (should be fast; don't burn the whole budget).
const CLEAR_TIMEOUT: Duration = Duration::from_millis(800);
/// Cap for clipboard **read** (`wl-paste`).
const READ_TIMEOUT: Duration = Duration::from_millis(4000);
/// Outer wall-clock budget for the whole op (clear + write + margin).
const OP_BUDGET: Duration = Duration::from_millis(5500);

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
    let cell = if name == "wl-copy" {
        &WL_COPY
    } else {
        &WL_PASTE
    };
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

/// Spawn `cmd` in a **new process group** so we can SIGKILL the whole tree
/// (timeout → wl-copy → any double-fork that stayed in the group).
fn spawn_group(mut cmd: Command) -> std::io::Result<Child> {
    // SAFETY: setpgid(0,0) only affects the child right after fork, before exec.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    cmd.spawn()
}

/// SIGKILL the process group headed by `child`, then reap.
fn kill_group(child: &mut Child, label: &str) {
    let pid = child.id() as i32;
    if pid > 1 {
        // Negative pid = process group.
        // SAFETY: kill(-pgid) is the standard group-kill interface.
        let rc = unsafe { libc::kill(-pid, libc::SIGKILL) };
        if rc != 0 {
            let e = std::io::Error::last_os_error();
            // ESRCH = already gone — fine.
            if e.raw_os_error() != Some(libc::ESRCH) {
                warn!(%e, label, pid, "clip kill process group failed; trying child only");
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Wait for a child with a hard deadline; kill the **process group** on timeout.
fn wait_cli(mut child: Child, label: &str, limit: Duration) -> Option<Output> {
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let deadline = Instant::now() + limit;
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
                    timeout_ms = limit.as_millis() as u64,
                    "clip CLI hung — killing process group"
                );
                kill_group(&mut child, label);
                NEED_REAP.store(true, Ordering::SeqCst);
                return None;
            }
            Err(e) => {
                warn!(%e, label, "clip child try_wait failed");
                kill_group(&mut child, label);
                NEED_REAP.store(true, Ordering::SeqCst);
                return None;
            }
        }
    }
}

/// Reap leftover `wl-copy` data-source processes for this user.
///
/// Safe on the desk path: sola-kvm is the only long-lived auto-clipboard
/// client; a wedged offer blocks all subsequent sync until cleared.
fn reap_orphaned_wl_copy() {
    if !NEED_REAP.swap(false, Ordering::SeqCst) {
        return;
    }
    let uid = unsafe { libc::getuid() };
    // pkill -x matches exact process name; -u scopes to our uid.
    let status = Command::new("pkill")
        .args(["-u", &uid.to_string(), "-x", "wl-copy"])
        .status();
    match status {
        Ok(s) if s.success() => info!("clip reaped orphaned wl-copy after prior hang"),
        Ok(s) if s.code() == Some(1) => {
            // No matches — fine.
        }
        Ok(s) => warn!(?s, "clip pkill wl-copy unexpected status"),
        Err(e) => warn!(%e, "clip pkill wl-copy failed"),
    }
}

/// arboard’s Linux backend prefers X11 and can block for a long X connection
/// timeout under pure Wayland — never call it when WAYLAND_DISPLAY is set.
fn arboard_safe() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_none()
}

/// Run a clipboard op on a helper thread with a hard wall-clock budget.
/// Even if `wl-copy`/`wl-paste` ignore signals, the clip worker stays live.
fn with_cli_budget<T: Send + 'static>(
    label: &'static str,
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(OP_BUDGET) {
        Ok(v) => Some(v),
        Err(_) => {
            NEED_REAP.store(true, Ordering::SeqCst);
            warn!(
                label,
                timeout_ms = OP_BUDGET.as_millis() as u64,
                "clip platform op exceeded budget — reaping orphans on next write"
            );
            // Best-effort reap now (helper may still hold a wedged wl-copy).
            reap_orphaned_wl_copy();
            // Flag again so the next write also reaps (this call cleared NEED_REAP).
            NEED_REAP.store(true, Ordering::SeqCst);
            None
        }
    }
}

/// Read compositor clipboard. Prefers `image/png` (screenshots) then text.
pub fn read_local() -> LocalClip {
    if let Some(clip) = with_cli_budget("read_local", read_local_inner) {
        return clip;
    }
    LocalClip::Empty
}

fn read_local_inner() -> LocalClip {
    if let Some(clip) = read_via_data_control() {
        return clip;
    }
    match read_text_inner() {
        Some(s) if !s.is_empty() => LocalClip::Text(s),
        _ => LocalClip::Empty,
    }
}

fn read_via_data_control() -> Option<LocalClip> {
    let types = paste::get_mime_types(ClipboardType::Regular, Seat::Unspecified).ok()?;
    let offered: Vec<String> = types.iter().map(|s| s.to_ascii_lowercase()).collect();
    if offered.iter().any(|m| m == "image/png") {
        if let Some(bytes) = read_mime_bytes("image/png") {
            if is_png(&bytes) {
                info!(bytes = bytes.len(), "clip read image/png via data-control");
                return Some(LocalClip::Png(bytes));
            }
        }
    }
    if let Some(bytes) = read_mime_bytes("text/plain;charset=utf-8")
        .or_else(|| read_mime_bytes("text/plain"))
        .or_else(|| read_paste_text())
    {
        let s = String::from_utf8_lossy(&bytes).into_owned();
        if !s.is_empty() {
            info!(bytes = s.len(), "clip read text via data-control");
            return Some(LocalClip::Text(s));
        }
    }
    None
}

fn read_mime_bytes(mime: &str) -> Option<Vec<u8>> {
    match paste::get_contents(
        ClipboardType::Regular,
        Seat::Unspecified,
        PasteMime::Specific(mime),
    ) {
        Ok((mut pipe, _)) => {
            let mut buf = Vec::new();
            pipe.read_to_end(&mut buf).ok()?;
            if buf.is_empty() { None } else { Some(buf) }
        }
        Err(PasteError::ClipboardEmpty | PasteError::NoMimeType) => None,
        Err(e) => {
            debug!(%e, mime, "clip data-control read failed");
            None
        }
    }
}

fn read_paste_text() -> Option<Vec<u8>> {
    match paste::get_contents(ClipboardType::Regular, Seat::Unspecified, PasteMime::Text) {
        Ok((mut pipe, _)) => {
            let mut buf = Vec::new();
            pipe.read_to_end(&mut buf).ok()?;
            if buf.is_empty() { None } else { Some(buf) }
        }
        Err(_) => None,
    }
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
}

/// Write CLIP1 payload onto the compositor clipboard.
pub fn write_local(clip: &LocalClip) -> bool {
    match clip {
        LocalClip::Empty => clear(),
        LocalClip::Text(s) => write_text(s),
        LocalClip::Png(b) => write_png(b),
    }
}

fn write_png(bytes: &[u8]) -> bool {
    let bytes = bytes.to_vec();
    let epoch = WRITE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    with_cli_budget("write_png", move || write_png_inner(&bytes, epoch)).unwrap_or(false)
}

fn write_png_inner(bytes: &[u8], epoch: u64) -> bool {
    if WRITE_GEN.load(Ordering::SeqCst) != epoch {
        return false;
    }
    let opts = copy::Options::new();
    match opts.copy(
        Source::Bytes(bytes.to_vec().into_boxed_slice()),
        CopyMime::Specific("image/png".into()),
    ) {
        Ok(()) => {
            info!(bytes = bytes.len(), "clip write image/png via data-control");
            true
        }
        Err(e) => {
            warn!(%e, bytes = bytes.len(), "clip write image/png failed");
            false
        }
    }
}

fn write_text_data_control(text: &str) -> bool {
    let opts = copy::Options::new();
    match opts.copy(
        Source::Bytes(text.as_bytes().to_vec().into_boxed_slice()),
        CopyMime::Text,
    ) {
        Ok(()) => {
            info!(bytes = text.len(), "clip write text via data-control");
            true
        }
        Err(e) => {
            debug!(%e, "clip data-control text write failed");
            false
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
    if write_text_data_control(text) {
        return true;
    }
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
    let secs = read_timeout_secs();
    let mut cmd = Command::new(timeout_bin());
    cmd.args([
        "--signal=KILL",
        &secs,
        bin.to_str().unwrap_or("wl-paste"),
        "-n",
        "-t",
        "text",
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    pass_wayland_env(&mut cmd);
    let child = match spawn_group(cmd) {
        Ok(c) => c,
        Err(e) => {
            warn!(%e, "wl-paste spawn failed");
            return None;
        }
    };
    let out = wait_cli(child, "wl-paste", READ_TIMEOUT)?;
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

fn secs_ceil(d: Duration) -> String {
    // timeout(1) wants whole seconds; ceil so 800ms → 1, 4001ms → 5.
    let s = d.as_millis().div_ceil(1000).max(1);
    s.to_string()
}

fn read_timeout_secs() -> String {
    secs_ceil(READ_TIMEOUT)
}

fn write_timeout_secs() -> String {
    secs_ceil(WRITE_TIMEOUT)
}

fn clear_timeout_secs() -> String {
    secs_ceil(CLEAR_TIMEOUT)
}

fn write_text_cli(text: &str, epoch: u64) -> bool {
    let Some(bin) = resolve_wl("wl-copy") else {
        return false;
    };
    if WRITE_GEN.load(Ordering::SeqCst) != epoch {
        info!(epoch, "clip write_cli skipped (superseded before start)");
        return false;
    }

    // If a prior op timed out, kill any leftover data-source daemons first.
    reap_orphaned_wl_copy();

    // A stuck previous `wl-copy` data-source process can block new offers for
    // tens of seconds. Clear first (fast budget) then set the new payload.
    {
        let mut clear = Command::new(timeout_bin());
        clear
            .args([
                "--signal=KILL",
                &clear_timeout_secs(),
                bin.to_str().unwrap_or("wl-copy"),
                "--clear",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        pass_wayland_env(&mut clear);
        if let Ok(child) = spawn_group(clear) {
            let _ = wait_cli(child, "wl-copy --clear", CLEAR_TIMEOUT);
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
        &write_timeout_secs(),
        bin.to_str().unwrap_or("wl-copy"),
        "-t",
        "text/plain;charset=utf-8",
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::piped());
    pass_wayland_env(&mut cmd);
    let mut child = match spawn_group(cmd) {
        Ok(c) => c,
        Err(e) => {
            warn!(%e, path = %bin.display(), "wl-copy (via timeout) spawn failed");
            return false;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(text.as_bytes()) {
            warn!(%e, "wl-copy stdin write failed");
            kill_group(&mut child, "wl-copy");
            return false;
        }
        // Drop stdin so wl-copy sees EOF and the parent can exit.
    }
    match wait_cli(child, "wl-copy", WRITE_TIMEOUT) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secs_ceil_rounds_up() {
        assert_eq!(secs_ceil(Duration::from_millis(800)), "1");
        assert_eq!(secs_ceil(Duration::from_secs(4)), "4");
        assert_eq!(secs_ceil(Duration::from_millis(4001)), "5");
    }

    #[test]
    fn op_budget_covers_clear_plus_write() {
        // Outer budget must exceed clear + write poll caps (not equal).
        assert!(OP_BUDGET > CLEAR_TIMEOUT + WRITE_TIMEOUT);
    }
}
