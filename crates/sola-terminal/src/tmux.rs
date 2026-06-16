use std::path::PathBuf;

const TMUX_SOCKET: &str = "sola";

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

const TMUX_CONF: &str = "\
set -g status off
set -g prefix None
unbind -a
set -g mouse off
set -g history-limit 10000
set -g default-terminal xterm-256color
set -g escape-time 0
set -g extended-keys on
set -g extended-keys-format csi-u
set -as terminal-features 'xterm*:extkeys'
set -ga terminal-overrides ',*:smcup@:rmcup@'
set -g set-titles off
set -g allow-passthrough on
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
    let socket_path = PathBuf::from(format!("/tmp/tmux-{uid}/{TMUX_SOCKET}"));
    if socket_path.exists() {
        tracing::info!("Removing stale tmux socket: {}", socket_path.display());
        let _ = std::fs::remove_file(&socket_path);
    }
}

/// Kill any orphaned tmux client processes from previous Sola runs.
///
/// Scans /proc for processes whose kernel-set name is `tmux: client` and whose
/// cmdline mentions our socket (`-L sola`). `tmux list-clients` misses "ghost"
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
            .any(|w| w[0] == b"-L" && w[1] == TMUX_SOCKET.as_bytes());
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
    cmd.args(["-L", TMUX_SOCKET]);
    cmd
}

pub fn tmux_cmd() -> std::process::Command {
    let mut cmd = std::process::Command::new("tmux");
    cmd.args(["-L", TMUX_SOCKET]);
    if let Ok(conf) = ensure_config() {
        cmd.args(["-f", &*conf.to_string_lossy()]);
    }
    cmd
}

pub fn session_name(id: &str) -> String {
    format!("sola-{id}")
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
        .args(["--user", "is-active", "--quiet", "sola-tmux.service"])
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
            "--unit=sola-tmux.service",
            "--description=tmux daemon for sola-terminal",
            "--property=Type=oneshot",
            "--property=RemainAfterExit=yes",
            "--property=KillMode=none",
            "--",
            "tmux",
            "-L",
            TMUX_SOCKET,
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
            tracing::info!("started sola-tmux.service");
        }
        Ok(s) => {
            tracing::warn!("systemd-run sola-tmux.service exited with {:?}", s.code());
        }
        Err(e) => {
            tracing::warn!("failed to start sola-tmux.service: {e}");
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
                .filter(|l| l.starts_with("sola-"))
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
}
