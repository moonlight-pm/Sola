//! Projects' sessions on a computer, as its tmux lists them: read by
//! Projects (which opens them) and by the launcher's chooser (which lists
//! them under their projects). Sessions anywhere, 2026-10-30.
//!
//! A session says what it is in tmux user options set when it is made:
//! `@sola-project` (its project's folder), `@sola-name`, `@sola-title`,
//! `@sola-kind`. One made before those existed is placed by the folder its
//! pane is in (`…/Projects/<folder>/…`).

use std::path::{Path, PathBuf};

use crate::ssh;

/// Projects' tmux socket on every computer.
pub const SOCKET: &str = "sola-ws";
/// Where sessions opened from another computer lived before 2026-10-30.
pub const LEGACY_SOCKET: &str = "sola-ws-remote";
/// Projects' session names: `sws-<id>`.
pub const PREFIX: &str = "sws-";

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

impl HostSession {
    /// The project's main session (the label names only the project).
    pub fn is_main(&self) -> bool {
        match self.kind.as_deref() {
            Some(k) => k == "main" || k == "folder",
            None => self.id == "ws-main" || self.id.starts_with("ws-main-"),
        }
    }

    /// What a list calls it: its title, else its name (`main` for the
    /// project's main one), else its id.
    pub fn label(&self) -> String {
        if let Some(t) = &self.title {
            return t.clone();
        }
        if self.is_main() {
            return "main".into();
        }
        self.name
            .clone()
            .unwrap_or_else(|| self.id.strip_prefix("ws-").unwrap_or(&self.id).to_string())
    }
}

/// The sessions on this computer.
pub fn list_local() -> Result<Vec<HostSession>, String> {
    let out = std::process::Command::new("sh")
        .args(["-c", &script()])
        .output()
        .map_err(|e| format!("listing sessions: {e}"))?;
    Ok(parse(&String::from_utf8_lossy(&out.stdout)))
}

/// The sessions on `host`, over the shared ssh connection. Waits on ssh:
/// keep it off the UI thread.
pub fn list_remote(host: &str) -> Result<Vec<HostSession>, String> {
    let host = ssh::check_host(host)?;
    let out = ssh::run(ssh::batch(host, &script()), None)?;
    if !out.status.success() {
        return Err(format!("listing sessions on {host}: {}", ssh::reason(&out.stderr)));
    }
    Ok(parse(&String::from_utf8_lossy(&out.stdout)))
}

/// A `sh` script that lists every session on the Projects socket, then on
/// the legacy one. No `'` or `\`: it goes over ssh as it is
/// ([`ssh::is_quotable`]).
pub fn script() -> String {
    let mut s = String::new();
    for socket in [SOCKET, LEGACY_SOCKET] {
        s.push_str(&format!("tmux -L {socket} list-sessions -F \"{FORMAT}\" 2>/dev/null; "));
    }
    s.push_str("true");
    s
}

/// A listing's sessions (the first of a name wins: [`SOCKET`] is listed
/// before [`LEGACY_SOCKET`]). Lines that are not Projects' sessions are
/// skipped.
pub fn parse(text: &str) -> Vec<HostSession> {
    let mut out: Vec<HostSession> = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let [session, project, name, title, kind, cwd] = f[..] else {
            continue;
        };
        let Some(id) = session.strip_prefix(PREFIX) else {
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
        let got = parse(text);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].project.as_deref(), Some(Path::new("/home/Projects/Sola")));
        assert_eq!(got[0].name.as_deref(), Some("root"));
        assert!(got[0].is_main());
        assert_eq!(got[0].label(), "main");
        assert_eq!(got[1].id, "ws-fix");
        assert_eq!(got[1].project.as_deref(), Some(Path::new("/home/Projects/Oath")));
        assert_eq!(got[1].label(), "fix");
        assert_eq!(got[2].project, None);
    }

    #[test]
    fn a_title_names_it_first() {
        let s = HostSession {
            id: "ws-zones".into(),
            project: None,
            name: Some("zones".into()),
            title: Some("tab bar zones".into()),
            kind: Some("root".into()),
        };
        assert_eq!(s.label(), "tab bar zones");
        assert!(!s.is_main());
    }
}
