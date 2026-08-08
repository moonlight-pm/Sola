//! Build / execute the host launch path for a Steam game under **windowed**
//! gamescope.
//!
//! Product rule: host always has fixed size (`-W`/`-H`); never host `-f`.
//! Nested windowed / borderless / exclusive FS inside gamescope is fine.
//!
//! ## Steam already running (critical)
//!
//! `steam -applaunch` only pokes an **existing** client — the game is **not**
//! a child of gamescope, escapes the nest, and often takes true exclusive
//! fullscreen on Sola.
//!
//! We used to `steam -shutdown` then restart under gamescope. That tears down
//! Xwayland surfaces mid-flight and has **crashed River** (`Protocol error 2
//! on river_window_manager_v1`) — hosing the whole Sola shell. **Never
//! auto-shutdown Steam from Arcade.**
//!
//! Safe policy:
//! - Steam **not** running → `gamescope -W/-H -- steam -applaunch <id>` (nest).
//! - Steam **running** → bare `steam -applaunch <id>` only; UI warns that
//!   windowed nest needs Steam quit first (user-initiated).

use std::process::{Command, Stdio};

use sola_core::applications::resolve_in_path;

/// Default host width when arcade has no override.
pub const DEFAULT_HOST_WIDTH: u32 = 1920;
/// Default host height when arcade has no override.
pub const DEFAULT_HOST_HEIGHT: u32 = 1080;

/// Wayland / X11 app_id Steam usually reports (case variants exist).
#[allow(dead_code)] // kept for AppHidden / hide-Steam if we reintroduce it
pub const STEAM_APP_ID: &str = "steam";

/// Session / LaunchApp identity for a gamescope-wrapped game process.
pub fn game_session_app_id(steam_app_id: u32) -> String {
    format!("steam-game-{steam_app_id}")
}

/// Whether a Steam *client* process appears to be running already.
///
/// Matches the real Steam binary / srt launcher, not arbitrary paths that
/// merely contain the word "steam". Used only to decide nest vs bare
/// applaunch (never auto-`steam -shutdown`).
pub fn steam_running() -> bool {
    // Prefer the known Steam main binary path; fall back to a tighter pgrep.
    if process_cmdline_contains(b"ubuntu12_32/steam") {
        return true;
    }
    Command::new("pgrep")
        .args(["-f", r"ubuntu12_32/steam |[/]steam -srt-logger-opened|[/]steam -silent"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `/proc/<pid>/cmdline` is NUL-separated argv. Normalize to spaces so
/// multi-token needles like `sola-arcade --run 400` match.
fn read_cmdline_spaced(pid_dir: &std::path::Path) -> Vec<u8> {
    let raw = std::fs::read(pid_dir.join("cmdline")).unwrap_or_default();
    raw.into_iter()
        .map(|b| if b == 0 { b' ' } else { b })
        .collect()
}

fn process_cmdline_contains(needle: &[u8]) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let cmdline = read_cmdline_spaced(&entry.path());
        if cmdline.windows(needle.len()).any(|w| w == needle) {
            return true;
        }
    }
    false
}

/// True while Arcade's `--run` helper (and typically gamescope/Steam) for
/// this app id is still alive.
pub fn session_alive(steam_app_id: u32) -> bool {
    let run = format!("sola-arcade --run {steam_app_id}");
    if process_cmdline_contains(run.as_bytes()) {
        return true;
    }
    // Nest host still up with this applaunch (even if reaper reparented).
    let launch = format!("-applaunch {steam_app_id}");
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let cmdline = read_cmdline_spaced(&entry.path());
        let has_launch = cmdline
            .windows(launch.len())
            .any(|w| w == launch.as_bytes());
        let is_gs = cmdline.windows(b"gamescope".len()).any(|w| w == b"gamescope");
        if has_launch && is_gs {
            return true;
        }
    }
    false
}

/// Best-effort kill of a nest started by `sola-arcade --run <steam_app_id>`.
/// Prefer `Topic::CloseApp(steam-game-<id>)` first so sola-session stops the
/// scope cleanly; this is a fallback for leftover processes.
pub fn stop_nest_local(steam_app_id: u32) {
    let run = format!("sola-arcade --run {steam_app_id}");
    let _ = Command::new("pkill")
        .args(["-f", &run])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    // Also kill a leftover gamescope still holding this applaunch.
    let gs = format!("-applaunch {steam_app_id}");
    let _ = Command::new("pkill")
        .args(["-f", &format!("gamescope.*{gs}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn gamescope_bin() -> Option<std::path::PathBuf> {
    resolve_in_path("gamescope").or_else(|| {
        let p = std::path::PathBuf::from("/opt/sola/bin/gamescope");
        p.is_file().then_some(p)
    })
}

/// LaunchApp argv for sola-session (whitespace-split, no shell):
///
/// ```text
/// /opt/sola/bin/sola-arcade --run <appid> <width> <height>
/// ```
pub fn launch_command(steam_app_id: u32, width: u32, height: u32) -> LaunchPlan {
    let arcade = resolve_in_path("sola-arcade")
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/opt/sola/bin/sola-arcade".into());
    let steam_open = steam_running();
    let have_gs = gamescope_bin().is_some();
    // Nest only when we can and Steam is cold — otherwise bare applaunch.
    let will_nest = have_gs && !steam_open;

    LaunchPlan {
        command: format!("{arcade} --run {steam_app_id} {width} {height}"),
        gamescope: will_nest,
        steam_already_running: steam_open,
        host_width: width,
        host_height: height,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub command: String,
    /// True only when the run helper will actually start gamescope.
    pub gamescope: bool,
    pub steam_already_running: bool,
    pub host_width: u32,
    pub host_height: u32,
}

/// Entry for `sola-arcade --run <appid> [width] [height]`.
/// Does not return on success (process exits with child status).
pub fn run_game_blocking(steam_app_id: u32, width: u32, height: u32) -> ! {
    let steam = resolve_in_path("steam")
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "steam".into());

    let app = steam_app_id.to_string();
    let w = width.to_string();
    let h = height.to_string();

    let status = if steam_running() {
        // Do **not** steam -shutdown here — Xwayland teardown races River.
        eprintln!(
            "sola-arcade: Steam already running — launching bare steam -applaunch \
             (no gamescope nest; quit Steam first for windowed nest)"
        );
        Command::new(&steam)
            .args(["-applaunch", &app])
            .status()
    } else if let Some(gs) = gamescope_bin() {
        // Cold Steam under windowed gamescope host (never host -f).
        //
        // Host backend: **wayland** + borderless. Live probes on River+NVIDIA
        // (2026-08-08):
        // - `--backend wayland -b` + glxgears → River `dimensions` + pixels.
        // - same + **`-e`** (Steam integration) → host stays `size=None`
        //   forever (even with glxgears) — held first-frame / steam-mode path.
        // - `--backend sdl` → internal swapchain, host window never maps to
        //   River (no switcher).
        //
        // So we deliberately **omit `-e`**. Nested Steam still runs; we lose
        // some gamescope↔Steam overlay integration until we have a better
        // steam-mode path under River.
        //
        // Other flags:
        // - `-b` — borderless host (simpler libdecor map)
        // - nested `-w`/`-h` fixed at Arcade nest size (Proton sees stable res)
        // - initial `-W`/`-H` same; host may later be zoned larger/smaller —
        //   `-S fit` letterbox-scales nested content into the host (scale-to-fit
        //   without changing nested resolution)
        // - `steam -silent` — avoid BPM as the primary surface
        // - `SteamDeck=0` — don't force gamepad UI under the nest
        eprintln!(
            "sola-arcade: nesting steam -silent -applaunch {app} under gamescope \
             {w}x{h} (--backend wayland -b -S fit, no -e, SteamDeck=0)"
        );
        let mut cmd = Command::new(gs);
        cmd.args([
            "--backend",
            "wayland",
            "-b",
            "-S",
            "fit",
            "-W",
            &w,
            "-H",
            &h,
            "-w",
            &w,
            "-h",
            &h,
            "--",
            &steam,
            "-silent",
            "-applaunch",
            &app,
        ])
        .env("SteamDeck", "0")
        .env("STEAM_USE_GAMEPADUI", "0")
        .status()
    } else {
        eprintln!(
            "sola-arcade: gamescope not found — bare steam -applaunch \
             (game may take exclusive fullscreen on Sola)"
        );
        Command::new(&steam)
            .args(["-applaunch", &app])
            .status()
    };

    let code = status
        .map(|s| s.code().unwrap_or(1))
        .unwrap_or(1);
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_command_uses_run_subcommand_never_host_fullscreen() {
        let plan = launch_command(400, 1280, 720);
        let tokens: Vec<&str> = plan.command.split_whitespace().collect();
        assert!(
            !tokens.iter().any(|t| *t == "-f" || *t == "--fullscreen"),
            "must not pass host fullscreen: {}",
            plan.command
        );
        assert!(plan.command.contains("--run"));
        assert!(plan.command.contains("400"));
        assert!(
            !plan.command.contains('"') && !plan.command.contains('\''),
            "must not need shell quoting: {}",
            plan.command
        );
    }

    #[test]
    fn game_session_app_id_stable() {
        assert_eq!(game_session_app_id(400), "steam-game-400");
    }
}
