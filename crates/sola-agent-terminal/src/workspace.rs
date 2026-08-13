//! In-memory project / workspace ids.
//!
//! Persist and spawn land in a later slice. One live checkout plus
//! labeled demo rows prove the status column without inventing disk
//! worktree policy (D3).

use std::path::PathBuf;

use crate::status::AgentStatus;

#[derive(Clone, Debug)]
pub struct Project {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Workspace {
    pub id: String,
    #[allow(dead_code)]
    pub project_id: String,
    pub name: String,
    pub path: PathBuf,
    pub status: AgentStatus,
    /// Who is in the pane. Separate from [`Self::status`].
    pub agent: Option<String>,
    /// Scan fixture. Not a checkout; never persisted; no PTY.
    pub demo: bool,
}

/// Stable pane / tmux id for the live checkout. Must not be a random
/// UUID — `new-session -A` reattaches only when the name matches.
pub const LIVE_ID: &str = "ws-main";

/// If the stable session is missing, rename the most recently active
/// orphan `sat-*` session onto it. Returns the previous pane id (so
/// hooks from a still-running Grok still match) or `None`.
pub fn adopt_orphan_session() -> Option<String> {
    let want = sola_terminal::tmux::session_name(LIVE_ID);
    let listed = sola_terminal::tmux::list_sessions_activity()?;
    let old = pick_orphan(&listed, &want)?;
    if !sola_terminal::tmux::rename_session(&old, &want) {
        tracing::warn!(from = %old, to = %want, "failed to adopt orphan tmux session");
        return None;
    }
    tracing::info!(from = %old, to = %want, "adopted orphan tmux session");
    sola_terminal::tmux::pane_id_from_session(&old)
}

/// Prefer the newest orphan. If `want` already exists, do nothing.
pub fn pick_orphan(sessions: &[(String, u64)], want: &str) -> Option<String> {
    if sessions.iter().any(|(name, _)| name == want) {
        return None;
    }
    sessions
        .iter()
        .max_by_key(|(_, activity)| *activity)
        .map(|(name, _)| name.clone())
}

/// One live workspace on `cwd`, plus demo siblings covering the three
/// live marks. Demo names are illustrative — not created on disk.
pub fn seed() -> (Project, Vec<Workspace>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let name = cwd
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Sola")
        .to_string();
    let project = Project {
        id: "proj-seed".into(),
        name: name.clone(),
    };
    let live = Workspace {
        id: LIVE_ID.into(),
        project_id: project.id.clone(),
        name: "main".into(),
        path: cwd.clone(),
        status: AgentStatus::Idle,
        agent: None,
        demo: false,
    };
    let fixtures = [
        ("ws-demo-working", "kvm-perf", AgentStatus::Working),
        ("ws-demo-waiting", "mail-kit", AgentStatus::Waiting),
        ("ws-demo-done", "distribution", AgentStatus::Done),
    ];
    let mut workspaces = vec![live];
    for (id, ws_name, status) in fixtures {
        workspaces.push(Workspace {
            id: id.into(),
            project_id: project.id.clone(),
            name: ws_name.into(),
            path: cwd.clone(),
            status,
            agent: Some("grok".into()),
            demo: true,
        });
    }
    (project, workspaces)
}

pub fn live<'a>(workspaces: &'a [Workspace]) -> Option<&'a Workspace> {
    workspaces.iter().find(|w| !w.demo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_has_one_live_and_every_status() {
        let (_project, workspaces) = seed();
        let live_count = workspaces.iter().filter(|w| !w.demo).count();
        assert_eq!(live_count, 1);
        let mut seen = [false; 4];
        for w in &workspaces {
            let i = match w.status {
                AgentStatus::Working => 0,
                AgentStatus::Waiting => 1,
                AgentStatus::Done => 2,
                AgentStatus::Idle => 3,
            };
            seen[i] = true;
            if w.demo {
                assert!(w.agent.is_some(), "demo rows carry who, not just state");
            } else {
                assert_eq!(w.status, AgentStatus::Idle);
            }
        }
        assert!(seen.iter().all(|b| *b), "rail must show all four marks");
    }

    #[test]
    fn pick_orphan_skips_when_stable_exists() {
        let want = "sat-ws-main";
        let listed = vec![
            (want.to_string(), 10),
            ("sat-old".into(), 99),
        ];
        assert!(pick_orphan(&listed, want).is_none());
    }

    #[test]
    fn pick_orphan_takes_newest() {
        let listed = vec![
            ("sat-aaa".into(), 10),
            ("sat-bbb".into(), 50),
            ("sat-ccc".into(), 20),
        ];
        assert_eq!(
            pick_orphan(&listed, "sat-ws-main").as_deref(),
            Some("sat-bbb")
        );
    }
}
