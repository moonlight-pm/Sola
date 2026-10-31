//! What brings a computer's sessions back after a reboot (sessions
//! anywhere, slice 4; Joshua: "yeah start them -- as other sessions might
//! expect them to be there").
//!
//! Each session has a record on the computer it runs on,
//! `~/.config/sola/projects/sessions/<id>.json`: its project, name,
//! title, kind, folder, and the agent in it with that agent's session.
//! The computer that makes a session writes it; every computer watching
//! updates its agent when a hook says it changed; ending the session
//! (End Session, or its shell exiting while its computer is up) removes
//! it. [`restore`] makes every recorded session whose tmux is gone again,
//! resuming its agent (`claude --resume`, `grok -r`) when that session's
//! transcript is still there: at boot (`sola-projects --restore`), when
//! Projects starts, and when it links another computer.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sola_core::project_sessions::{self, KIND, NAME, PROJECT, TITLE};

use crate::machine::Machine;
use crate::status::PaneStatus;

/// Where records live, under a computer's home.
pub const DIR: &str = ".config/sola/projects/sessions";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Record {
    /// Its id on its computer (`sws-<id>`).
    pub id: String,
    pub project: PathBuf,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub kind: String,
    pub cwd: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// The agent's own session (what `--resume` takes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<String>,
}

fn home_of(m: &Machine) -> PathBuf {
    match m {
        Machine::Local => std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".")),
        Machine::Remote(r) => r.home.clone(),
    }
}

fn path_of(m: &Machine, id: &str) -> PathBuf {
    home_of(m).join(DIR).join(format!("{id}.json"))
}

/// Write `rec` on `m`. Waits on ssh for another computer.
pub fn write(m: &Machine, rec: &Record) -> Result<(), String> {
    let path = path_of(m, &rec.id);
    if let Some(dir) = path.parent() {
        m.mkdir_p(dir)?;
    }
    let json = serde_json::to_string(rec).map_err(|e| e.to_string())?;
    m.write(&path, format!("{json}\n").as_bytes(), None)
}

/// Write `rec` on `m` unless it has a record already (attaching keeps
/// what another computer recorded of its agent).
pub fn ensure(m: &Machine, rec: &Record) -> Result<(), String> {
    let path = path_of(m, &rec.id);
    if let Some(dir) = path.parent() {
        m.mkdir_p(dir)?;
    }
    let json = serde_json::to_string(rec).map_err(|e| e.to_string())?;
    let script = "[ -f \"$1\" ] || cat > \"$1\"";
    m.run(None, &[], &["sh", "-c", script, "sh", &path.to_string_lossy()], Some(format!("{json}\n").as_bytes()))
        .map(drop)
}

/// Remove session `id`'s record on `host` (`None`: here): it is not
/// coming back. Waits on ssh for another computer.
pub fn remove(host: Option<&str>, id: &str) {
    let rel = format!("{DIR}/{id}.json");
    let done = match host {
        None => {
            let path = home_of(&Machine::Local).join(&rel);
            match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            }
        }
        Some(host) => sola_core::ssh::run(
            sola_core::ssh::batch(host, &format!("rm -f -- \"$HOME/{rel}\"")),
            None,
        )
        .map(drop),
    };
    if let Err(e) = done {
        tracing::warn!(?host, session = %id, "removing its record: {e}");
    }
}

/// Every record on `m`.
pub fn list(m: &Machine) -> Vec<Record> {
    let dir = home_of(m).join(DIR);
    let script = "for f in \"$1\"/*.json; do [ -f \"$f\" ] && cat \"$f\" && echo; done; true";
    let Ok(out) = m.run(None, &[], &["sh", "-c", script, "sh", &dir.to_string_lossy()], None) else {
        return Vec::new();
    };
    parse(&String::from_utf8_lossy(&out.stdout))
}

fn parse(text: &str) -> Vec<Record> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// How [`restore`] makes sessions.
#[derive(Clone, Debug, Default)]
pub struct Restore {
    /// Never start a tmux server (`tmux -N`): at boot the server is PID 1's
    /// (`svc:sola-tmux-ws`), and one started here would die with this
    /// oneshot's cgroup, the restored sessions with it.
    pub no_server: bool,
    /// The desk's environment for what runs in them (`WAYLAND_DISPLAY`,
    /// `XDG_SESSION_TYPE`, `DISPLAY`): a resumed agent keeps it for life
    /// (Claude pastes an image through Wayland).
    pub env: Vec<(String, String)>,
}

/// This computer's desk, for sessions made here: its Wayland socket (the
/// name sola-river publishes; `wait` that long for it at boot) and X
/// display, when there is a desk.
pub fn desk_env(wait: std::time::Duration) -> Vec<(String, String)> {
    let deadline = std::time::Instant::now() + wait;
    let wayland = loop {
        match sola_core::env::wayland_socket() {
            Some(name) => break Some(name),
            None if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(250))
            }
            None => break sola_core::env::child_wayland_display(),
        }
    };
    let mut env = Vec::new();
    if let Some(name) = wayland {
        env.push(("WAYLAND_DISPLAY".into(), name));
        env.push(("XDG_SESSION_TYPE".into(), "wayland".into()));
    }
    if let Some(display) = sola_core::env::x_display() {
        env.push(("DISPLAY".into(), display));
    }
    env
}

/// Make every session recorded on `m` whose tmux is not there, detached,
/// as it was: its folder, its options, its agent resumed when its
/// transcript is still there (else a shell). Returns the ids made.
pub fn restore(m: &Machine, how: &Restore) -> Vec<String> {
    let records = list(m);
    if records.is_empty() {
        return Vec::new();
    }
    let live: HashSet<String> = match m {
        Machine::Local => project_sessions::list_local(),
        Machine::Remote(r) => project_sessions::list_remote(&r.host),
    }
    .unwrap_or_default()
    .into_iter()
    .map(|s| s.id)
    .collect();
    let mut made = Vec::new();
    for rec in records.iter().filter(|r| !live.contains(&r.id)) {
        match make(m, rec, how) {
            Ok(()) => {
                tracing::info!(host = ?m.host(), session = %rec.id, agent = ?rec.agent, "session back");
                made.push(rec.id.clone());
            }
            Err(e) => tracing::warn!(host = ?m.host(), session = %rec.id, "could not bring it back: {e}"),
        }
    }
    made
}

/// `tmux new-session -d` for `rec`, then its options.
fn make(m: &Machine, rec: &Record, how: &Restore) -> Result<(), String> {
    let session = format!("{}{}", project_sessions::PREFIX, rec.id);
    let cwd = rec.cwd.to_string_lossy().into_owned();
    let pane_env = format!("SOLA_PANE_ID={}", rec.id);
    let path_env = format!("{}={cwd}", crate::catalog::SOLA_WS_PATH);
    let resume = resume_argv(m, rec);
    let desk: Vec<String> = how.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut argv: Vec<&str> = vec!["tmux", "-L", project_sessions::SOCKET];
    if how.no_server {
        argv.push("-N");
    }
    argv.extend([
        "new-session", "-d", "-s", &session, "-c", &cwd, "-e", &pane_env, "-e", &path_env,
    ]);
    for e in &desk {
        argv.extend(["-e", e.as_str()]);
    }
    argv.extend(resume.iter().map(String::as_str));
    let out = m.run(None, &[], &argv, None)?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let target = format!("={session}:");
    let project = rec.project.to_string_lossy().into_owned();
    let title = rec.title.clone().unwrap_or_default();
    let mut set: Vec<&str> = vec!["tmux", "-L", project_sessions::SOCKET];
    for (i, (k, v)) in [(PROJECT, project.as_str()), (NAME, rec.name.as_str()), (TITLE, title.as_str()), (KIND, rec.kind.as_str())]
        .into_iter()
        .enumerate()
    {
        if i > 0 {
            set.push(";");
        }
        set.extend(["set-option", "-t", target.as_str(), k, v]);
    }
    let _ = m.run(None, &[], &set, None);
    Ok(())
}

/// Its agent's resume, when that session is still on `m`.
fn resume_argv(m: &Machine, rec: &Record) -> Vec<String> {
    let st = PaneStatus {
        agent: rec.agent.clone(),
        owner_session: rec.agent_session.clone(),
        ..PaneStatus::default()
    };
    crate::resume_argv_on(m, &st).unwrap_or_default()
}

/// The record for session `id` from what Projects knows of it.
pub fn of(id: &str, project: &Path, name: &str, title: Option<&str>, kind: &str, cwd: &Path, st: Option<&PaneStatus>) -> Record {
    Record {
        id: id.to_string(),
        project: project.to_path_buf(),
        name: name.to_string(),
        title: title.map(str::to_string),
        kind: kind.to_string(),
        cwd: cwd.to_path_buf(),
        agent: st.and_then(|s| s.agent.clone()),
        agent_session: st.and_then(|s| s.owner_session.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip_one_per_line() {
        let a = Record {
            id: "ws-main".into(),
            project: "/home/Projects/Sola".into(),
            name: "root".into(),
            title: None,
            kind: "main".into(),
            cwd: "/home/Projects/Sola".into(),
            agent: Some("claude".into()),
            agent_session: Some("cafcd1e7".into()),
        };
        let b = Record {
            id: "ws-zones".into(),
            name: "zones".into(),
            kind: "root".into(),
            ..a.clone()
        };
        let text = format!(
            "{}\n\n{}\nnot json\n",
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
        assert_eq!(parse(&text), vec![a, b]);
    }

    #[test]
    fn records_live_under_home() {
        let p = path_of(&Machine::Local, "ws-x");
        assert!(p.ends_with(".config/sola/projects/sessions/ws-x.json"), "{}", p.display());
    }
}
