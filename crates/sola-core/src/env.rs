//! Environment discovery helpers shared across Sola components.
//!
//! Centralizes three patterns that were reimplemented in several crates:
//! - resolving `$XDG_RUNTIME_DIR`,
//! - reading the name files sola-river publishes (`sola-wayland`,
//!   `sola-display`),
//! - probing live Unix sockets for Wayland/X11 displays.

use std::path::{Path, PathBuf};

/// Filename (inside `$XDG_RUNTIME_DIR`) where sola publishes the name of
/// the live Wayland socket (e.g. `wayland-1`). Written by the River
/// supervisor once the socket is confirmed to be accepting connections.
pub const SOLA_WAYLAND_NAME_FILE: &str = "sola-wayland";

/// Filename (inside `$XDG_RUNTIME_DIR`) where sola publishes the X11
/// DISPLAY string (e.g. `:0`) that River's XWayland is serving. Absent
/// when XWayland isn't active.
pub const SOLA_DISPLAY_NAME_FILE: &str = "sola-display";

/// Directory holding X11 Unix-domain display sockets (`X0`, `X1`, ...).
const X_UNIX_DIR: &str = "/tmp/.X11-unix";

/// Return `$XDG_RUNTIME_DIR` as a `PathBuf`, or `/tmp` if the variable
/// is unset. `/tmp` is the historical XDG fallback and is always present.
pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Read and trim a single-line name file at `$XDG_RUNTIME_DIR/<file>`.
/// Returns `None` if the variable is unset, the file is missing, or the
/// contents are empty after trimming.
pub fn read_runtime_name(file: &str) -> Option<String> {
    let path = runtime_dir().join(file);
    let raw = std::fs::read_to_string(&path).ok()?;
    let name = raw.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Wayland socket name published by the River supervisor. `None` if
/// River hasn't come up yet (or isn't running).
pub fn wayland_socket() -> Option<String> {
    read_runtime_name(SOLA_WAYLAND_NAME_FILE)
}

/// Resolve the X11 display user apps should target.
///
/// Prefers the name River published to `$XDG_RUNTIME_DIR/sola-display`;
/// if absent (XWayland started lazily or the publish missed), falls back
/// to a live probe of `/tmp/.X11-unix/X*`.
pub fn x_display() -> Option<String> {
    read_runtime_name(SOLA_DISPLAY_NAME_FILE).or_else(probe_live_x_display)
}

/// Return `:N` for the lowest-numbered X11 display whose socket at
/// `/tmp/.X11-unix/X<N>` accepts a connection. `None` if none do.
///
/// Liveness probing matters because X sockets are often left behind by
/// dead servers and the filesystem entry alone doesn't indicate that
/// anything is listening.
pub fn probe_live_x_display() -> Option<String> {
    let dir = std::fs::read_dir(X_UNIX_DIR).ok()?;
    let mut candidates: Vec<(u32, PathBuf)> = dir
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_owned();
            let n = name.strip_prefix('X')?.parse::<u32>().ok()?;
            Some((n, e.path()))
        })
        .collect();
    candidates.sort_by_key(|(n, _)| *n);
    for (n, path) in candidates {
        if std::os::unix::net::UnixStream::connect(&path).is_ok() {
            return Some(format!(":{n}"));
        }
    }
    None
}

/// Find the first `wayland-N` socket in `dir` (excluding `.lock` files
/// and XWayland's `wayland-x*`) that accepts a connection. Returns the
/// socket *name* (e.g. `"wayland-1"`), not the full path.
pub fn find_live_wayland_socket(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut candidates: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            if !name.starts_with("wayland-") {
                return None;
            }
            if name.starts_with("wayland-x") || name.ends_with(".lock") {
                return None;
            }
            Some(name)
        })
        .collect();
    // Lowest N first so we're deterministic if multiple servers are live.
    candidates.sort();
    for name in candidates {
        let path = dir.join(&name);
        if std::os::unix::net::UnixStream::connect(&path).is_ok() {
            return Some(name);
        }
    }
    None
}

/// Resolve which wayland socket to connect to, prioritising the name
/// sola-river published over the inherited `$WAYLAND_DISPLAY` env (the
/// env may be stale from a prior session). Polls
/// `$XDG_RUNTIME_DIR/sola-wayland` for up to `timeout_ms`, then falls
/// back to `$WAYLAND_DISPLAY`, then to `"wayland-0"`.
///
/// Pure: no env-var mutation, no side effects beyond filesystem reads.
/// Use [`activate_wayland_session`] when you also want the env set.
pub fn resolve_wayland_display(timeout_ms: u64) -> String {
    let start = std::time::Instant::now();
    let interval = std::time::Duration::from_millis(500);
    loop {
        if let Some(name) = wayland_socket() {
            return name;
        }
        if (start.elapsed().as_millis() as u64) >= timeout_ms {
            break;
        }
        std::thread::sleep(interval);
    }
    if let Ok(v) = std::env::var("WAYLAND_DISPLAY") {
        if !v.is_empty() {
            return v;
        }
    }
    "wayland-0".to_string()
}

/// Resolve the sola-river wayland socket (per [`resolve_wayland_display`])
/// AND set `WAYLAND_DISPLAY` to it so any wayland client library — winit,
/// sctk, raw wayland-client — picks it up without the caller having to
/// thread the value through. Returns the resolved name for callers that
/// want to log or use it directly.
///
/// Call once, early, single-threaded — before any wayland connection
/// is opened in this process. Safe to call from a non-graphical shell
/// where `WAYLAND_DISPLAY` isn't set by the login session; that's the
/// whole point. `XDG_RUNTIME_DIR` must already be set in env (any
/// active user session sets it; a fully bare environment isn't supported).
pub fn activate_wayland_session(timeout_ms: u64) -> String {
    let display = resolve_wayland_display(timeout_ms);
    // SAFETY: documented as single-threaded pre-init.
    unsafe { std::env::set_var("WAYLAND_DISPLAY", &display) };
    display
}

/// Block until `$XDG_RUNTIME_DIR/<display>` exists, or the timeout
/// elapses. The name returned by [`activate_wayland_session`] points
/// at a socket that river *intends* to publish, but on a fresh boot
/// the name file can land microseconds before the socket itself is
/// bind-ready. Wayland clients (winit, sctk, smithay-clipboard)
/// connect early enough that this race produces "no such file or
/// directory" failures.
///
/// Returns `true` if the socket appeared, `false` on timeout. Caller
/// chooses whether timeout is fatal — kit `startup()` logs and
/// continues so partial bus connectivity is still observable.
pub fn wait_for_wayland_socket(display: &str, timeout_ms: u64) -> bool {
    let path = runtime_dir().join(display);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let step = std::time::Duration::from_millis(50);
    loop {
        if path.exists() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(step);
    }
}

/// True on an Oath guest (live catalog at `/oath`).
pub fn on_oath() -> bool {
    Path::new("/oath/INDEX.md").is_file()
}

/// Directory of Sola ELFs: `/bin` on Oath (symlink farm), `/opt/sola/bin`
/// on NixOS.
pub fn bin_dir() -> PathBuf {
    if on_oath() {
        PathBuf::from("/bin")
    } else {
        PathBuf::from("/opt/sola/bin")
    }
}

/// `bin_dir()` joined with a binary name (`sola-session`, …).
pub fn bin_path(name: &str) -> PathBuf {
    bin_dir().join(name)
}

/// Point NixOS-specific GPU dispatch env at `/run/opengl-driver/` so
/// any wgpu/EGL/VAAPI/Vulkan client launched from a TTY (where the
/// desktop session never ran to set these) can actually find vendor
/// ICDs and dispatch onto the GPU.
///
/// Sets, only if unset:
///
/// - `__EGL_VENDOR_LIBRARY_DIRS` — libglvnd's vendor-ICD search path.
///   Without it libEGL.so loads but dispatches to nothing → wgpu's
///   GL backend silently produces empty geometry.
/// - `LIBVA_DRIVERS_PATH` — VA-API decoder DSO directory.
/// - `VK_ICD_FILENAMES` — explicit Vulkan ICD path. wgpu's Vulkan
///   backend (the default on Linux) can't initialise without this on
///   NixOS — the loader has no fallback search.
/// - `GSETTINGS_BACKEND=memory` — Chromium/GLib probe GSettings on
///   start; with no schema source in scope they spam
///   `g_settings_schema_source_lookup` failures. Memory backend
///   returns empty values, which is what missing schemas would
///   produce anyway.
///
/// Currently pins `VK_ICD_FILENAMES` to the NVIDIA ICD because the
/// dev box is NVIDIA-only. Cross-vendor support waits until we have
/// a Mesa-on-NixOS test rig.
///
/// Call once, single-threaded, before any GPU library initialises —
/// in practice that's right after `activate_wayland_session` in each
/// kit's `startup()`.
pub fn activate_gpu_env() {
    // SAFETY: documented as single-threaded pre-init.
    unsafe {
        if std::env::var_os("__EGL_VENDOR_LIBRARY_DIRS").is_none() {
            std::env::set_var(
                "__EGL_VENDOR_LIBRARY_DIRS",
                "/run/opengl-driver/share/glvnd/egl_vendor.d",
            );
        }
        if std::env::var_os("LIBVA_DRIVERS_PATH").is_none() {
            std::env::set_var("LIBVA_DRIVERS_PATH", "/run/opengl-driver/lib/dri");
        }
        if std::env::var_os("VK_ICD_FILENAMES").is_none() {
            std::env::set_var(
                "VK_ICD_FILENAMES",
                "/run/opengl-driver/share/vulkan/icd.d/nvidia_icd.x86_64.json",
            );
        }
        if std::env::var_os("GSETTINGS_BACKEND").is_none() {
            std::env::set_var("GSETTINGS_BACKEND", "memory");
        }
    }
}

/// True when this process has a working `systemd --user` manager
/// (`systemd-run --user --scope`, transient tmux units). Loginless
/// install seats often do not.
pub fn user_systemd_available() -> bool {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        if runtime_dir_has_systemd_private(&dir) {
            return true;
        }
    }
    match std::process::Command::new("systemctl")
        .args(["--user", "is-system-running"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        // 0 = running; 1 often means degraded but still usable.
        Ok(st) => st.success() || st.code() == Some(1),
        Err(_) => false,
    }
}

fn runtime_dir_has_systemd_private(dir: &str) -> bool {
    Path::new(dir).join("systemd/private").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemd_private_socket_is_under_runtime_dir() {
        assert!(!runtime_dir_has_systemd_private(
            "/tmp/sola-no-systemd-here"
        ));
    }

    #[test]
    fn bin_dir_is_opt_sola_off_oath() {
        // Host unit tests do not have `/oath/INDEX.md`.
        assert!(!on_oath());
        assert_eq!(bin_dir(), PathBuf::from("/opt/sola/bin"));
        assert_eq!(
            bin_path("sola-session"),
            PathBuf::from("/opt/sola/bin/sola-session")
        );
    }
}
