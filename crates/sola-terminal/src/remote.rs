//! tmux on another computer, over ssh ([`sola_core::ssh`]).
//!
//! A tab of a project on another computer is a session of a tmux there, so
//! it runs there and outlives the connection. That server is the same one
//! a Sola on that computer keeps its own tabs on ([`socket`], `sola-ws` for
//! Projects): a session belongs to the computer it runs on, whichever
//! computer opened it, and any of them can open it again (sessions
//! anywhere, 2026-10-30). Sessions made before that live on
//! [`legacy_socket`] (`sola-ws-remote`) until they end; every command here
//! finds a session on either ([`pick`]), and nothing new is made there.
//! Session names are the same as here (`sws-<pane>`). Each attach first
//! writes our tmux.conf there (`~/.config/sola/<socket>.tmux.conf`) and
//! sources it into a running server, so the remote tmux draws like the
//! local one (no status bar, no prefix, extended keys).

use std::process::Command;

use base64::Engine;
use sola_core::ssh;

use crate::tmux::TMUX_CONF;

/// The tmux socket on the other computer: the one its own tabs are on.
pub fn socket() -> String {
    crate::tmux::socket().to_string()
}

/// Where sessions opened from another computer lived before 2026-10-30.
pub fn legacy_socket() -> String {
    format!("{}-remote", crate::tmux::socket())
}

/// A shell line that sets `L` to the socket `target` (a tmux `-t`, such as
/// `=sws-x` or `"=$s"`) is on: [`socket`], unless only [`legacy_socket`]
/// has it.
pub fn pick(target: &str) -> String {
    pick_on(&socket(), &legacy_socket(), target)
}

fn pick_on(socket: &str, legacy: &str, target: &str) -> String {
    format!(
        "L={socket}; tmux -L {socket} has-session -t {target} 2>/dev/null || \
         {{ tmux -L {legacy} has-session -t {target} 2>/dev/null && L={legacy}; }};"
    )
}

/// A tab's ssh: attach `session` on `host`, or make it in `dir`, running
/// `exec` there when given (else the login shell), with `env` in its
/// environment and `opts` (tmux user options, `@…`) set on it. A session
/// that is there already is attached as it is: another computer may have
/// made it, and its environment is its own.
pub fn attach(
    host: &str,
    session: &str,
    dir: &str,
    env: &[(&str, &str)],
    opts: &[(&str, &str)],
    exec: &[&str],
) -> std::io::Result<Command> {
    let host = ssh::check_host(host).map_err(std::io::Error::other)?;
    let option_name = |k: &str| k.strip_prefix('@').is_some_and(is_word);
    if !is_word(session)
        || !env.iter().all(|(k, _)| is_word(k))
        || !opts.iter().all(|(k, _)| option_name(k))
    {
        return Err(std::io::Error::other(format!(
            "tmux session {session:?} or its env names are not one word"
        )));
    }
    Ok(ssh::interactive(
        host,
        &attach_script(&socket(), &legacy_socket(), session, dir, env, opts, &sh_line(exec)),
    ))
}

/// `session`'s screen and scrollback on `host` ([`crate::tmux::capture_scrollback`]).
pub fn capture_scrollback(host: &str, session: &str) -> Result<String, String> {
    let out = tmux(host, session, &["capture-pane", "-t", &pane(session)?, "-p", "-S", "-"], None)?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Type `text` into `session` on `host` ([`crate::tmux::send_literal`]).
pub fn send_literal(host: &str, session: &str, text: &str) -> Result<(), String> {
    tmux(host, session, &["send-keys", "-t", &pane(session)?, "-l", "--", text], None).map(|_| ())
}

/// Paste `text` into `session` on `host`, then Enter if asked
/// ([`crate::tmux::send_prompt`]): one round trip, the text on stdin.
pub fn send_prompt(host: &str, session: &str, text: &str, enter: bool) -> Result<(), String> {
    let target = pane(session)?;
    let t = "tmux -L \"$L\"";
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
    let script = format!("{} {}", pick(&format!("={session}")), steps.join(" && "));
    let out = ssh::run(ssh::batch(host, &script), Some(text.as_bytes()))?;
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
    tmux(host, session, &["set-environment", "-t", &format!("={session}"), key, value], None)
        .map(|_| ())
}

/// `tmux <args>` on `host`, on the socket `session` is on ([`pick`]); a
/// failure says what tmux said. `session` is one word (its callers check).
fn tmux(
    host: &str,
    session: &str,
    args: &[&str],
    stdin: Option<&[u8]>,
) -> Result<std::process::Output, String> {
    let script = format!("{} tmux -L \"$L\" {}", pick(&format!("={session}")), sh_line(args));
    let out = ssh::run(ssh::batch(host, &script), stdin)?;
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
        "command -v tmux >/dev/null || exit 3; tmux -L {} has-session -t ={session} 2>/dev/null \
         || tmux -L {} has-session -t ={session} 2>/dev/null",
        socket(),
        legacy_socket()
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
        "tmux -L {} kill-session -t ={session} 2>/dev/null; \
         tmux -L {} kill-session -t ={session} 2>/dev/null; true",
        socket(),
        legacy_socket()
    );
    match ssh::run(ssh::batch(host, &script), None) {
        Ok(_) => tracing::debug!(%host, %session, "killed remote tmux session"),
        Err(e) => tracing::warn!(%host, %session, "could not kill remote tmux session: {e}"),
    }
}

/// The script a tab runs there. The config, the folder, the command and
/// every value ride in base64, so the script stays one quotable word
/// ([`ssh::is_quotable`]). A new session is made detached, its options
/// set, then attached.
fn attach_script(
    socket: &str,
    legacy: &str,
    session: &str,
    dir: &str,
    env: &[(&str, &str)],
    opts: &[(&str, &str)],
    exec: &str,
) -> String {
    let b64 = base64::engine::general_purpose::STANDARD;
    let conf = b64.encode(TMUX_CONF);
    let dir = b64.encode(dir);
    let exec = b64.encode(exec);
    let t = "tmux -L \"$L\" -f \"$c\"";
    // Env values decode into e0, e1, …, option values into o0, o1, ….
    let decode: String = env
        .iter()
        .enumerate()
        .map(|(i, (_, v))| format!("e{i}=$(echo {} | base64 -d); ", b64.encode(v)))
        .chain(
            opts.iter()
                .enumerate()
                .map(|(i, (_, v))| format!("o{i}=$(echo {} | base64 -d); ", b64.encode(v))),
        )
        .collect();
    let flags: String = env
        .iter()
        .enumerate()
        .map(|(i, (k, _))| format!(" -e \"{k}=$e{i}\""))
        .collect();
    let stamp: String = opts
        .iter()
        .enumerate()
        .map(|(i, (k, _))| format!("{t} set-option -t ={session} {k} \"$o{i}\"; "))
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
        pick_on(socket, legacy, &format!("={session}")).trim_end_matches(';').to_string(),
        format!("{t} source-file \"$c\" 2>/dev/null"),
        if decode.is_empty() { "true".to_string() } else { decode },
        format!("d=$(echo {dir} | base64 -d)"),
        format!("x=$(echo {exec} | base64 -d)"),
        format!(
            "{t} has-session -t ={session} 2>/dev/null || {{ \
             if [ -n \"$x\" ]; then {t} new-session -d -s {session} -c \"$d\"{flags} \"$x\"; \
             else {t} new-session -d -s {session} -c \"$d\"{flags}; fi || exit 1; {stamp}}}"
        ),
        format!("{t} set-window-option -u -t ={session}: window-size 2>/dev/null"),
        format!("exec {t} attach-session -t ={session}"),
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
            "sola-ws",
            "sola-ws-remote",
            "sws-ws-novus-sola",
            "/home/j/Projects/It's \"here\"",
            &[("SOLA_PANE_ID", "ws-novus-sola"), ("SOLA_WS_PATH", "/it's")],
            &[("@sola-project", "/home/j/Projects/It's"), ("@sola-name", "a b")],
            &sh_line(&["claude", "--resume", "a b"]),
        );
        assert!(ssh::is_quotable(&s), "{s}");
        assert!(s.contains("exec tmux -L \"$L\" -f \"$c\" attach-session -t =sws-ws-novus-sola"));
        assert!(s.contains("new-session -d -s sws-ws-novus-sola -c \"$d\" -e \"SOLA_PANE_ID=$e0\" -e \"SOLA_WS_PATH=$e1\""));
        assert!(s.contains("set-option -t =sws-ws-novus-sola @sola-name \"$o1\""));
        assert!(!s.contains("set-environment"), "an existing session's env is its own: {s}");
    }

    #[test]
    fn the_attach_script_runs_under_sh() {
        // Up to the tmux call: config written, folder and command decoded.
        let home = std::env::temp_dir().join(format!("sola-remote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let script = attach_script(
            "t",
            "u",
            "s",
            "/a dir/it's",
            &[("K", "v w")],
            &[("@o", "p q")],
            &sh_line(&["echo", "it's"]),
        )
        .replace("command -v tmux >/dev/null", "true")
        .replace("tmux -L t has-session", "false")
        .replace("tmux -L u has-session", "false")
        .replace("tmux -L \"$L\" -f \"$c\" has-session", "false")
        .replace("tmux -L \"$L\" -f \"$c\"", "echo")
        .replace("exec echo", "echo");
        let out = Command::new("sh")
            .args(["-c", &script])
            .env("HOME", &home)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let conf = std::fs::read_to_string(home.join(".config/sola/t.tmux.conf")).unwrap();
        assert!(said_l(&script), "{script}");
        assert_eq!(conf, TMUX_CONF);
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(said.contains("new-session -d -s s -c /a dir/it's -e K=v w echo 'it'\\''s'"), "{said}");
        assert!(said.contains("set-option -t =s @o p q"), "{said}");
        assert!(said.contains("attach-session -t =s"), "{said}");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// The attach script picks its socket before any tmux call on it.
    fn said_l(script: &str) -> bool {
        script.find("L=t;").is_some_and(|i| i < script.find("echo source-file").unwrap_or(0))
    }

    #[test]
    fn a_session_is_found_on_either_socket() {
        let line = pick_on("sola-ws", "sola-ws-remote", "=sws-x");
        let run = |new: bool, old: bool| {
            let script = line
                .replace("tmux -L sola-ws has-session -t =sws-x", if new { "true" } else { "false" })
                .replace("tmux -L sola-ws-remote has-session -t =sws-x", if old { "true" } else { "false" });
            let out = Command::new("sh").args(["-c", &format!("{script} echo $L")]).output().unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        assert_eq!(run(false, false), "sola-ws");
        assert_eq!(run(true, false), "sola-ws");
        assert_eq!(run(false, true), "sola-ws-remote");
        assert_eq!(run(true, true), "sola-ws");
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
