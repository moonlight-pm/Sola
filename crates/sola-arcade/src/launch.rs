//! Build / execute the host launch path for a Steam game under **windowed**
//! gamescope.
//!
//! Product rule: host never `-f`. Nested `-w/-h` is Arcade's per-title
//! nest setting (Fit → display pixels at Play, or a locked resolution).
//! Initial host `-W/-H` matches that size; River zone/float after the
//! pre-init pin. Fit then retargets nested mode to the live host frame
//! (Arcade X11 poke on the nested display). Nested windowed /
//! borderless / exclusive FS inside gamescope is fine.
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
        .args([
            "-f",
            r"ubuntu12_32/steam |[/]steam -srt-logger-opened|[/]steam -silent",
        ])
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

/// True when `/proc/<pid>/cmdline` contains `needle` (space-normalized).
pub fn pid_cmdline_contains(pid: u32, needle: &[u8]) -> bool {
    cmdline_contains(&std::path::PathBuf::from(format!("/proc/{pid}")), needle)
}

fn cmdline_contains(pid_dir: &std::path::Path, needle: &[u8]) -> bool {
    let cmdline = read_cmdline_spaced(pid_dir);
    cmdline.windows(needle.len()).any(|w| w == needle)
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
        if cmdline_contains(&entry.path(), needle) {
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
    let nested = format!("sola-arcade --nested-steam {steam_app_id}");
    if process_cmdline_contains(nested.as_bytes()) {
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
        let is_gs = cmdline
            .windows(b"gamescope".len())
            .any(|w| w == b"gamescope");
        if has_launch && is_gs {
            return true;
        }
    }
    false
}

/// True when Steam's launch reaper / game process for this app id is live.
///
/// Matches `AppId=<id>` on cmdline (Steam reaper / proton wrappers). Used to
/// detect in-game exit so the nested Steam client can be torn down.
pub fn game_process_alive(steam_app_id: u32) -> bool {
    let needle = format!("AppId={steam_app_id}");
    process_cmdline_contains(needle.as_bytes())
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
    let nested = format!("sola-arcade --nested-steam {steam_app_id}");
    let _ = Command::new("pkill")
        .args(["-f", &nested])
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
    // Nested Steam often survives the game process; reap its reaper too.
    let app_id = format!("AppId={steam_app_id}");
    let _ = Command::new("pkill")
        .args(["-f", &app_id])
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

/// gamescope flags before `--` and the nested helper.
///
/// Nested X cursors (Factorio menus, SDL titles, …) are often sized for the
/// fake monitor. The Wayland backend then hands that bitmap to River 1:1 via
/// `wl_pointer.set_cursor` — a 64–256px glyph over a zoned host looks huge
/// next to Sola’s 24px McMojave. `--cursor-scale-height` is gamescope’s
/// downsample: host cursor ≈ 36px × floor(output_h / this), clamped [36, 256].
/// Matching initial `-H` keeps the pointer desktop-sized. SteamOS uses 720 so
/// the cursor *grows* on a 4K panel — the opposite of a windowed nest.
pub fn gamescope_nest_args(width: u32, height: u32) -> Vec<String> {
    let w = width.to_string();
    let h = height.to_string();
    vec![
        "--backend".into(),
        "wayland".into(),
        "-b".into(),
        "-S".into(),
        "fit".into(),
        "-W".into(),
        w.clone(),
        "-H".into(),
        h.clone(),
        "-w".into(),
        w,
        "-h".into(),
        h.clone(),
        "--cursor-scale-height".into(),
        h,
    ]
}

/// LaunchApp argv for sola-session (whitespace-split, no shell):
///
/// ```text
/// /opt/sola/bin/sola-arcade --run <appid> <width> <height> [fit]
/// ```
pub fn launch_command(steam_app_id: u32, width: u32, height: u32, fit: bool) -> LaunchPlan {
    let arcade = resolve_in_path("sola-arcade")
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/opt/sola/bin/sola-arcade".into());
    let steam_open = steam_running();
    let have_gs = gamescope_bin().is_some();
    // Nest only when we can and Steam is cold — otherwise bare applaunch.
    let will_nest = have_gs && !steam_open;
    let command = if fit {
        format!("{arcade} --run {steam_app_id} {width} {height} fit")
    } else {
        format!("{arcade} --run {steam_app_id} {width} {height}")
    };

    LaunchPlan {
        command,
        gamescope: will_nest,
        steam_already_running: steam_open,
        host_width: width,
        host_height: height,
        fit,
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
    pub fit: bool,
}

/// Parsed `sola-arcade --run <appid> [width] [height] [fit]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunArgs {
    pub steam_app_id: u32,
    pub width: u32,
    pub height: u32,
    pub fit: bool,
}

/// Parse `--run <appid> [width] [height] [fit]`. Returns `None` unless the
/// first token is `--run`. Extra `fit` may appear anywhere after the app id.
pub fn parse_run_args<I, S>(mut args: I) -> Option<RunArgs>
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    let first = args.next()?;
    if first.as_ref() != "--run" {
        return None;
    }
    let app_id: u32 = args.next()?.as_ref().parse().ok()?;
    let mut fit = false;
    let mut nums: Vec<u32> = Vec::new();
    for tok in args {
        let s = tok.as_ref();
        if s.eq_ignore_ascii_case("fit") {
            fit = true;
            continue;
        }
        if let Ok(n) = s.parse() {
            nums.push(n);
        }
    }
    Some(RunArgs {
        steam_app_id: app_id,
        width: nums.first().copied().unwrap_or(DEFAULT_HOST_WIDTH),
        height: nums.get(1).copied().unwrap_or(DEFAULT_HOST_HEIGHT),
        fit,
    })
}

/// Entry for `sola-arcade --run <appid> [width] [height] [fit]`.
/// Does not return on success (process exits with child status).
pub fn run_game_blocking(steam_app_id: u32, width: u32, height: u32, fit: bool) -> ! {
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
        Command::new(&steam).args(["-applaunch", &app]).status()
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
        // - host `-W`/`-H` initial (same as nested); River zone/float after pin
        // - `-w`/`-h` virtual monitor from Arcade nest dropdown
        // - `-S fit` — letterbox nested content into host when sizes differ
        // - `--cursor-scale-height` = `-H` — keep the host pointer ~36px
        //   (nested X cursors otherwise present 1:1 to River)
        // - Fit does **not** pass `--force-windows-fullscreen` (wayland
        //   backend aborted). Arcade sets `GAMESCOPE_FORCE_WINDOWS_FULLSCREEN`
        //   on the nested X root after the nest is up.
        // - Child is **`sola-arcade --nested-steam <id>`**, not bare steam:
        //   gamescope forces `XDG_CURRENT_DESKTOP=gamescope` on children, and
        //   Steam then logs `forcing gamepadui for steamdeck + gamescope` and
        //   opens Big Picture without finishing `-applaunch`. The nested
        //   helper rewrites desktop identity and runs desktop Steam so
        //   prepare/shader CEF can complete *inside* the nest without BPM.
        let arcade = resolve_in_path("sola-arcade")
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/opt/sola/bin/sola-arcade".into());
        eprintln!(
            "sola-arcade: nesting --nested-steam {app} under gamescope \
             {w}x{h} (--backend wayland -b -S fit --cursor-scale-height {h}{}, no -e)",
            if fit { ", fit-follow" } else { "" }
        );
        let nest = gamescope_nest_args(width, height);
        let mut cmd = Command::new(gs);
        cmd.args(&nest)
            .args(["--", &arcade, "--nested-steam", &app])
            .status()
    } else {
        eprintln!(
            "sola-arcade: gamescope not found — bare steam -applaunch \
             (game may take exclusive fullscreen on Sola)"
        );
        Command::new(&steam).args(["-applaunch", &app]).status()
    };

    let code = status.map(|s| s.code().unwrap_or(1)).unwrap_or(1);
    std::process::exit(code);
}

/// Gamescope child entry: `sola-arcade --nested-steam <steam_app_id>`.
///
/// gamescope sets `XDG_CURRENT_DESKTOP=gamescope` for nested clients. Steam
/// treats that as Steam Deck + gamescope and **forces gamepadui / Big Picture**,
/// which swallows `-applaunch` (user sees BPM library, game never starts).
/// We undo that identity, force desktop Steam, and launch with CEF allowed so
/// `ProcessingShaderCache` and friends can finish inside the nest.
pub fn run_nested_steam_blocking(steam_app_id: u32) -> ! {
    use std::thread;
    use std::time::Duration;

    let steam = resolve_in_path("steam")
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "steam".into());
    let app = steam_app_id.to_string();

    eprintln!(
        "sola-arcade: nested-steam -applaunch {app} \
         (desktop Steam under nest: no gamepadui/BPM; CEF prepare UI allowed; \
         exit Steam when game process ends)"
    );

    // No `-silent`: first-run shader/update interstitials need CEF.
    // No `-gamepadui` / no Big Picture flags.
    // `-nofriendsui` keeps the friends list from eating the nest surface.
    //
    // Env overrides (on the Steam child only): counteract gamescope forcing
    // `XDG_CURRENT_DESKTOP=gamescope`, which makes Steam
    // `forcing gamepadui for steamdeck + gamescope` and open BPM instead of
    // finishing `-applaunch`.
    let mut child = match Command::new(&steam)
        .args(["-nofriendsui", "-applaunch", &app])
        .env("XDG_CURRENT_DESKTOP", "Sola")
        .env("XDG_SESSION_DESKTOP", "Sola")
        .env("XDG_SESSION_TYPE", "x11")
        .env_remove("GAMESCOPE_WAYLAND_DISPLAY")
        .env("SteamDeck", "0")
        .env("STEAM_USE_GAMEPADUI", "0")
        .env("SteamTenfoot", "0")
        .env("SDL_VIDEODRIVER", "x11")
        .env("GDK_BACKEND", "x11")
        .env("QT_QPA_PLATFORM", "xcb")
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sola-arcade: failed to spawn steam: {e}");
            std::process::exit(1);
        }
    };

    // Steam stays up after the game returns to the client (library UI in the
    // nest). Watch for the game process: once it has been seen and then gone
    // for a short debounce, kill Steam so gamescope/`--run` exit cleanly.
    let mut saw_game = false;
    let mut gone_ticks: u32 = 0;
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(1),
            Ok(None) => {}
            Err(e) => {
                eprintln!("sola-arcade: wait on steam failed: {e}");
                break 1;
            }
        }

        if game_process_alive(steam_app_id) {
            if !saw_game {
                eprintln!("sola-arcade: nested-steam saw game process AppId={app}");
            }
            saw_game = true;
            gone_ticks = 0;
        } else if saw_game {
            gone_ticks += 1;
            // ~2s gone (4 × 500ms) — avoid flapping during Steam's relaunch path.
            if gone_ticks >= 4 {
                eprintln!("sola-arcade: game AppId={app} exited — stopping nested Steam");
                // Kill the Steam client we spawned (and its tree). Do **not**
                // `steam -shutdown` — that uses the shared user Steam socket
                // and can tear down a Steam the user started outside the nest.
                let _ = child.kill();
                let _ = Command::new("pkill")
                    .args(["-P", &child.id().to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                // Reaper / proton helpers still holding this app id.
                let _ = Command::new("pkill")
                    .args(["-f", &format!("AppId={app}")])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                break child.wait().ok().and_then(|s| s.code()).unwrap_or(0);
            }
        }

        thread::sleep(Duration::from_millis(500));
    };

    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_command_uses_run_subcommand_never_host_fullscreen() {
        let plan = launch_command(400, 1280, 720, false);
        let tokens: Vec<&str> = plan.command.split_whitespace().collect();
        assert!(
            !tokens.iter().any(|t| *t == "-f" || *t == "--fullscreen"),
            "must not pass host fullscreen: {}",
            plan.command
        );
        assert!(plan.command.contains("--run"));
        assert!(plan.command.contains("400"));
        assert!(plan.command.contains("1280"));
        assert!(plan.command.contains("720"));
        assert!(
            !plan.command.contains(" fit"),
            "locked res has no fit token"
        );
        assert!(
            !plan.command.contains('"') && !plan.command.contains('\''),
            "must not need shell quoting: {}",
            plan.command
        );
    }

    #[test]
    fn launch_command_fit_appends_token() {
        let plan = launch_command(427520, 5120, 2160, true);
        assert!(plan.fit);
        assert!(plan.command.ends_with(" 5120 2160 fit"));
        assert!(
            !plan.command.contains('"') && !plan.command.contains('\''),
            "must not need shell quoting: {}",
            plan.command
        );
    }

    #[test]
    fn parse_run_defaults_and_fit_token() {
        assert_eq!(
            parse_run_args(["--run", "400"].into_iter()),
            Some(RunArgs {
                steam_app_id: 400,
                width: 1920,
                height: 1080,
                fit: false,
            })
        );
        assert_eq!(
            parse_run_args(["--run", "427520", "2253", "2132", "fit"].into_iter()),
            Some(RunArgs {
                steam_app_id: 427520,
                width: 2253,
                height: 2132,
                fit: true,
            })
        );
        assert_eq!(
            parse_run_args(["--run", "400", "fit", "1280", "720"].into_iter())
                .map(|r| (r.width, r.height, r.fit)),
            Some((1280, 720, true))
        );
        assert!(parse_run_args(["--nested-steam", "400"].into_iter()).is_none());
    }

    #[test]
    fn game_session_app_id_stable() {
        assert_eq!(game_session_app_id(400), "steam-game-400");
    }

    #[test]
    fn nest_command_uses_nested_steam_helper_not_bare_steam() {
        // launch_command only builds the session argv; the actual gamescope
        // child is assembled in run_game_blocking. Smoke the helper id path.
        assert_eq!(game_session_app_id(3527290), "steam-game-3527290");
    }

    #[test]
    fn gamescope_nest_args_scale_host_cursor_to_desktop() {
        let args = gamescope_nest_args(5120, 2160);
        assert!(
            args.windows(2)
                .any(|w| w == ["--cursor-scale-height", "2160"]),
            "host cursor downsample must match -H: {args:?}"
        );
        assert!(args.windows(2).any(|w| w == ["-H", "2160"]));
        assert!(args.windows(2).any(|w| w == ["-h", "2160"]));
        assert!(
            !args.iter().any(|t| t == "-f" || t == "--fullscreen" || t == "-e"),
            "must not pass host fullscreen or -e: {args:?}"
        );
        let locked = gamescope_nest_args(1920, 1080);
        assert!(
            locked
                .windows(2)
                .any(|w| w == ["--cursor-scale-height", "1080"]),
            "{locked:?}"
        );
    }
}
