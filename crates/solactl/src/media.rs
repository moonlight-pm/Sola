//! `solactl media <action>` — global media-key handling.
//!
//! Invoked per keypress by `sola-shell` when a global `XF86Audio*` chord
//! fires (sola-shell `keys.rs` registers the keysyms, `media.rs` spawns
//! us). River eats bound keys, so these never reach a focused window — the
//! whole point is focus-independent control.
//!
//!   - **Transport keys** (play-pause / next / prev) act on whatever MPRIS
//!     player is active over the *session* D-Bus — primarily the browser.
//!     No player-side work is needed; Chromium-based browsers already
//!     expose `org.mpris.MediaPlayer2.Player`.
//!   - **Mute / volume** act on the system default audio sink via `wpctl`
//!     (PipeWire). This is intentionally a sink action, not a player
//!     action, so it works even when nothing is playing.
//!
//! The no-op paths (no player present) exit 0 — a missing player is not an
//! error. Lookup failures against D-Bus / `wpctl` exit 3.

use std::process::Command;

use clap::ValueEnum;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum MediaAction {
    /// Toggle play/pause on the active MPRIS player.
    PlayPause,
    /// Skip to the next track on the active MPRIS player.
    Next,
    /// Skip to the previous track on the active MPRIS player.
    Prev,
    /// Toggle mute on the default audio sink (PipeWire).
    Mute,
    /// Raise the default-sink volume by 5% (capped at 100%).
    VolUp,
    /// Lower the default-sink volume by 5%.
    VolDown,
}

pub fn run(action: MediaAction) -> i32 {
    match action {
        MediaAction::PlayPause => mpris(PlayerCmd::PlayPause),
        MediaAction::Next => mpris(PlayerCmd::Next),
        MediaAction::Prev => mpris(PlayerCmd::Prev),
        MediaAction::Mute => wpctl(&["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]),
        MediaAction::VolUp => {
            wpctl(&["set-volume", "@DEFAULT_AUDIO_SINK@", "5%+", "-l", "1.0"])
        }
        MediaAction::VolDown => wpctl(&["set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"]),
    }
}

// -----------------------------------------------------------------------------
// PipeWire (mute / volume) — default sink, via wpctl.
// -----------------------------------------------------------------------------

fn wpctl(args: &[&str]) -> i32 {
    match Command::new("wpctl").args(args).status() {
        Ok(s) if s.success() => 0,
        Ok(s) => {
            eprintln!("solactl media: wpctl {args:?} exited with {s}");
            3
        }
        Err(e) => {
            eprintln!("solactl media: failed to run wpctl (is it installed?): {e}");
            3
        }
    }
}

// -----------------------------------------------------------------------------
// MPRIS (play-pause / next / prev) — active session-bus player.
// -----------------------------------------------------------------------------

enum PlayerCmd {
    PlayPause,
    Next,
    Prev,
}

#[zbus::proxy(
    interface = "org.mpris.MediaPlayer2.Player",
    default_path = "/org/mpris/MediaPlayer2"
)]
trait Player {
    fn play_pause(&self) -> zbus::Result<()>;
    fn next(&self) -> zbus::Result<()>;
    fn previous(&self) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn can_control(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn can_go_next(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn can_go_previous(&self) -> zbus::Result<bool>;
}

fn mpris(cmd: PlayerCmd) -> i32 {
    let conn = match zbus::blocking::Connection::session() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("solactl media: connect session bus: {e}");
            return 3;
        }
    };
    let dbus = match zbus::blocking::fdo::DBusProxy::new(&conn) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("solactl media: DBus proxy: {e}");
            return 3;
        }
    };
    let names = match dbus.list_names() {
        Ok(n) => n,
        Err(e) => {
            eprintln!("solactl media: list bus names: {e}");
            return 3;
        }
    };

    // Build a proxy for every controllable MPRIS player, then prefer one
    // that is actually Playing — so play/pause hits the audible player even
    // when several are registered — else fall back to the first found.
    let mut candidates: Vec<(PlayerProxyBlocking, bool)> = Vec::new();
    for name in names {
        let name = name.as_str();
        if !name.starts_with("org.mpris.MediaPlayer2.") {
            continue;
        }
        let proxy = match PlayerProxyBlocking::builder(&conn).destination(name.to_owned()) {
            Ok(builder) => match builder.build() {
                Ok(p) => p,
                Err(_) => continue,
            },
            Err(_) => continue,
        };
        if proxy.can_control().unwrap_or(false) {
            let playing = proxy
                .playback_status()
                .map(|s| s == "Playing")
                .unwrap_or(false);
            candidates.push((proxy, playing));
        }
    }

    let Some((player, _)) = candidates.iter().find(|(_, playing)| *playing).or_else(|| candidates.first())
    else {
        // No controllable player present — a no-op, not an error.
        eprintln!("solactl media: no MPRIS player available");
        return 0;
    };

    let res = match cmd {
        PlayerCmd::PlayPause => player.play_pause(),
        PlayerCmd::Next => {
            if !player.can_go_next().unwrap_or(true) {
                return 0;
            }
            player.next()
        }
        PlayerCmd::Prev => {
            if !player.can_go_previous().unwrap_or(true) {
                return 0;
            }
            player.previous()
        }
    };
    if let Err(e) = res {
        eprintln!("solactl media: player command failed: {e}");
        return 3;
    }
    0
}
