//! Live Fit: retarget gamescope's nested Xwayland to the host window size.
//!
//! Stock gamescope has no `--nested-auto-resize`. The proven path (Factorio
//! under windowed gamescope) is:
//! 1. `GAMESCOPE_FORCE_WINDOWS_FULLSCREEN=1` on the nested root
//! 2. `GAMESCOPE_XWAYLAND_MODE_CONTROL` = `[server_idx, w, h, allowSuperRes]`
//! 3. `ConfigureWindow` the focused client to `0,0,w,h` (mode change alone
//!    can leave the game parked at the old origin — dead clicks)
//!
//! Arcade UI drives this from `Topic::WindowGeometry` on the gamescope host.
//! DISPLAY comes from the `--nested-steam` child's environ, not Arcade's.

use std::fs;
use std::path::Path;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConfigureWindowAux, ConnectionExt, PropMode};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// Ignore transient / unmapped host sizes (pre-init 1×1, etc.).
pub const MIN_FIT_EDGE: u32 = 64;

/// Whether a host size should trigger a nest poke.
pub fn should_apply_fit(width: u32, height: u32, last: Option<(u32, u32)>) -> bool {
    if width < MIN_FIT_EDGE || height < MIN_FIT_EDGE {
        return false;
    }
    last != Some((width, height))
}

/// Poke nested mode + focused window to `width`×`height`.
pub fn apply_fit(steam_app_id: u32, width: u32, height: u32) -> Result<(), String> {
    if width < MIN_FIT_EDGE || height < MIN_FIT_EDGE {
        return Ok(());
    }
    let dpy = nested_display(steam_app_id).ok_or_else(|| {
        format!("no nested DISPLAY for sola-arcade --nested-steam {steam_app_id}")
    })?;
    let (conn, screen) = x11rb::connect(Some(&dpy)).map_err(|e| format!("X connect {dpy}: {e}"))?;
    poke_nested(&conn, screen, width, height, &dpy)?;
    tracing::info!(
        steam_app_id,
        width,
        height,
        dpy = dpy.as_str(),
        "arcade fit nest poke"
    );
    Ok(())
}

fn poke_nested(
    conn: &RustConnection,
    screen: usize,
    width: u32,
    height: u32,
    dpy: &str,
) -> Result<(), String> {
    let root = conn
        .setup()
        .roots
        .get(screen)
        .ok_or("X screen missing")?
        .root;
    let mode_atom = intern_existing(conn, b"GAMESCOPE_XWAYLAND_MODE_CONTROL")?;
    let force_atom = intern_existing(conn, b"GAMESCOPE_FORCE_WINDOWS_FULLSCREEN")?;
    let focused_atom = intern_existing(conn, b"GAMESCOPE_FOCUSED_WINDOW")?;
    let server_atom = intern_existing(conn, b"GAMESCOPE_XWAYLAND_SERVER_ID")?;

    let server_idx = card32(conn, root, server_atom).ok_or_else(|| {
        format!("GAMESCOPE_XWAYLAND_SERVER_ID unset on {dpy} — refusing host X poke")
    })?;
    conn.change_property32(
        PropMode::REPLACE,
        root,
        force_atom,
        AtomEnum::CARDINAL,
        &[1],
    )
    .map_err(|e| format!("FORCE_WINDOWS_FULLSCREEN: {e}"))?;
    conn.change_property32(
        PropMode::REPLACE,
        root,
        mode_atom,
        AtomEnum::CARDINAL,
        &[server_idx, width, height, 1],
    )
    .map_err(|e| format!("MODE_CONTROL: {e}"))?;
    conn.flush().map_err(|e| format!("flush mode: {e}"))?;

    if let Some(xid) = card32(conn, root, focused_atom) {
        if xid != 0 && xid != root {
            conn.configure_window(
                xid,
                &ConfigureWindowAux::new()
                    .x(0)
                    .y(0)
                    .width(width)
                    .height(height),
            )
            .map_err(|e| format!("configure 0x{xid:x}: {e}"))?;
            conn.flush().map_err(|e| format!("flush configure: {e}"))?;
        }
    }
    Ok(())
}

fn intern_existing(conn: &RustConnection, name: &[u8]) -> Result<Atom, String> {
    let atom = conn
        .intern_atom(true, name)
        .map_err(|e| format!("intern_atom: {e}"))?
        .reply()
        .map(|r| r.atom)
        .map_err(|e| format!("intern_atom reply: {e}"))?;
    if atom == 0 {
        return Err(format!(
            "{} missing — not a gamescope nested X",
            String::from_utf8_lossy(name)
        ));
    }
    Ok(atom)
}

fn card32(conn: &RustConnection, window: u32, atom: Atom) -> Option<u32> {
    let reply = conn
        .get_property(false, window, atom, AtomEnum::CARDINAL, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    reply.value32()?.next()
}

/// Nested X display from the `--nested-steam` helper (not gamescope's own env).
///
/// gamescope's argv includes `-- sola-arcade --nested-steam <id>`, so a naive
/// cmdline search hits the **host** gamescope process (`DISPLAY=:0`) and pokes
/// the Sola Xwayland — that aborted gamescope's Wayland input thread.
fn nested_display(steam_app_id: u32) -> Option<String> {
    let host = std::env::var("DISPLAY").ok();
    let nested = format!("sola-arcade --nested-steam {steam_app_id}");
    let app = format!("AppId={steam_app_id}");
    find_display_for(nested.as_bytes(), MatchKind::ArcadeHelper, host.as_deref())
        .or_else(|| find_display_for(app.as_bytes(), MatchKind::Game, host.as_deref()))
}

#[derive(Clone, Copy)]
enum MatchKind {
    /// argv0 is sola-arcade (the nest helper). Skip gamescope's host argv.
    ArcadeHelper,
    /// Steam reaper / game (`AppId=`). Skip gamescope.
    Game,
}

fn find_display_for(needle: &[u8], kind: MatchKind, host_dpy: Option<&str>) -> Option<String> {
    let entries = fs::read_dir("/proc").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let dir = entry.path();
        let cmdline = spaced_cmdline(&dir);
        if !cmdline.windows(needle.len()).any(|w| w == needle) {
            continue;
        }
        if !kind.matches_argv0(&cmdline) {
            continue;
        }
        let Some(d) = env_var(
            &fs::read(dir.join("environ")).unwrap_or_default(),
            "DISPLAY",
        ) else {
            continue;
        };
        if display_is_host(&d, host_dpy) {
            continue;
        }
        return Some(d);
    }
    None
}

impl MatchKind {
    fn matches_argv0(self, cmdline: &[u8]) -> bool {
        match self {
            Self::ArcadeHelper => argv0_is_arcade(cmdline) && !argv0_is_gamescope(cmdline),
            Self::Game => !argv0_is_gamescope(cmdline),
        }
    }
}

fn spaced_cmdline(pid_dir: &Path) -> Vec<u8> {
    let raw = fs::read(pid_dir.join("cmdline")).unwrap_or_default();
    raw.into_iter()
        .map(|b| if b == 0 { b' ' } else { b })
        .collect()
}

fn argv0(cmdline: &[u8]) -> &[u8] {
    cmdline.split(|b| *b == b' ').next().unwrap_or(b"")
}

fn argv0_basename(cmdline: &[u8]) -> &[u8] {
    argv0(cmdline).rsplit(|b| *b == b'/').next().unwrap_or(b"")
}

/// Host gamescope argv0 (`gamescope`, `gamescope-wl`, nix `.gamescope-wrapped`).
pub fn argv0_is_gamescope(cmdline: &[u8]) -> bool {
    let base = argv0_basename(cmdline);
    base == b"gamescope" || base == b"gamescope-wl" || base == b".gamescope-wrapped"
}

pub fn argv0_is_arcade(cmdline: &[u8]) -> bool {
    argv0_basename(cmdline) == b"sola-arcade"
}

/// Same server as Arcade's own `$DISPLAY` (`:0` vs `:0.0`).
pub fn display_is_host(dpy: &str, host: Option<&str>) -> bool {
    let Some(host) = host else {
        return false;
    };
    normalize_display(dpy) == normalize_display(host)
}

fn normalize_display(dpy: &str) -> &str {
    dpy.split('.').next().unwrap_or(dpy)
}

/// NUL-separated `/proc/.../environ` → first `KEY=value`.
pub fn env_var(environ: &[u8], key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    for entry in environ.split(|b| *b == 0) {
        let Ok(s) = std::str::from_utf8(entry) else {
            continue;
        };
        if let Some(v) = s.strip_prefix(&prefix) {
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_tiny_and_identical() {
        assert!(!should_apply_fit(16, 16, None));
        assert!(should_apply_fit(2253, 2132, None));
        assert!(!should_apply_fit(2253, 2132, Some((2253, 2132))));
        assert!(should_apply_fit(1920, 1080, Some((2253, 2132))));
    }

    #[test]
    fn environ_display_from_nested_steam() {
        let env = b"HOME=/home/j\0DISPLAY=:1\0WAYLAND_DISPLAY=wayland-1\0";
        assert_eq!(env_var(env, "DISPLAY").as_deref(), Some(":1"));
        assert!(env_var(env, "XAUTHORITY").is_none());
        assert!(env_var(b"", "DISPLAY").is_none());
    }

    #[test]
    fn gamescope_host_argv_is_not_nested_steam() {
        let gs = b"/opt/sola/bin/gamescope --backend wayland -b -- /opt/sola/bin/sola-arcade --nested-steam 427520";
        assert!(argv0_is_gamescope(gs));
        assert!(!argv0_is_arcade(gs));
        assert!(!MatchKind::ArcadeHelper.matches_argv0(gs));
        let helper = b"/opt/sola/bin/sola-arcade --nested-steam 427520";
        assert!(argv0_is_arcade(helper));
        assert!(!argv0_is_gamescope(helper));
        assert!(MatchKind::ArcadeHelper.matches_argv0(helper));
    }

    #[test]
    fn host_display_colon_zero_rejected() {
        assert!(display_is_host(":0", Some(":0")));
        assert!(display_is_host(":0.0", Some(":0")));
        assert!(!display_is_host(":1", Some(":0")));
        assert!(!display_is_host(":1", None));
    }
}
