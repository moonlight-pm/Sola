//! The Projects chooser's Remote stage: open a project on another computer
//! (Joshua, 2026-10-28: "When i open projects through launcher, there
//! should be a 'remote' option. This allows me to type in a hostname, and
//! it should remember recently used. It will ssh there to get a list of
//! projects to open.").
//!
//! The field takes a host (`name`, `user@name`, a `~/.ssh/config` alias);
//! the rows are the hosts opened before, latest first, matching what is
//! typed, then the typed one. Picking one lists that computer's
//! `~/Projects` over ssh ([`list`]); the chooser then shows its folders,
//! and picking one has Projects open it there (`project.open --host`):
//! its tabs are tmux sessions on that computer.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sola_core::ssh;

/// How many recent hosts are kept.
const RECENT_MAX: usize = 8;

/// A row of the host stage, in the order shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostRow {
    /// `recent[i]`.
    Recent(usize),
    /// The host as typed (it is not one of the recent ones).
    Typed,
}

#[derive(Debug, Default)]
pub struct Hosts {
    /// Hosts opened before, latest first.
    pub recent: Vec<String>,
    /// `recent` matching the query, in order.
    pub filtered: Vec<usize>,
    /// The query, trimmed.
    query: String,
    /// Listing this host's projects (the rows wait for it).
    pub connecting: Option<String>,
    /// Why the last host picked could not be listed.
    pub error: Option<String>,
}

impl Hosts {
    pub fn new(recent: Vec<String>) -> Self {
        let mut h = Hosts {
            recent,
            ..Hosts::default()
        };
        h.apply_query("");
        h
    }

    /// Keep the recent hosts holding `query` (case aside).
    pub fn apply_query(&mut self, query: &str) {
        self.query = query.trim().to_string();
        let q = self.query.to_lowercase();
        self.filtered = (0..self.recent.len())
            .filter(|&i| q.is_empty() || self.recent[i].to_lowercase().contains(&q))
            .collect();
    }

    /// The rows: matching recent hosts, then the typed one. None while
    /// connecting.
    pub fn rows(&self) -> Vec<HostRow> {
        if self.connecting.is_some() {
            return Vec::new();
        }
        let mut rows: Vec<HostRow> = self.filtered.iter().map(|&i| HostRow::Recent(i)).collect();
        if self.typed().is_some() {
            rows.push(HostRow::Typed);
        }
        rows
    }

    /// The host typed, unless it is one of the recent ones.
    pub fn typed(&self) -> Option<&str> {
        let q = self.query.as_str();
        (!q.is_empty() && !self.recent.iter().any(|h| h == q)).then_some(q)
    }

    /// The host row `row` stands for.
    pub fn host(&self, row: HostRow) -> Option<&str> {
        match row {
            HostRow::Recent(i) => self.recent.get(i).map(String::as_str),
            HostRow::Typed => self.typed(),
        }
    }
}

/// A computer's `~/Projects`: its full path there, its folders, and the
/// Projects sessions running there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub root: PathBuf,
    /// By name, case aside; no hidden ones.
    pub names: Vec<String>,
    pub sessions: Vec<sola_core::project_sessions::HostSession>,
}

/// Between the folders and the sessions in [`list`]'s output.
const SESSIONS_MARK: &str = "--sola-sessions--";

/// List `host`'s `~/Projects` over ssh. Waits on ssh: run it off the UI
/// thread. A tab there needs tmux, so its absence is an error here too.
pub fn list(host: &str) -> Result<Listing, String> {
    let host = ssh::check_host(host)?;
    let script = format!(
        "{LIST_SCRIPT}; echo {SESSIONS_MARK}; {}",
        sola_core::project_sessions::script()
    );
    let out = ssh::run(ssh::batch(host, &script), None)?;
    match out.status.code() {
        Some(0) => parse_listing(&String::from_utf8_lossy(&out.stdout)),
        Some(3) => Err(format!("There is no ~/Projects on {host}")),
        Some(4) => Err(format!("tmux is not installed on {host}")),
        _ => Err(ssh::reason(&out.stderr)),
    }
}

/// `pwd`, then each folder's name. No `'` or `\` ([`ssh::is_quotable`]).
const LIST_SCRIPT: &str = "cd \"$HOME/Projects\" 2>/dev/null || exit 3; \
    command -v tmux >/dev/null || exit 4; \
    pwd; for d in */; do [ -d \"$d\" ] && printf %s \"${d%/}\" && echo; done; true";

fn parse_listing(out: &str) -> Result<Listing, String> {
    let (out, sessions) = match out.split_once(&format!("{SESSIONS_MARK}\n")) {
        Some((folders, sessions)) => (folders, sola_core::project_sessions::parse(sessions)),
        None => (out, Vec::new()),
    };
    let mut lines = out.lines();
    let root = PathBuf::from(lines.next().unwrap_or_default().trim());
    if !root.is_absolute() {
        return Err("Its ~/Projects listing made no sense".into());
    }
    let mut names: Vec<String> = lines
        .map(str::to_string)
        .filter(|n| !n.is_empty() && !n.starts_with('.'))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    Ok(Listing {
        root,
        names,
        sessions,
    })
}

/// Make folder `name` in `root` (its `~/Projects`) on `host`; an
/// existing one is used as it is. Waits on ssh.
pub fn create(host: &str, root: &std::path::Path, name: &str) -> Result<PathBuf, String> {
    let host = ssh::check_host(host)?;
    let name = super::projects::check_name(name)?;
    let path = root.join(name);
    let script = ssh::argv_script(None, &[], &["mkdir", "-p", "--", &path.to_string_lossy()]);
    let out = ssh::run(ssh::batch(host, &script), None)?;
    if out.status.success() {
        Ok(path)
    } else {
        Err(format!("{host}: {}", ssh::reason(&out.stderr)))
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Recent {
    hosts: Vec<String>,
}

fn recent_path() -> PathBuf {
    sola_core::config::sola_config_dir()
        .join("shell")
        .join("remote-hosts.json")
}

/// The hosts opened before, latest first.
pub fn load_recent() -> Vec<String> {
    match sola_core::config::load_json_or_default::<Recent>(&recent_path()) {
        Ok(r) => r.hosts,
        Err(e) => {
            tracing::warn!("recent hosts: {e}");
            Vec::new()
        }
    }
}

/// `host` is the latest opened.
pub fn remember(host: &str) {
    let hosts = with_latest(load_recent(), host);
    if let Err(e) = sola_core::config::save_json_pretty(&recent_path(), &Recent { hosts }) {
        tracing::warn!(%host, "recent hosts: {e}");
    }
}

fn with_latest(mut hosts: Vec<String>, host: &str) -> Vec<String> {
    hosts.retain(|h| h != host);
    hosts.insert(0, host.to_string());
    hosts.truncate(RECENT_MAX);
    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts(names: &[&str]) -> Hosts {
        Hosts::new(names.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn rows_are_matching_recent_hosts_then_the_typed_one() {
        let mut h = hosts(&["novus", "buccina"]);
        assert_eq!(h.rows(), [HostRow::Recent(0), HostRow::Recent(1)]);
        h.apply_query("nov");
        assert_eq!(h.rows(), [HostRow::Recent(0), HostRow::Typed]);
        assert_eq!(h.host(HostRow::Typed), Some("nov"));
        h.apply_query(" novus ");
        assert_eq!(h.rows(), [HostRow::Recent(0)]);
        h.connecting = Some("novus".into());
        assert!(h.rows().is_empty());
    }

    #[test]
    fn the_latest_host_goes_first_once() {
        let v = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(with_latest(v(&["a", "b", "c"]), "c"), v(&["c", "a", "b"]));
        assert_eq!(with_latest(v(&[]), "a"), v(&["a"]));
        let many: Vec<String> = (0..20).map(|i| i.to_string()).collect();
        assert_eq!(with_latest(many, "x").len(), RECENT_MAX);
    }

    #[test]
    fn a_listing_is_its_root_then_its_folders() {
        let l = parse_listing("/home/j/Projects\nsola\nOath\n\n.hidden\n").unwrap();
        assert_eq!(l.root, PathBuf::from("/home/j/Projects"));
        assert_eq!(l.names, ["Oath", "sola"]);
        assert!(parse_listing("").is_err());
        assert!(parse_listing("Projects\nsola").is_err());
    }

    #[test]
    fn a_listing_carries_the_sessions_there() {
        let l = parse_listing(
            "/home/Projects\nSola\n--sola-sessions--\nsws-ws-main\t/home/Projects/Sola\troot\t\tmain\t/home/Projects/Sola\n",
        )
        .unwrap();
        assert_eq!(l.names, ["Sola"]);
        assert_eq!(l.sessions.len(), 1);
        assert_eq!(l.sessions[0].id, "ws-main");
        assert!(ssh::is_quotable(&format!(
            "{LIST_SCRIPT}; echo {SESSIONS_MARK}; {}",
            sola_core::project_sessions::script()
        )));
    }

    #[test]
    fn the_list_script_runs_under_sh() {
        assert!(ssh::is_quotable(LIST_SCRIPT));
        let home = std::env::temp_dir().join(format!("sola-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("Projects/b b")).unwrap();
        std::fs::create_dir_all(home.join("Projects/A")).unwrap();
        std::fs::write(home.join("Projects/notes.txt"), "").unwrap();
        let script = LIST_SCRIPT.replace("command -v tmux >/dev/null", "true");
        let out = std::process::Command::new("sh")
            .args(["-c", &script])
            .env("HOME", &home)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0));
        let l = parse_listing(&String::from_utf8_lossy(&out.stdout)).unwrap();
        assert_eq!(l.names, ["A", "b b"]);
        assert!(l.root.ends_with("Projects"));
        std::fs::remove_dir_all(home.join("Projects")).unwrap();
        let out = std::process::Command::new("sh")
            .args(["-c", &script])
            .env("HOME", &home)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(3));
        let _ = std::fs::remove_dir_all(&home);
    }
}
