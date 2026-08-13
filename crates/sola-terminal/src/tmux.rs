use std::path::PathBuf;
use std::sync::OnceLock;

/// Which tmux server this process talks to.
///
/// `sola-terminal` keeps the historical defaults (`sola` / `sola-tmux.service`
/// / `sola-` sessions). Other apps that reuse this crate as a library must
/// call [`configure`] **before** any tmux helper so they do not share that
/// server or collide on session names.
pub struct TmuxIdentity {
    pub socket: &'static str,
    pub unit: &'static str,
    pub session_prefix: &'static str,
}

const DEFAULT_IDENTITY: TmuxIdentity = TmuxIdentity {
    socket: "sola",
    unit: "sola-tmux.service",
    session_prefix: "sola-",
};

static IDENTITY: OnceLock<TmuxIdentity> = OnceLock::new();

/// Pin the tmux socket / unit / session prefix for this process.
/// First call wins; later calls are ignored so a library consumer cannot
/// clobber the binary that already configured itself.
pub fn configure(socket: &'static str, unit: &'static str, session_prefix: &'static str) {
    let _ = IDENTITY.set(TmuxIdentity {
        socket,
        unit,
        session_prefix,
    });
}

fn identity() -> &'static TmuxIdentity {
    IDENTITY.get().unwrap_or(&DEFAULT_IDENTITY)
}

fn config_path() -> PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".config")
        });
    config_dir.join("sola").join("tmux.conf")
}

pub fn ensure_config() -> Result<PathBuf, std::io::Error> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, TMUX_CONF)?;
    Ok(path)
}

/// Reload the config on an already-running tmux server.
/// The -f flag on tmux_cmd only applies at server startup, so if the server
/// was started with an older config, new options won't take effect without this.
pub fn reload_config() {
    if let Ok(conf) = ensure_config() {
        let _ = tmux_cmd_raw()
            .args(["source-file", &conf.to_string_lossy()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

// TERM_PROGRAM=alacritty: Sola's grid is alacritty_terminal with
// kitty_keyboard=true. Apps like Grok only *request* the kitty keyboard
// protocol when they recognise a KKP-capable host. COLORTERM unlocks truecolor.
//
// extended-keys always (not "on"): Grok/Claude request extended keys via the
// kitty push sequence, which tmux does not implement — under "on" the request
// is ignored and CSI-u from the outer terminal is never accepted, so
// Shift+Enter collapses to plain Enter. "always" keeps CSI-u acceptance on
// permanently; our encoder always emits CSI-u for Shift/Ctrl+Enter to match.
//
// update-environment pulls the client-side values into the session on
// create/attach; set-environment -g seeds the global env for new panes even
// when a session already exists.
const TMUX_CONF: &str = "\
set -g status off
set -g prefix None
unbind -a
set -g mouse off
set -g history-limit 10000
set -g default-terminal xterm-256color
set -g escape-time 0
set -g extended-keys always
set -g extended-keys-format csi-u
set -as terminal-features 'xterm*:extkeys'
set -ga terminal-overrides ',*:smcup@:rmcup@'
set -g set-titles off
set -g allow-passthrough on
set -ga update-environment ' TERM_PROGRAM TERM_PROGRAM_VERSION COLORTERM'
set-environment -g TERM_PROGRAM alacritty
set-environment -g COLORTERM truecolor
";

/// Remove a stale tmux socket left behind by a crashed or killed server.
/// Without this, all tmux commands fail with "server exited unexpectedly"
/// until someone manually deletes the socket file.
///
/// Only removes the socket when tmux explicitly reports "no server running"
/// on stderr. Any other failure (tmux not spawnable, unknown error) is left
/// alone so we don't destroy a healthy server's socket on a transient error.
pub fn cleanup_stale_socket() {
    let output = match tmux_cmd_raw().args(["ls"]).output() {
        Ok(out) => out,
        Err(e) => {
            tracing::warn!("tmux ls failed to spawn, leaving socket alone: {e}");
            return;
        }
    };

    if output.status.success() {
        return;
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.contains("no server running") {
        tracing::warn!(
            "tmux ls failed unexpectedly, leaving socket alone: {}",
            stderr.trim()
        );
        return;
    }

    let uid = unsafe { libc::getuid() };
    let socket_path = PathBuf::from(format!("/tmp/tmux-{uid}/{}", identity().socket));
    if socket_path.exists() {
        tracing::info!("Removing stale tmux socket: {}", socket_path.display());
        let _ = std::fs::remove_file(&socket_path);
    }
}

/// Kill any orphaned tmux client processes from previous Sola runs.
///
/// Scans /proc for processes whose kernel-set name is `tmux: client` and whose
/// cmdline mentions our socket (`-L <socket>`). `tmux list-clients` misses "ghost"
/// clients whose server-side connection has already been closed — those are
/// stuck in poll() forever, invisible to the server, but still alive. Reading
/// /proc/*/status finds them by name regardless of connection state.
///
/// We distinguish clients from the server via the kernel name (`tmux: server`
/// vs `tmux: client`), so this can never kill the live server.
pub fn kill_orphaned_clients() {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return;
    };
    let self_pid = std::process::id() as i32;

    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        if pid == self_pid {
            continue;
        }

        let status_path = entry.path().join("status");
        let Ok(status) = std::fs::read_to_string(&status_path) else {
            continue;
        };
        let is_client = status.lines().any(|l| l == "Name:\ttmux: client");
        if !is_client {
            continue;
        }

        let cmdline_path = entry.path().join("cmdline");
        let Ok(cmdline) = std::fs::read(&cmdline_path) else {
            continue;
        };
        // cmdline args are NUL-separated; scan for the socket flag pair.
        let parts: Vec<&[u8]> = cmdline.split(|&b| b == 0).collect();
        let matches_socket = parts
            .windows(2)
            .any(|w| w[0] == b"-L" && w[1] == identity().socket.as_bytes());
        if !matches_socket {
            continue;
        }

        tracing::info!("Killing orphaned tmux client pid={pid}");
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
}

/// Raw tmux command without config file (used for health checks where
/// config doesn't matter and ensure_config side effects aren't wanted).
fn tmux_cmd_raw() -> std::process::Command {
    let mut cmd = std::process::Command::new("tmux");
    cmd.args(["-L", identity().socket]);
    cmd
}

pub fn tmux_cmd() -> std::process::Command {
    let mut cmd = std::process::Command::new("tmux");
    cmd.args(["-L", identity().socket]);
    if let Ok(conf) = ensure_config() {
        cmd.args(["-f", &*conf.to_string_lossy()]);
    }
    cmd
}

pub fn session_name(id: &str) -> String {
    format!("{}{id}", identity().session_prefix)
}

/// Inverse of [`session_name`] when `session` uses this process's prefix.
pub fn pane_id_from_session(session: &str) -> Option<String> {
    session
        .strip_prefix(identity().session_prefix)
        .filter(|id| !id.is_empty())
        .map(String::from)
}

/// Sessions on our socket with last-activity time (unix seconds).
pub fn list_sessions_activity() -> Option<Vec<(String, u64)>> {
    let output = tmux_cmd()
        .args(["ls", "-F", "#{session_name} #{session_activity}"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("no server running") || stderr.contains("error connecting to") {
            return Some(Vec::new());
        }
        tracing::warn!("tmux ls (activity) failed: {}", stderr.trim());
        return None;
    }
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let (name, activity) = line.rsplit_once(' ')?;
                if !name.starts_with(identity().session_prefix) {
                    return None;
                }
                Some((name.to_string(), activity.parse().ok()?))
            })
            .collect(),
    )
}

pub fn rename_session(from: &str, to: &str) -> bool {
    if from == to {
        return true;
    }
    tmux_cmd()
        .args(["rename-session", "-t", from, to])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn capture_scrollback(session: &str) -> Result<String, String> {
    let output = tmux_cmd()
        .args(["capture-pane", "-t", session, "-p", "-S", "-"])
        .output()
        .map_err(|e| format!("tmux capture-pane failed: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "tmux capture-pane exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn kill_session(session: &str) {
    let _ = tmux_cmd()
        .args(["kill-session", "-t", session])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Return the set of live sola-* tmux session names.
///
/// `None` means the query failed in a way we can't distinguish from a dead
/// server (command couldn't spawn, or tmux exited with an unexpected error).
/// Callers should treat `None` as "unknown" and NOT drop persisted state on it.
/// `Some(vec![])` means tmux explicitly reported no sessions running.
/// Start the tmux server in its own systemd user service so it
/// survives the sola-terminal scope being torn down on Meta+Q. The
/// `_keepalive` session running `sleep infinity` keeps the server
/// alive between `tmux new-session` invocations and is filtered out
/// of `list_sessions()` by the `sola-` prefix check.
///
/// Uses `Type=oneshot` + `RemainAfterExit=yes` + `KillMode=none`.
/// The first two keep the unit "active" after ExecStart exits;
/// `KillMode=none` is the only way to stop systemd from SIGTERMing
/// the forked daemon (the default `control-group` kills it on
/// ExecStart completion, and `process` for some reason still loses
/// it). We never explicitly stop this unit, so KillMode=none is
/// safe — logout/reboot tears down the user manager and the daemon
/// with it.
///
/// Idempotent: skips if the unit is already active. Without this,
/// tmux daemonizes inside the sola-app scope's cgroup, and systemd
/// reaps it (and every shell inside) when the scope stops.
pub fn ensure_server_running() {
    let active = std::process::Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", identity().unit])
        .status();
    if matches!(active, Ok(s) if s.success()) {
        return;
    }

    // Resolve `sleep` to an absolute path. Systemd transient services
    // get a minimal environment, and the `_keepalive` pane spawns its
    // command via the user's login shell — which can't find `sleep` in
    // an empty PATH. Without this, the pane exits 127 immediately, tmux
    // sees an empty session, and (with default `exit-empty=on`) the
    // server self-terminates within milliseconds of starting.
    let sleep_path = sola_core::applications::resolve_in_path("sleep")
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "sleep".to_string());

    let status = std::process::Command::new("systemd-run")
        .args([
            "--user",
            "--quiet",
            "--collect",
            &format!("--unit={}", identity().unit),
            &format!("--description=tmux daemon ({})", identity().socket),
            "--property=Type=oneshot",
            "--property=RemainAfterExit=yes",
            "--property=KillMode=none",
            "--",
            "tmux",
            "-L",
            identity().socket,
            "new-session",
            "-d",
            "-s",
            "_keepalive",
            &sleep_path,
            "infinity",
        ])
        .status();

    match status {
        Ok(s) if s.success() => {
            tracing::info!(unit = identity().unit, "started tmux systemd unit");
        }
        Ok(s) => {
            tracing::warn!(
                unit = identity().unit,
                "systemd-run exited with {:?}",
                s.code()
            );
        }
        Err(e) => {
            tracing::warn!(unit = identity().unit, "failed to start tmux unit: {e}");
        }
    }
}

pub fn list_sessions() -> Option<Vec<String>> {
    let output = tmux_cmd()
        .args(["ls", "-F", "#{session_name}"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()?;

    if output.status.success() {
        return Some(
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|l| l.starts_with(identity().session_prefix))
                .map(String::from)
                .collect(),
        );
    }

    // "no server running" and "error connecting to <socket>" both mean
    // the tmux server doesn't exist — no live sessions either way. The
    // socket-missing path fires after `cleanup_stale_socket` deletes the
    // socket on startup; without this, reconciliation can't tell the
    // difference between "tmux is gone" and "tmux is broken."
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("no server running") || stderr.contains("error connecting to") {
        Some(Vec::new())
    } else {
        tracing::warn!("tmux ls failed: {}", stderr.trim());
        None
    }
}

/// Best-effort: ask tmux for the foreground process's cwd in this pane.
///
/// Tmux derives this by walking the pane's controlling tty's foreground
/// process group and reading `/proc/<pid>/cwd`, so it works even when the
/// shell never emits OSC 7 (zsh on NixOS, etc). Returns `None` if the
/// session is gone or the path is empty.
pub fn pane_current_path(session: &str) -> Option<String> {
    let output = tmux_cmd()
        .args([
            "display-message",
            "-p",
            "-t",
            session,
            "-F",
            "#{pane_current_path}",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() { None } else { Some(path) }
}

/// Foreground pane pid, if tmux still has the session.
pub fn pane_pid(session: &str) -> Option<i32> {
    let output = tmux_cmd()
        .args([
            "display-message",
            "-p",
            "-t",
            session,
            "-F",
            "#{pane_pid}",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// Stamp a session environment variable (inherited by new panes / shells).
pub fn set_environment(session: &str, key: &str, value: &str) {
    let _ = tmux_cmd()
        .args(["set-environment", "-t", session, key, value])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

pub fn resize_window(session: &str, cols: u16, rows: u16) {
    let _ = tmux_cmd()
        .args([
            "resize-window",
            "-t",
            session,
            "-x",
            &cols.to_string(),
            "-y",
            &rows.to_string(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_name_format() {
        assert_eq!(session_name("abc-123"), "sola-abc-123");
        assert_eq!(
            pane_id_from_session("sola-abc-123").as_deref(),
            Some("abc-123")
        );
        assert_eq!(pane_id_from_session("other"), None);
    }

    #[test]
    fn config_path_under_sola_dir() {
        let path = config_path();
        assert!(path.to_string_lossy().contains("sola/tmux.conf"));
    }

    #[test]
    fn tmux_conf_disables_status() {
        assert!(TMUX_CONF.contains("status off"));
        assert!(TMUX_CONF.contains("prefix None"));
    }

    #[test]
    fn tmux_conf_advertises_kkp_capable_host() {
        // Grok (and peers) only negotiate kitty keyboard / Shift+Enter when
        // the session env identifies a known KKP-capable terminal. CSI-u
        // acceptance must be permanent (`always`) because apps request via
        // kitty push, which tmux ignores under request-based `on`.
        assert!(TMUX_CONF.contains("TERM_PROGRAM alacritty"));
        assert!(TMUX_CONF.contains("COLORTERM truecolor"));
        assert!(TMUX_CONF.contains("update-environment"));
        assert!(TMUX_CONF.contains("extended-keys always"));
        assert!(TMUX_CONF.contains("extended-keys-format csi-u"));
        assert!(TMUX_CONF.contains("xterm*:extkeys"));
    }
}
