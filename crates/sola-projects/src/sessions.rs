//! A computer's Projects sessions, as its tmux tells them: any computer
//! can list them and open one (sessions anywhere, 2026-10-30).
//!
//! A session says what it is in tmux user options, set when it is made
//! ([`stamp`]): `@sola-project` (its project's folder), `@sola-name`,
//! `@sola-title`, `@sola-kind`. A session made before those existed is
//! placed by the folder its pane is in (`~/Projects/<folder>/…`).

use std::path::{Path, PathBuf};

use sola_core::ssh;

use crate::machine::Machine;

pub const PROJECT: &str = "@sola-project";
pub const NAME: &str = "@sola-name";
pub const TITLE: &str = "@sola-title";
pub const KIND: &str = "@sola-kind";

/// One line per session: name, the options above, the pane's folder.
const FORMAT: &str =
    "#{session_name}\t#{@sola-project}\t#{@sola-name}\t#{@sola-title}\t#{@sola-kind}\t#{pane_current_path}";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostSession {
    /// Its id there (`sws-<id>`).
    pub id: String,
    /// Its project's folder there.
    pub project: Option<PathBuf>,
    pub name: Option<String>,
    pub title: Option<String>,
    pub kind: Option<String>,
}

/// The sessions on this computer.
pub fn list_local() -> Result<Vec<HostSession>, String> {
    list_with(&Machine::Local)
}

/// The sessions on `host`, over the shared ssh connection (no link
/// needed). Waits on ssh: keep it off the UI thread.
pub fn list_remote(host: &str) -> Result<Vec<HostSession>, String> {
    let out = ssh::run(ssh::batch(host, &script()), None)?;
    if !out.status.success() {
        return Err(format!("listing sessions on {host}: {}", ssh::reason(&out.stderr)));
    }
    Ok(parse(&String::from_utf8_lossy(&out.stdout), &prefix()))
}

fn list_with(m: &Machine) -> Result<Vec<HostSession>, String> {
    let script = script();
    let out = m.run(None, &[], &["sh", "-c", &script], None)?;
    Ok(parse(&String::from_utf8_lossy(&out.stdout), &prefix()))
}

/// Every session on the Projects socket, and on the one sessions opened
/// from another computer used before (`sola-ws-remote`).
fn script() -> String {
    let sockets = [
        sola_terminal::remote::socket(),
        sola_terminal::remote::legacy_socket(),
    ];
    let mut s = String::new();
    for socket in sockets {
        // Double quotes: an ssh script holds no `'` ([`ssh::is_quotable`]).
        s.push_str(&format!(
            "tmux -L {socket} list-sessions -F \"{FORMAT}\" 2>/dev/null; "
        ));
    }
    s.push_str("true");
    s
}

/// Projects' session names start with this (`sws-`).
fn prefix() -> String {
    sola_terminal::tmux::session_name("")
}

/// A listing's sessions (the first of a name wins: `sola-ws` is listed
/// before `sola-ws-remote`).
fn parse(text: &str, prefix: &str) -> Vec<HostSession> {
    let mut out: Vec<HostSession> = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let [session, project, name, title, kind, cwd] = f[..] else {
            continue;
        };
        let Some(id) = session.strip_prefix(prefix) else {
            continue;
        };
        if id.is_empty() || out.iter().any(|s| s.id == id) {
            continue;
        }
        let some = |v: &str| (!v.trim().is_empty()).then(|| v.to_string());
        let project = some(project)
            .map(PathBuf::from)
            .or_else(|| project_of(Path::new(cwd)));
        out.push(HostSession {
            id: id.to_string(),
            project,
            name: some(name),
            title: some(title),
            kind: some(kind),
        });
    }
    out
}

/// The project a folder is in: `…/Projects/<folder>` above it.
fn project_of(cwd: &Path) -> Option<PathBuf> {
    let mut at = cwd;
    loop {
        let parent = at.parent()?;
        if parent.file_name().is_some_and(|n| n == "Projects") {
            return Some(at.to_path_buf());
        }
        at = parent;
    }
}

/// The tmux options a new session carries (see the module docs).
pub fn options(project: &Path, name: &str, title: Option<&str>, kind: &str) -> Vec<(String, String)> {
    vec![
        (PROJECT.into(), project.to_string_lossy().into_owned()),
        (NAME.into(), name.into()),
        (TITLE.into(), title.unwrap_or("").into()),
        (KIND.into(), kind.into()),
    ]
}

/// Set `opts` on local session `session`. `set-option` takes a pane
/// target, so the exact session is `=name:`.
pub fn stamp(session: &str, opts: &[(String, String)]) {
    let target = format!("={session}:");
    let mut cmd = sola_terminal::tmux::tmux_cmd();
    for (i, (k, v)) in opts.iter().enumerate() {
        if i > 0 {
            cmd.arg(";");
        }
        cmd.args(["set-option", "-t", &target, k, v]);
    }
    match cmd.output() {
        Ok(o) if o.status.success() => {}
        Ok(o) => tracing::warn!(%session, "stamping session options: {}", String::from_utf8_lossy(&o.stderr).trim()),
        Err(e) => tracing::warn!(%session, "stamping session options: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_listing_goes_over_ssh_whole() {
        assert!(ssh::is_quotable(&script()), "{}", script());
    }

    #[test]
    fn a_listing_says_each_sessions_project() {
        let text = "sws-ws-main\t/home/Projects/Sola\troot\t\tmain\t/home/Projects/Sola\n\
                    sws-ws-fix\t\t\t\t\t/home/Projects/Oath/.worktrees/fix/crates\n\
                    _keepalive\t\t\t\t\t/home\n\
                    sws-ws-main\t\t\t\t\t/elsewhere\n\
                    sws-ws-tmp\t\t\t\t\t/tmp\n";
        let got = parse(text, "sws-");
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].project.as_deref(), Some(Path::new("/home/Projects/Sola")));
        assert_eq!(got[0].name.as_deref(), Some("root"));
        assert_eq!(got[0].kind.as_deref(), Some("main"));
        assert_eq!(got[1].id, "ws-fix");
        assert_eq!(got[1].project.as_deref(), Some(Path::new("/home/Projects/Oath")));
        assert_eq!(got[1].name, None);
        assert_eq!(got[2].project, None);
    }
}
