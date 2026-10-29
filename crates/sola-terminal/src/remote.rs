//! tmux on another computer, over ssh ([`sola_core::ssh`]).
//!
//! A tab of a project on another computer is a session of a tmux there, so
//! it runs there and outlives the connection. That server is this
//! process's socket name plus `-remote` (`sola-ws-remote` for Projects): a
//! Sola running on that computer keeps its own sessions on its own socket,
//! and the two never meet. Session names are the same as here
//! (`sws-<pane>`). Each attach first writes our tmux.conf there
//! (`~/.config/sola/<socket>.tmux.conf`) and sources it into a running
//! server, so the remote tmux draws like the local one (no status bar, no
//! prefix, extended keys).

use std::process::Command;

use base64::Engine;
use sola_core::ssh;

use crate::tmux::TMUX_CONF;

/// The tmux socket on the other computer.
pub fn socket() -> String {
    format!("{}-remote", crate::tmux::socket())
}

/// A tab's ssh: attach `session` on `host`, or make it in `dir`, running
/// `exec` there when given (else the login shell). `env` is stamped on the
/// session either way, as [`crate::pty::PtyBackend::spawn_or_attach_with_env`]
/// does here.
pub fn attach(
    host: &str,
    session: &str,
    dir: &str,
    env: &[(&str, &str)],
    exec: &[&str],
) -> std::io::Result<Command> {
    let host = ssh::check_host(host).map_err(std::io::Error::other)?;
    if !is_word(session) || !env.iter().all(|(k, _)| is_word(k)) {
        return Err(std::io::Error::other(format!(
            "tmux session {session:?} or its env names are not one word"
        )));
    }
    Ok(ssh::interactive(
        host,
        &attach_script(&socket(), session, dir, env, &sh_line(exec)),
    ))
}

/// `session`'s screen and scrollback on `host` ([`crate::tmux::capture_scrollback`]).
pub fn capture_scrollback(host: &str, session: &str) -> Result<String, String> {
    let out = tmux(host, &["capture-pane", "-t", &pane(session)?, "-p", "-S", "-"], None)?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Type `text` into `session` on `host` ([`crate::tmux::send_literal`]).
pub fn send_literal(host: &str, session: &str, text: &str) -> Result<(), String> {
    tmux(host, &["send-keys", "-t", &pane(session)?, "-l", "--", text], None).map(|_| ())
}

/// Paste `text` into `session` on `host`, then Enter if asked
/// ([`crate::tmux::send_prompt`]): one round trip, the text on stdin.
pub fn send_prompt(host: &str, session: &str, text: &str, enter: bool) -> Result<(), String> {
    let target = pane(session)?;
    let t = format!("tmux -L {}", socket());
    let buf = format!("sws-paste-{session}");
    let mut steps = Vec::new();
    if !text.is_empty() {
        steps.push(format!("{t} load-buffer -b {buf} -"));
        steps.push(format!("{t} paste-buffer -dp -b {buf} -t {target}"));
        if enter {
            let ms = crate::tmux::PASTE_SETTLE.as_millis();
            steps.push(format!("sleep 0.{ms:03}"));
        }
    }
    if enter {
        steps.push(format!("{t} send-keys -t {target} Enter"));
    }
    if steps.is_empty() {
        return Ok(());
    }
    let out = ssh::run(ssh::batch(host, &steps.join(" && ")), Some(text.as_bytes()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("tmux on {host}: {}", ssh::reason(&out.stderr)))
    }
}

/// Set `key` in `session`'s environment on `host`.
pub fn set_environment(host: &str, session: &str, key: &str, value: &str) -> Result<(), String> {
    if !is_word(session) {
        return Err(format!("tmux session {session:?} is not one word"));
    }
    tmux(host, &["set-environment", "-t", &format!("={session}"), key, value], None).map(|_| ())
}

/// `tmux -L <socket> <args>` on `host`; a failure says what tmux said.
fn tmux(host: &str, args: &[&str], stdin: Option<&[u8]>) -> Result<std::process::Output, String> {
    let socket = socket();
    let mut argv = vec!["tmux", "-L", socket.as_str()];
    argv.extend_from_slice(args);
    let out = ssh::run(ssh::batch(host, &ssh::argv_script(None, &[], &argv)), stdin)?;
    if out.status.success() {
        Ok(out)
    } else {
        Err(format!("tmux on {host}: {}", ssh::reason(&out.stderr)))
    }
}

/// `=session:`, the session's own pane ([`crate::tmux::pane_target`]).
fn pane(session: &str) -> Result<String, String> {
    if is_word(session) {
        Ok(format!("={session}:"))
    } else {
        Err(format!("tmux session {session:?} is not one word"))
    }
}

/// Whether `session` is alive on `host`. `Err`: why that can't be known
/// (ssh failed, or there is no tmux there).
pub fn session_state(host: &str, session: &str) -> Result<bool, String> {
    if !is_word(session) {
        return Ok(false);
    }
    let script = format!(
        "command -v tmux >/dev/null || exit 3; tmux -L {} has-session -t ={session} 2>/dev/null",
        socket()
    );
    let out = ssh::run(ssh::batch(host, &script), None)?;
    match out.status.code() {
        Some(0) => Ok(true),
        Some(3) => Err(format!("tmux is not installed on {host}")),
        _ => Ok(false),
    }
}

/// Kill `session` on `host`. Waits on ssh: keep it off the UI thread.
pub fn kill_session(host: &str, session: &str) {
    if !is_word(session) {
        return;
    }
    let script = format!(
        "tmux -L {} kill-session -t ={session} 2>/dev/null; true",
        socket()
    );
    match ssh::run(ssh::batch(host, &script), None) {
        Ok(_) => tracing::debug!(%host, %session, "killed remote tmux session"),
        Err(e) => tracing::warn!(%host, %session, "could not kill remote tmux session: {e}"),
    }
}

/// The script a tab runs there. The config, the folder and the command
/// ride in base64, so the script stays one quotable word
/// ([`ssh::is_quotable`]).
fn attach_script(
    socket: &str,
    session: &str,
    dir: &str,
    env: &[(&str, &str)],
    exec: &str,
) -> String {
    let b64 = base64::engine::general_purpose::STANDARD;
    let conf = b64.encode(TMUX_CONF);
    let dir = b64.encode(dir);
    let exec = b64.encode(exec);
    let t = format!("tmux -L {socket} -f \"$c\"");
    // Env values decode into e0, e1, …; then `-e` for a new session,
    // `set-environment` for one there already.
    let decode: String = env
        .iter()
        .enumerate()
        .map(|(i, (_, v))| format!("e{i}=$(echo {} | base64 -d); ", b64.encode(v)))
        .collect();
    let stamp: String = env
        .iter()
        .enumerate()
        .map(|(i, (k, _))| format!("{t} set-environment -t ={session} {k} \"$e{i}\"; "))
        .collect();
    let flags: String = env
        .iter()
        .enumerate()
        .map(|(i, (k, _))| format!(" -e \"{k}=$e{i}\""))
        .collect();
    let decode = decode.trim_end_matches("; ").to_string();
    [
        format!("c=\"$HOME/.config/sola/{socket}.tmux.conf\""),
        "mkdir -p \"${c%/*}\"".to_string(),
        format!("echo {conf} | base64 -d > \"$c\" || exit 1"),
        concat!(
            "command -v tmux >/dev/null || ",
            "{ echo \"tmux is not installed on $(uname -n)\" >&2; exit 127; }"
        )
        .to_string(),
        format!("{t} source-file \"$c\" 2>/dev/null"),
        if decode.is_empty() { "true".to_string() } else { decode },
        format!(
            "{t} has-session -t ={session} 2>/dev/null && {{ {stamp}exec {t} attach-session -t ={session}; }}"
        ),
        format!("d=$(echo {dir} | base64 -d)"),
        format!("x=$(echo {exec} | base64 -d)"),
        format!("[ -n \"$x\" ] && exec {t} new-session -s {session} -c \"$d\"{flags} \"$x\""),
        format!("exec {t} new-session -s {session} -c \"$d\"{flags}"),
    ]
    .join("; ")
}

/// Letters, digits, `-` and `_`: safe bare in a script, and a tmux name.
fn is_word(s: &str) -> bool {
    !s.is_empty()
        && s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// `argv` as one sh command line.
fn sh_line(argv: &[&str]) -> String {
    argv.iter()
        .map(|a| sh_quote(a))
        .collect::<Vec<_>>()
        .join(" ")
}

fn sh_quote(arg: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c);
    if !arg.is_empty() && arg.chars().all(plain) {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_attach_script_is_one_quotable_word() {
        let s = attach_script(
            "sola-ws-remote",
            "sws-ws-novus-sola",
            "/home/j/Projects/It's \"here\"",
            &[("SOLA_PANE_ID", "ws-novus-sola"), ("SOLA_WS_PATH", "/it's")],
            &sh_line(&["claude", "--resume", "a b"]),
        );
        assert!(ssh::is_quotable(&s), "{s}");
        assert!(s.contains("attach-session -t =sws-ws-novus-sola"));
        assert!(s.contains("new-session -s sws-ws-novus-sola -c \"$d\" -e \"SOLA_PANE_ID=$e0\" -e \"SOLA_WS_PATH=$e1\""));
        assert!(s.contains("set-environment -t =sws-ws-novus-sola SOLA_WS_PATH \"$e1\""));
    }

    #[test]
    fn the_attach_script_runs_under_sh() {
        // Up to the tmux call: config written, folder and command decoded.
        let home = std::env::temp_dir().join(format!("sola-remote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let script = attach_script("t", "s", "/a dir/it's", &[("K", "v w")], &sh_line(&["echo", "it's"]))
            .replace("command -v tmux >/dev/null", "true")
            .replace("tmux -L t -f \"$c\"", "echo")
            .replace("exec echo", "echo");
        let out = Command::new("sh")
            .args(["-c", &script])
            .env("HOME", &home)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let conf = std::fs::read_to_string(home.join(".config/sola/t.tmux.conf")).unwrap();
        assert_eq!(conf, TMUX_CONF);
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(said.contains("new-session -s s -c /a dir/it's -e K=v w echo 'it'\\''s'"), "{said}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn quoting_keeps_plain_words_bare() {
        assert_eq!(sh_line(&["grok", "-r", "abc-123"]), "grok -r abc-123");
        assert_eq!(sh_quote("a b"), "'a b'");
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
        assert_eq!(sh_quote(""), "''");
        assert_eq!(sh_line(&[]), "");
    }

    #[test]
    fn session_names_are_words() {
        assert!(is_word("sws-ws-novus-sola_2"));
        assert!(!is_word("sws ws"));
        assert!(!is_word("sws;rm"));
        assert!(!is_word(""));
    }
}
