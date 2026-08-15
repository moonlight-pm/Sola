//! Projects, workspaces, catalog persist.
//!
//! Live status stays off the catalog. Demo rows are gone — hooks
//! supply the marks. Worktree paths are `<root>/.worktrees/<slug>` (D4.2).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::spawn;
use crate::status::AgentStatus;

/// Stable pane / tmux id for a project's main checkout. Must stay
/// `ws-main` so the smoked `sat-ws-main` session reattaches.
pub const LIVE_ID: &str = "ws-main";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Main,
    Worktree,
    Folder,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub collapsed: bool,
    pub root: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Workspace {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub path: PathBuf,
    pub kind: Kind,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(skip)]
    pub status: AgentStatus,
    /// Who is in the pane. Separate from [`Self::status`].
    #[serde(skip)]
    pub agent: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Catalog {
    #[serde(default)]
    pub version: u32,
    pub selected: Option<String>,
    pub projects: Vec<Project>,
    pub workspaces: Vec<Workspace>,
}

impl Catalog {
    pub fn empty() -> Self {
        Self {
            version: 1,
            selected: None,
            projects: Vec::new(),
            workspaces: Vec::new(),
        }
    }
}

fn catalog_path() -> PathBuf {
    sola_core::config::sola_config_dir()
        .join("agent-terminal")
        .join("catalog.json")
}

pub fn load() -> Catalog {
    let text = match std::fs::read_to_string(catalog_path()) {
        Ok(t) => t,
        Err(_) => return Catalog::empty(),
    };
    match serde_json::from_str::<Catalog>(&text) {
        Ok(mut c) => {
            c.version = 1;
            c
        }
        Err(e) => {
            tracing::warn!("catalog parse failed: {e}");
            Catalog::empty()
        }
    }
}

pub fn save(catalog: &Catalog) {
    let path = catalog_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_string_pretty(catalog) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&path, text) {
                tracing::warn!(path = %path.display(), "catalog write failed: {e}");
            }
        }
        Err(e) => tracing::warn!("catalog serialize failed: {e}"),
    }
}

/// If the stable main session is missing, rename the most recently
/// active orphan `sat-*` session onto it. Returns the previous pane
/// id (so hooks from a still-running Grok still match) or `None`.
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

/// First launch: one project from `cwd` when that folder is a git checkout.
pub fn seed_from_cwd() -> Option<(Project, Workspace)> {
    let cwd = std::env::current_dir().ok()?;
    if !spawn::is_git_checkout(&cwd) {
        return None;
    }
    Some(project_from_root(&cwd, "proj-seed", LIVE_ID))
}

pub fn project_from_root(root: &Path, project_id: &str, main_id: &str) -> (Project, Workspace) {
    let name = root
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("project")
        .to_string();
    let project = Project {
        id: project_id.into(),
        name,
        collapsed: false,
        root: root.to_path_buf(),
    };
    let kind = if spawn::is_git_checkout(root) {
        Kind::Main
    } else {
        Kind::Folder
    };
    let ws = Workspace {
        id: main_id.into(),
        project_id: project.id.clone(),
        name: "root".into(),
        path: root.to_path_buf(),
        kind,
        parent: None,
        status: AgentStatus::Idle,
        agent: None,
    };
    (project, ws)
}

/// Reuse `ws-main` only when that tmux session is free or already in
/// this checkout. Stops a first-added other project from stealing the
/// smoked `sat-ws-main` pane.
pub fn main_workspace_id(root: &Path, taken: &HashSet<String>) -> String {
    if !taken.contains(LIVE_ID) {
        let session = sola_terminal::tmux::session_name(LIVE_ID);
        match sola_terminal::tmux::pane_current_path(&session) {
            None => return LIVE_ID.into(),
            Some(cwd) => {
                if path_eq(&cwd, root) {
                    return LIVE_ID.into();
                }
            }
        }
    }
    unique_id("ws", "main", taken)
}

fn path_eq(a: &str, b: &Path) -> bool {
    let pa = PathBuf::from(a);
    match (std::fs::canonicalize(&pa), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => pa == *b,
    }
}

pub fn unique_id(prefix: &str, slug: &str, taken: &HashSet<String>) -> String {
    let base = if slug.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}-{slug}")
    };
    if !taken.contains(&base) {
        return base;
    }
    for i in 2.. {
        let id = format!("{base}-{i}");
        if !taken.contains(&id) {
            return id;
        }
    }
    base
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn lineage_depth(ws: &Workspace, all: &[Workspace]) -> u8 {
    let mut d = 0u8;
    let mut cur = ws.parent.as_deref();
    let mut guard = 0u8;
    while let Some(pid) = cur {
        d = d.saturating_add(1);
        guard += 1;
        if d >= 2 || guard > 8 {
            break;
        }
        cur = all
            .iter()
            .find(|w| w.id == pid)
            .and_then(|w| w.parent.as_deref());
    }
    d.min(2)
}

/// Main first, then each parent's children, then remaining by name.
pub fn ordered_for_project<'a>(
    project_id: &str,
    all: &'a [Workspace],
) -> Vec<&'a Workspace> {
    let mine: Vec<&Workspace> = all.iter().filter(|w| w.project_id == project_id).collect();
    let mut out = Vec::with_capacity(mine.len());
    let mut seen = HashSet::new();
    let mut roots: Vec<&Workspace> = mine
        .iter()
        .copied()
        .filter(|w| {
            w.parent
                .as_ref()
                .is_none_or(|p| mine.iter().all(|o| o.id != *p))
        })
        .collect();
    roots.sort_by(|a, b| match (&a.kind, &b.kind) {
        (Kind::Main, Kind::Main) => a.name.cmp(&b.name),
        (Kind::Main, _) => std::cmp::Ordering::Less,
        (_, Kind::Main) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
    fn walk<'a>(
        node: &'a Workspace,
        mine: &[&'a Workspace],
        seen: &mut HashSet<String>,
        out: &mut Vec<&'a Workspace>,
    ) {
        if !seen.insert(node.id.clone()) {
            return;
        }
        out.push(node);
        let mut kids: Vec<&Workspace> = mine
            .iter()
            .copied()
            .filter(|w| w.parent.as_deref() == Some(node.id.as_str()))
            .collect();
        kids.sort_by(|a, b| a.name.cmp(&b.name));
        for k in kids {
            walk(k, mine, seen, out);
        }
    }
    for r in roots {
        walk(r, &mine, &mut seen, &mut out);
    }
    for w in mine {
        if seen.insert(w.id.clone()) {
            out.push(w);
        }
    }
    out
}

pub fn find_project<'a>(projects: &'a [Project], id: &str) -> Option<&'a Project> {
    projects.iter().find(|p| p.id == id)
}

/// Match id, exact name, or case-insensitive name.
pub fn resolve_project<'a>(projects: &'a [Project], q: &str) -> Result<&'a Project, String> {
    if let Some(p) = projects.iter().find(|p| p.id == q) {
        return Ok(p);
    }
    let lower = q.to_ascii_lowercase();
    let hits: Vec<&Project> = projects
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case(q) || p.name.to_ascii_lowercase() == lower)
        .collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("unknown project '{q}'")),
        _ => Err(format!("project '{q}' is ambiguous")),
    }
}

pub fn resolve_workspace<'a>(
    workspaces: &'a [Workspace],
    q: &str,
) -> Result<&'a Workspace, String> {
    if let Some(w) = workspaces.iter().find(|w| w.id == q) {
        return Ok(w);
    }
    let hits: Vec<&Workspace> = workspaces
        .iter()
        .filter(|w| w.name.eq_ignore_ascii_case(q))
        .collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("unknown workspace '{q}'")),
        _ => Err(format!("workspace '{q}' is ambiguous")),
    }
}

pub fn find_workspace_mut<'a>(
    workspaces: &'a mut [Workspace],
    id: &str,
) -> Option<&'a mut Workspace> {
    workspaces.iter_mut().find(|w| w.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_orphan_skips_when_stable_exists() {
        let want = "sat-ws-main";
        let listed = vec![(want.to_string(), 10), ("sat-old".into(), 99)];
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

    #[test]
    fn resolve_project_by_name() {
        let projects = vec![Project {
            id: "proj-seed".into(),
            name: "Sola".into(),
            collapsed: false,
            root: PathBuf::from("/tmp/sola"),
        }];
        assert_eq!(resolve_project(&projects, "Sola").unwrap().id, "proj-seed");
        assert_eq!(resolve_project(&projects, "sola").unwrap().id, "proj-seed");
        assert!(resolve_project(&projects, "nope").is_err());
    }

    #[test]
    fn unique_id_suffixes() {
        let mut taken = HashSet::new();
        taken.insert("ws-kvm-perf".into());
        assert_eq!(unique_id("ws", "kvm-perf", &taken), "ws-kvm-perf-2");
        taken.insert("ws-kvm-perf-2".into());
        assert_eq!(unique_id("ws", "kvm-perf", &taken), "ws-kvm-perf-3");
    }

    #[test]
    fn lineage_orders_children_under_parent() {
        let p = "p".to_string();
        let main = Workspace {
            id: "ws-main".into(),
            project_id: p.clone(),
            name: "main".into(),
            path: PathBuf::from("/r"),
            kind: Kind::Main,
            parent: None,
            status: AgentStatus::Idle,
            agent: None,
        };
        let child = Workspace {
            id: "ws-kid".into(),
            project_id: p.clone(),
            name: "kid".into(),
            path: PathBuf::from("/r/.worktrees/kid"),
            kind: Kind::Worktree,
            parent: Some("ws-main".into()),
            status: AgentStatus::Idle,
            agent: None,
        };
        let other = Workspace {
            id: "ws-z".into(),
            project_id: p.clone(),
            name: "zeta".into(),
            path: PathBuf::from("/r/.worktrees/zeta"),
            kind: Kind::Worktree,
            parent: None,
            status: AgentStatus::Idle,
            agent: None,
        };
        let all = vec![other.clone(), child.clone(), main.clone()];
        let ids: Vec<&str> = ordered_for_project(&p, &all)
            .into_iter()
            .map(|w| w.id.as_str())
            .collect();
        assert_eq!(ids, ["ws-main", "ws-kid", "ws-z"]);
        assert_eq!(lineage_depth(&child, &all), 1);
        assert_eq!(lineage_depth(&main, &all), 0);
    }

    #[test]
    fn catalog_round_trip_skips_live_status() {
        let mut c = Catalog::empty();
        c.version = 1;
        c.selected = Some("ws-main".into());
        c.projects.push(Project {
            id: "proj-seed".into(),
            name: "Sola".into(),
            collapsed: true,
            root: PathBuf::from("/tmp/sola"),
        });
        c.workspaces.push(Workspace {
            id: "ws-main".into(),
            project_id: "proj-seed".into(),
            name: "main".into(),
            path: PathBuf::from("/tmp/sola"),
            kind: Kind::Main,
            parent: None,
            status: AgentStatus::Working,
            agent: Some("grok".into()),
        });
        let text = serde_json::to_string(&c).unwrap();
        assert!(!text.contains("working"), "{text}");
        assert!(!text.contains("grok"), "{text}");
        let back: Catalog = serde_json::from_str(&text).unwrap();
        assert_eq!(back.projects[0].name, "Sola");
        assert!(back.projects[0].collapsed);
        assert_eq!(back.workspaces[0].kind, Kind::Main);
        assert_eq!(back.workspaces[0].status, AgentStatus::Idle);
        assert!(back.workspaces[0].agent.is_none());
    }
}
