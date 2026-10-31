//! ssh to another computer, the way Sola runs it.
//!
//! What runs off a terminal ([`batch`]: the launcher's listing of a host's
//! projects, checking or killing a session there) never prompts: no
//! password, and a host seen for the first time is trusted
//! (`StrictHostKeyChecking=accept-new`; a changed key is still refused).
//! It also leaves a shared connection behind (`ControlMaster`, kept ten
//! minutes after its last use), which a tab's ssh ([`interactive`]) rides
//! when it is there, so a project's tabs open without logging in again. A
//! tab never starts that connection itself: it logs in on its own, and may
//! ask for a password in the tab. Keepalives notice a dead link within a
//! minute.
//!
//! The remote command is a POSIX `sh` script, run as `sh -c '<script>'`
//! whatever the login shell there is. A script holds no `'` and no `\`, so
//! that word reads the same to sh, bash, zsh and fish; anything else rides
//! in base64 or on stdin.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use base64::Engine;

/// How long a shared connection stays after its last use.
const PERSIST: &str = "ControlPersist=10m";
/// A keepalive every 15 s; four unanswered and the link is dead.
const ALIVE: [&str; 4] = [
    "-o",
    "ServerAliveInterval=15",
    "-o",
    "ServerAliveCountMax=4",
];
const CONNECT_TIMEOUT: &str = "ConnectTimeout=10";

/// A host as typed: `name`, `user@name`, or a `~/.ssh/config` alias.
pub fn check_host(raw: &str) -> Result<&str, String> {
    let host = raw.trim();
    if host.is_empty() {
        return Err("Type a computer's name".into());
    }
    let bad = |c: char| c.is_whitespace() || c.is_control() || matches!(c, '\'' | '"' | '\\');
    if host.starts_with('-') || host.chars().any(bad) {
        return Err(format!("“{host}” isn't a host name"));
    }
    Ok(host)
}

/// Where shared connections' sockets live (`$XDG_RUNTIME_DIR/sola-ssh`,
/// 0700). `None`: no runtime dir, so nothing is shared.
fn control_dir() -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let dir = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?).join("sola-ssh");
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).ok()?;
    Some(dir)
}

fn base(master: bool) -> Command {
    let mut cmd = Command::new("ssh");
    match control_dir() {
        Some(dir) => {
            cmd.arg("-o")
                .arg(format!("ControlPath={}/%C", dir.display()));
            if master {
                cmd.args(["-o", "ControlMaster=auto", "-o", PERSIST]);
            } else {
                cmd.args(["-o", "ControlMaster=no"]);
            }
        }
        None => {
            cmd.args(["-o", "ControlMaster=no", "-o", "ControlPath=none"]);
        }
    }
    cmd.args(ALIVE).args(["-o", CONNECT_TIMEOUT]);
    cmd
}

/// ssh to `host` with a terminal, running `script` there (a tab).
pub fn interactive(host: &str, script: &str) -> Command {
    let mut cmd = base(false);
    cmd.args(["-t", "--", host]).arg(sh_word(script));
    cmd
}

/// ssh to `host` running `script` there, never prompting.
pub fn batch(host: &str, script: &str) -> Command {
    let mut cmd = base(true);
    cmd.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-T",
        "--",
        host,
    ])
    .arg(sh_word(script));
    cmd
}

/// `ssh -N` to `host`, forwarding the socket `remote` there to `local`
/// here. A connection of its own, not the shared one, so the forward lives
/// exactly as long as this does; it ends if the forward is refused. Never
/// prompts.
pub fn forward(host: &str, remote: &str, local: &str) -> Command {
    let mut cmd = Command::new("ssh");
    cmd.args([
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPath=none",
        "-o",
        "BatchMode=yes",
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-o",
        "ExitOnForwardFailure=yes",
    ])
    .args(ALIVE)
    .args(["-o", CONNECT_TIMEOUT, "-N", "-T", "-R"])
    .arg(format!("{remote}:{local}"))
    .args(["--", host]);
    cmd
}

/// Run `cmd` (from [`batch`]) to its end, `stdin` fed to it. `Err` when
/// ssh itself failed (exit 255: the host unreachable, the login refused),
/// with what ssh said; the script's own exit is the caller's to read.
pub fn run(mut cmd: Command, stdin: Option<&[u8]>) -> Result<Output, String> {
    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("ssh: {e}"))?;
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(bytes).map_err(|e| format!("ssh stdin: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("ssh: {e}"))?;
    if out.status.code() == Some(255) {
        return Err(reason(&out.stderr));
    }
    Ok(out)
}

/// The last thing ssh (or the script) said on stderr.
pub fn reason(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .last()
        .unwrap_or("ssh failed")
        .to_string()
}

/// A script that runs `argv` there, in `dir`, with `env` exported. Every
/// word rides in base64, so any text is safe in it ([`is_quotable`]);
/// a word loses trailing newlines (`$(…)`). `env` names are plain words.
/// A `dir` it cannot enter exits 126.
pub fn argv_script(dir: Option<&str>, env: &[(&str, &str)], argv: &[&str]) -> String {
    let mut s = String::new();
    if let Some(dir) = dir {
        s.push_str(&format!("cd {} || exit 126; ", word(dir)));
    }
    for (key, value) in env {
        debug_assert!(
            key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "env name {key:?}"
        );
        s.push_str(&format!("export {key}={}; ", word(value)));
    }
    s.push_str("exec");
    for arg in argv {
        s.push(' ');
        s.push_str(&word(arg));
    }
    s
}

/// `text` as one sh word, decoded from base64 there.
pub fn word(text: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(text);
    format!("\"$(echo {b64} | base64 -d)\"")
}

/// `sh -c '<script>'`.
fn sh_word(script: &str) -> String {
    debug_assert!(
        is_quotable(script),
        "an ssh script holds no ' or \\: {script}"
    );
    format!("sh -c '{script}'")
}

/// `script` can go in one single-quoted word.
pub fn is_quotable(script: &str) -> bool {
    !script.contains(['\'', '\\'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn hosts_are_one_word_that_is_not_an_option() {
        assert_eq!(check_host(" novus "), Ok("novus"));
        assert_eq!(check_host("joshua@novus.local"), Ok("joshua@novus.local"));
        assert!(check_host("").is_err());
        assert!(check_host("-oProxyCommand=x").is_err());
        assert!(check_host("two words").is_err());
        assert!(check_host("it's").is_err());
    }

    #[test]
    fn the_script_is_one_sh_word_after_the_host() {
        let a = argv(&batch("novus", "echo hi"));
        let end = &a[a.len() - 3..];
        assert_eq!(end, ["--", "novus", "sh -c 'echo hi'"]);
        assert!(a.iter().any(|s| s == "BatchMode=yes"));
        let a = argv(&interactive("novus", "true"));
        assert!(a.iter().any(|s| s == "-t"));
        assert!(!a.iter().any(|s| s == "BatchMode=yes"));
        // A tab rides a shared connection; it never starts one.
        assert!(!a.iter().any(|s| s == "ControlMaster=auto"));
    }

    #[test]
    fn an_argv_script_keeps_every_word_whole() {
        let dir = std::env::temp_dir().join(format!("sola-argv-{}", std::process::id()));
        let inner = dir.join("a dir/it's");
        std::fs::create_dir_all(&inner).unwrap();
        let script = argv_script(
            inner.to_str(),
            &[("NAME", "x \"y\" $z")],
            &["printf", "%s|%s|%s\n", "it's", "", "$HOME"],
        );
        assert!(is_quotable(&script), "{script}");
        let out = Command::new("sh").args(["-c", &script]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), "it's||$HOME");
        let env = Command::new("sh")
            .args(["-c", &argv_script(None, &[("NAME", "x \"y\" $z")], &["sh", "-c", "printf %s \"$NAME\""])])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&env.stdout), "x \"y\" $z");
        let gone = Command::new("sh")
            .args(["-c", &argv_script(Some("/no/such/dir"), &[], &["true"])])
            .output()
            .unwrap();
        assert_eq!(gone.status.code(), Some(126));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reason_is_the_last_line() {
        let err = b"Warning: something\nssh: connect to host novus port 22: No route to host\n\n";
        assert_eq!(
            reason(err),
            "ssh: connect to host novus port 22: No route to host"
        );
        assert_eq!(reason(b""), "ssh failed");
    }
}
