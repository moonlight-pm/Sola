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
set -g set-titles off
set -g allow-passthrough on
";

/// Remove a stale tmux socket left behind by a crashed or killed server.
/// Without this, all tmux commands fail with "server exited unexpectedly"
/// until someone manually deletes the socket file.
pub fn cleanup_stale_socket() {
    // Check if the server is alive by running a harmless command
    let alive = tmux_cmd_raw()
        .args(["ls"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if alive {
        return;
    }

    // Server is dead — check for a stale socket and remove it
    let uid = unsafe { libc::getuid() };
    let socket_path = PathBuf::from(format!("/tmp/tmux-{uid}/{TMUX_SOCKET}"));
    if socket_path.exists() {
        tracing::info!("Removing stale tmux socket: {}", socket_path.display());
        let _ = std::fs::remove_file(&socket_path);
    }
}

/// Kill any orphaned tmux client processes from a previous Sola run.
/// Uses `tmux list-clients` to get only client PIDs — never the server.
/// Previously used `pgrep -f` which matched both client AND server processes
/// (they share identical command lines), killing the server and destroying
/// all sessions.
pub fn kill_orphaned_clients() {
    let output = tmux_cmd_raw()
        .args(["list-clients", "-F", "#{client_pid}"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();

    if let Ok(out) = output {
        let pids: Vec<i32> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.trim().parse().ok())
            .collect();
        for pid in pids {
            tracing::info!("Killing orphaned tmux client pid={pid}");
            unsafe {
                libc::kill(pid, libc::SIGTERM);
                libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG);
            }
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

pub fn list_sessions() -> Vec<String> {
    let output = tmux_cmd()
        .args(["ls", "-F", "#{session_name}"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.starts_with("sola-"))
            .map(String::from)
            .collect(),
        _ => Vec::new(),
    }
}

/// Return all sola session names with their pane's current working directory.
pub fn list_session_paths() -> Vec<(String, String)> {
    let output = tmux_cmd_raw()
        .args([
            "list-sessions",
            "-F",
            "#{session_name} #{pane_current_path}",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if !line.starts_with("sola-") {
                    return None;
                }
                let idx = line.find(' ')?;
                Some((line[..idx].to_string(), line[idx + 1..].to_string()))
            })
            .collect(),
        _ => Vec::new(),
    }
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
