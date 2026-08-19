//! Projects, workspaces, catalog persist.
//!
//! Live status stays off the catalog. Demo rows are gone — hooks
//! supply the marks. Worktree paths are `<root>/.worktrees/<slug>` (D4.2).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sola_bus::topics::SplitDir;
use sola_terminal::state::PaneNode;

use crate::spawn;
use crate::status::AgentStatus;

/// Stable pane / tmux id for a project's main checkout. Reused only
/// when the live `sws-ws-main` session is free or already in that
/// checkout — never stolen from another project.
pub const LIVE_ID: &str = "ws-main";

/// Stamped on every pane so restart can refuse a leftover session
/// from a deleted / other workspace that reused the same id.
pub const SOLA_WS_PATH: &str = "SOLA_WS_PATH";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
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
    /// `/bin/sh -c` after each sibling worktree is created. Empty = skip.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub startup: String,
}

/// Binary split tree. Omitted in the catalog when the workspace is a
/// single leaf whose id is the workspace id (the pre-split shape).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Layout {
    Leaf {
        id: String,
    },
    Split {
        id: String,
        dir: SplitDir,
        ratio: f32,
        a: Box<Layout>,
        b: Box<Layout>,
    },
}

impl Layout {
    pub fn single(id: impl Into<String>) -> Self {
        Self::Leaf { id: id.into() }
    }

    pub fn from_node(node: &PaneNode) -> Self {
        match node {
            PaneNode::Leaf(id) => Self::Leaf { id: id.clone() },
            PaneNode::Split {
                id,
                dir,
                ratio,
                a,
                b,
            } => Self::Split {
                id: id.clone(),
                dir: *dir,
                ratio: *ratio,
                a: Box::new(Self::from_node(a)),
                b: Box::new(Self::from_node(b)),
            },
        }
    }

    pub fn to_node(&self) -> PaneNode {
        match self {
            Self::Leaf { id } => PaneNode::Leaf(id.clone()),
            Self::Split {
                id,
                dir,
                ratio,
                a,
                b,
            } => PaneNode::Split {
                id: id.clone(),
                dir: *dir,
                ratio: *ratio,
                a: Box::new(a.to_node()),
                b: Box::new(b.to_node()),
            },
        }
    }

    pub fn leaves(&self) -> Vec<String> {
        sola_terminal::state::leaves_of(&self.to_node())
    }

    pub fn split_ids(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect_split_ids(self, &mut out);
        out
    }

    pub fn is_single(&self) -> bool {
        matches!(self, Self::Leaf { .. })
    }
}

fn collect_split_ids(layout: &Layout, out: &mut Vec<String>) {
    if let Layout::Split { id, a, b, .. } = layout {
        out.push(id.clone());
        collect_split_ids(a, out);
        collect_split_ids(b, out);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Workspace {
    pub id: String,
    pub project_id: String,
    pub name: String,
    /// Extra rail label (`sc-1234 · short title`). Empty = just `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub path: PathBuf,
    pub kind: Kind,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Layout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_pane: Option<String>,
    #[serde(skip)]
    pub status: AgentStatus,
    /// Who is in the pane. Separate from [`Self::status`].
    #[serde(skip)]
    pub agent: Option<String>,
}

impl Workspace {
    pub fn layout(&self) -> Layout {
        self.layout
            .clone()
            .unwrap_or_else(|| Layout::single(&self.id))
    }

    pub fn active_pane_id(&self) -> String {
        self.active_pane
            .clone()
            .filter(|id| self.layout().leaves().iter().any(|p| p == id))
            .unwrap_or_else(|| {
                self.layout()
                    .leaves()
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| self.id.clone())
            })
    }

    pub fn set_tree(&mut self, node: PaneNode, active: String) {
        let layout = Layout::from_node(&node);
        if layout.is_single() && layout.leaves().first().is_some_and(|id| id == &self.id) {
            self.layout = None;
        } else {
            self.layout = Some(layout);
        }
        self.active_pane = if active == self.id && self.layout.is_none() {
            None
        } else {
            Some(active)
        };
    }

    pub fn owns_pane(&self, pane_id: &str) -> bool {
        self.layout().leaves().iter().any(|id| id == pane_id)
    }
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
    crate::paths::config_dir().join("catalog.json")
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

/// What we know about the live `ws-main` tmux session when choosing an id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveSession {
    /// No session with that name.
    Absent,
    /// Session exists but path/env could not be read. Do not reuse.
    Opaque,
    /// Session exists; this is `SOLA_WS_PATH` or the pane cwd.
    Bound(PathBuf),
}

/// Candidate for adopting onto `ws-main`: session name, last activity, bind path.
#[derive(Clone, Debug)]
pub struct OrphanCandidate {
    pub name: String,
    pub activity: u64,
    pub path: Option<PathBuf>,
}

/// If `sws-ws-main` is missing, rename a leftover session that already
/// sits in **this** checkout and is not claimed by another catalog
/// workspace. Never adopts the newest session blindly — a sibling or
/// another project can be more recently active.
///
/// Returns the previous pane id so hooks from a still-running Grok
/// still match, or `None`.
pub fn adopt_orphan_session(workspace_path: &Path, catalog_ids: &HashSet<String>) -> Option<String> {
    let want = sola_terminal::tmux::session_name(LIVE_ID);
    if sola_terminal::tmux::has_session(&want) {
        return None;
    }
    let listed = sola_terminal::tmux::list_sessions_activity()?;
    let claimed: HashSet<String> = catalog_ids
        .iter()
        .map(|id| sola_terminal::tmux::session_name(id))
        .collect();
    let candidates: Vec<OrphanCandidate> = listed
        .into_iter()
        .map(|(name, activity)| OrphanCandidate {
            path: session_bind_path(&name),
            name,
            activity,
        })
        .collect();
    let old = pick_adoptable(&candidates, &want, workspace_path, &claimed)?;
    if !sola_terminal::tmux::rename_session(&old, &want) {
        tracing::warn!(from = %old, to = %want, "failed to adopt orphan tmux session");
        return None;
    }
    tracing::info!(from = %old, to = %want, "adopted orphan tmux session");
    sola_terminal::tmux::pane_id_from_session(&old)
}

/// Newest unclaimed session whose bind path belongs to `want_path`.
/// If `want` already exists, do nothing — attach verifies that session.
pub fn pick_adoptable(
    sessions: &[OrphanCandidate],
    want: &str,
    want_path: &Path,
    claimed: &HashSet<String>,
) -> Option<String> {
    if sessions.iter().any(|s| s.name == want) {
        return None;
    }
    sessions
        .iter()
        .filter(|s| !claimed.contains(&s.name))
        .filter(|s| s.path.as_deref().is_some_and(|p| path_same(p, want_path)))
        .max_by_key(|s| s.activity)
        .map(|s| s.name.clone())
}

/// Ensure `sws-{id}` is safe to `new-session -A`. A leftover session
/// from a deleted workspace (or another checkout that reused the id)
/// is renamed out of the way so this tab starts clean.
pub fn bind_session(id: &str, workspace_path: &Path) -> String {
    let name = sola_terminal::tmux::session_name(id);
    if session_is_free_or_ours(&name, workspace_path) {
        return name;
    }
    if quarantine_session(&name, id) {
        return name;
    }
    tracing::error!(
        session = %name,
        workspace = %workspace_path.display(),
        "tmux session belongs to another checkout and could not be quarantined"
    );
    String::new()
}

fn session_is_free_or_ours(session: &str, workspace_path: &Path) -> bool {
    if !sola_terminal::tmux::has_session(session) {
        return true;
    }
    session_matches_workspace(session, workspace_path)
}

fn session_matches_workspace(session: &str, workspace_path: &Path) -> bool {
    if let Some(stamped) = sola_terminal::tmux::get_environment(session, SOLA_WS_PATH) {
        return path_same(Path::new(&stamped), workspace_path);
    }
    sola_terminal::tmux::pane_current_path(session)
        .is_some_and(|cwd| path_belongs(Path::new(&cwd), workspace_path))
}

fn session_bind_path(session: &str) -> Option<PathBuf> {
    sola_terminal::tmux::get_environment(session, SOLA_WS_PATH)
        .or_else(|| sola_terminal::tmux::pane_current_path(session))
        .map(PathBuf::from)
}

fn quarantine_session(session: &str, id: &str) -> bool {
    let dest = unused_orphan_name(id);
    if !sola_terminal::tmux::rename_session(session, &dest) {
        return false;
    }
    tracing::warn!(
        from = %session,
        to = %dest,
        "quarantined tmux session (path does not match workspace)"
    );
    true
}

fn unused_orphan_name(id: &str) -> String {
    let base = sola_terminal::tmux::session_name(&format!("orphaned-{id}"));
    if !sola_terminal::tmux::has_session(&base) {
        return base;
    }
    for i in 2.. {
        let name = format!("{base}-{i}");
        if !sola_terminal::tmux::has_session(&name) {
            return name;
        }
    }
    base
}

/// First launch: one project from `cwd` when that folder is a git checkout.
pub fn seed_from_cwd() -> Option<(Project, Workspace)> {
    let cwd = std::env::current_dir().ok()?;
    if !spawn::is_git_checkout(&cwd) {
        return None;
    }
    let taken = HashSet::new();
    let main_id = main_workspace_id(&cwd, &taken);
    Some(project_from_root(&cwd, "proj-seed", &main_id))
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
        startup: String::new(),
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
        title: None,
        path: root.to_path_buf(),
        kind,
        parent: None,
        layout: None,
        active_pane: None,
        status: AgentStatus::Idle,
        agent: None,
    };
    (project, ws)
}

/// Reuse `ws-main` only when that tmux session is free or already in
/// this checkout. Stops a first-added other project from stealing a
/// leftover pane — including when the path query fails.
pub fn main_workspace_id(root: &Path, taken: &HashSet<String>) -> String {
    let session = sola_terminal::tmux::session_name(LIVE_ID);
    let live = if !sola_terminal::tmux::has_session(&session) {
        LiveSession::Absent
    } else if let Some(p) = session_bind_path(&session) {
        LiveSession::Bound(p)
    } else {
        LiveSession::Opaque
    };
    choose_main_id(root, taken, live)
}

pub fn choose_main_id(root: &Path, taken: &HashSet<String>, live: LiveSession) -> String {
    if !taken.contains(LIVE_ID) {
        match &live {
            LiveSession::Absent => return LIVE_ID.into(),
            LiveSession::Bound(p) if path_belongs(p, root) => return LIVE_ID.into(),
            LiveSession::Bound(_) | LiveSession::Opaque => {
                // Leave `ws-main` for the checkout that actually owns
                // the leftover session. unique_id would otherwise
                // return it because the catalog does not list it.
                let mut reserved = taken.clone();
                reserved.insert(LIVE_ID.into());
                return unique_id("ws", "main", &reserved);
            }
        }
    }
    unique_id("ws", "main", taken)
}

/// `got` is this workspace, or a directory under it (user `cd`'d in).
/// Component-aware — `/proj-old` does not belong to `/proj`.
pub fn path_belongs(got: &Path, workspace: &Path) -> bool {
    match (canon(got), canon(workspace)) {
        (Ok(g), Ok(w)) => g == w || g.starts_with(&w),
        _ => got == workspace || got.starts_with(workspace),
    }
}

/// Exact checkout — used when adopting a leftover onto `ws-main` so a
/// sibling under `.worktrees/` is not stolen.
pub fn path_same(a: &Path, b: &Path) -> bool {
    match (canon(a), canon(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

fn canon(p: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(p)
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
pub fn ordered_for_project<'a>(project_id: &str, all: &'a [Workspace]) -> Vec<&'a Workspace> {
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

/// Sibling worktrees can close from the row. The project's root
/// checkout cannot — drop the whole project instead (`unregister_project`).
pub fn can_close(ws: &Workspace) -> bool {
    ws.kind == Kind::Worktree
}

/// Unregister a project and every workspace under it. Returns the
/// workspace ids that left the catalog so the caller can kill tmux.
/// Does not touch git worktrees or folders on disk.
pub fn unregister_project(catalog: &mut Catalog, project_id: &str) -> Vec<String> {
    if !catalog.projects.iter().any(|p| p.id == project_id) {
        return Vec::new();
    }
    let removed: Vec<String> = catalog
        .workspaces
        .iter()
        .filter(|w| w.project_id == project_id)
        .map(|w| w.id.clone())
        .collect();
    catalog.workspaces.retain(|w| w.project_id != project_id);
    catalog.projects.retain(|p| p.id != project_id);
    if catalog
        .selected
        .as_ref()
        .is_some_and(|id| removed.iter().any(|r| r == id))
    {
        catalog.selected = catalog.workspaces.first().map(|w| w.id.clone());
    }
    removed
}

pub fn find_project<'a>(projects: &'a [Project], id: &str) -> Option<&'a Project> {
    projects.iter().find(|p| p.id == id)
}

/// Expand a leading `~` / `~/…` to `$HOME`. Other paths are unchanged.
pub fn expand_user_path(raw: &str) -> PathBuf {
    let raw = raw.trim();
    if raw == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from("~"));
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(raw)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
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
    if let Some(w) = workspaces.iter().find(|w| w.owns_pane(q)) {
        return Ok(w);
    }
    let as_path = Path::new(q);
    if as_path.is_absolute() {
        let hits: Vec<&Workspace> = workspaces
            .iter()
            .filter(|w| path_matches(w, as_path))
            .collect();
        match hits.as_slice() {
            [one] => return Ok(one),
            [] => {}
            _ => return Err(format!("workspace path '{q}' is ambiguous")),
        }
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

pub fn path_matches(ws: &Workspace, path: &Path) -> bool {
    if ws.path == path {
        return true;
    }
    match (ws.path.canonicalize(), path.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orphan(name: &str, activity: u64, path: Option<&str>) -> OrphanCandidate {
        OrphanCandidate {
            name: name.into(),
            activity,
            path: path.map(PathBuf::from),
        }
    }

    #[test]
    fn pick_adoptable_skips_when_stable_exists() {
        let want = "sws-ws-main";
        let listed = vec![
            orphan(want, 10, Some("/a")),
            orphan("sws-old", 99, Some("/a")),
        ];
        assert!(pick_adoptable(&listed, want, Path::new("/a"), &HashSet::new()).is_none());
    }

    #[test]
    fn pick_adoptable_ignores_newer_other_checkout() {
        let listed = vec![
            orphan("sws-old", 10, Some("/a")),
            orphan("sws-other", 99, Some("/b")),
        ];
        assert_eq!(
            pick_adoptable(&listed, "sws-ws-main", Path::new("/a"), &HashSet::new()).as_deref(),
            Some("sws-old")
        );
    }

    #[test]
    fn pick_adoptable_skips_claimed_sibling() {
        let listed = vec![
            orphan("sws-ws-kid", 80, Some("/a/.worktrees/kid")),
            orphan("sws-legacy", 10, Some("/a")),
        ];
        let mut claimed = HashSet::new();
        claimed.insert("sws-ws-kid".into());
        assert_eq!(
            pick_adoptable(&listed, "sws-ws-main", Path::new("/a"), &claimed).as_deref(),
            Some("sws-legacy")
        );
    }

    #[test]
    fn pick_adoptable_does_not_steal_unclaimed_sibling() {
        // Catalog no longer lists the sibling (deleted outside) but its
        // tmux is still in `.worktrees/`. That is not the project root.
        let listed = vec![orphan("sws-ws-kid", 80, Some("/a/.worktrees/kid"))];
        assert!(
            pick_adoptable(&listed, "sws-ws-main", Path::new("/a"), &HashSet::new()).is_none()
        );
    }

    #[test]
    fn pick_adoptable_none_when_only_foreign() {
        let listed = vec![orphan("sws-other", 50, Some("/b"))];
        assert!(
            pick_adoptable(&listed, "sws-ws-main", Path::new("/a"), &HashSet::new()).is_none()
        );
    }

    #[test]
    fn path_belongs_eq_and_descendant_not_prefix() {
        assert!(path_belongs(Path::new("/proj"), Path::new("/proj")));
        assert!(path_belongs(Path::new("/proj/src"), Path::new("/proj")));
        assert!(!path_belongs(Path::new("/proj-old"), Path::new("/proj")));
        assert!(!path_belongs(Path::new("/other"), Path::new("/proj")));
        assert!(path_same(Path::new("/proj"), Path::new("/proj")));
        assert!(!path_same(Path::new("/proj/src"), Path::new("/proj")));
    }

    #[test]
    fn choose_main_id_refuses_opaque_and_foreign() {
        let taken = HashSet::new();
        let root = Path::new("/proj");
        assert_eq!(choose_main_id(root, &taken, LiveSession::Absent), "ws-main");
        assert_eq!(
            choose_main_id(root, &taken, LiveSession::Bound(PathBuf::from("/proj"))),
            "ws-main"
        );
        assert_eq!(
            choose_main_id(root, &taken, LiveSession::Bound(PathBuf::from("/proj/src"))),
            "ws-main"
        );
        assert_eq!(
            choose_main_id(root, &taken, LiveSession::Bound(PathBuf::from("/other"))),
            "ws-main-2"
        );
        assert_eq!(
            choose_main_id(root, &taken, LiveSession::Opaque),
            "ws-main-2"
        );
        let mut taken = HashSet::new();
        taken.insert("ws-main".into());
        assert_eq!(choose_main_id(root, &taken, LiveSession::Absent), "ws-main-2");
    }

    #[test]
    fn resolve_project_by_name() {
        let projects = vec![Project {
            id: "proj-seed".into(),
            name: "Sola".into(),
            collapsed: false,
            root: PathBuf::from("/tmp/sola"),
            startup: String::new(),
        }];
        assert_eq!(resolve_project(&projects, "Sola").unwrap().id, "proj-seed");
        assert_eq!(resolve_project(&projects, "sola").unwrap().id, "proj-seed");
        assert!(resolve_project(&projects, "nope").is_err());
    }

    #[test]
    fn resolve_workspace_by_pane_and_path() {
        let ws = Workspace {
            id: "ws-kid".into(),
            project_id: "p".into(),
            name: "kid".into(),
            title: None,
            path: PathBuf::from("/tmp/sola-ws-kid-resolve"),
            kind: Kind::Worktree,
            parent: None,
            layout: Some(Layout::single("ws-kid-p2")),
            active_pane: Some("ws-kid-p2".into()),
            status: AgentStatus::Idle,
            agent: None,
        };
        let all = vec![ws];
        assert_eq!(resolve_workspace(&all, "kid").unwrap().id, "ws-kid");
        assert_eq!(resolve_workspace(&all, "ws-kid-p2").unwrap().id, "ws-kid");
        assert_eq!(
            resolve_workspace(&all, "/tmp/sola-ws-kid-resolve")
                .unwrap()
                .id,
            "ws-kid"
        );
        assert!(resolve_workspace(&all, "nope").is_err());
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
            title: None,
            path: PathBuf::from("/r"),
            kind: Kind::Main,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        };
        let child = Workspace {
            id: "ws-kid".into(),
            project_id: p.clone(),
            name: "kid".into(),
            title: None,
            path: PathBuf::from("/r/.worktrees/kid"),
            kind: Kind::Worktree,
            parent: Some("ws-main".into()),
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        };
        let other = Workspace {
            id: "ws-z".into(),
            project_id: p.clone(),
            name: "zeta".into(),
            title: None,
            path: PathBuf::from("/r/.worktrees/zeta"),
            kind: Kind::Worktree,
            parent: None,
            layout: None,
            active_pane: None,
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
    fn unregister_project_drops_group_keeps_others() {
        let mut c = Catalog::empty();
        c.selected = Some("ws-kid".into());
        c.projects.push(Project {
            id: "proj-a".into(),
            name: "A".into(),
            collapsed: false,
            root: PathBuf::from("/a"),
            startup: String::new(),
        });
        c.projects.push(Project {
            id: "proj-b".into(),
            name: "B".into(),
            collapsed: false,
            root: PathBuf::from("/b"),
            startup: String::new(),
        });
        c.workspaces.push(Workspace {
            id: "ws-main".into(),
            project_id: "proj-a".into(),
            name: "root".into(),
            title: None,
            path: PathBuf::from("/a"),
            kind: Kind::Main,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        });
        c.workspaces.push(Workspace {
            id: "ws-kid".into(),
            project_id: "proj-a".into(),
            name: "kid".into(),
            title: None,
            path: PathBuf::from("/a/.worktrees/kid"),
            kind: Kind::Worktree,
            parent: Some("ws-main".into()),
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        });
        c.workspaces.push(Workspace {
            id: "ws-b".into(),
            project_id: "proj-b".into(),
            name: "root".into(),
            title: None,
            path: PathBuf::from("/b"),
            kind: Kind::Main,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        });
        let gone = unregister_project(&mut c, "proj-a");
        assert_eq!(gone, ["ws-main", "ws-kid"]);
        assert_eq!(c.projects.len(), 1);
        assert_eq!(c.projects[0].id, "proj-b");
        assert_eq!(c.workspaces.len(), 1);
        assert_eq!(c.workspaces[0].id, "ws-b");
        assert_eq!(c.selected.as_deref(), Some("ws-b"));
        assert!(unregister_project(&mut c, "nope").is_empty());
    }

    #[test]
    fn only_worktrees_can_close() {
        let main = Workspace {
            id: "ws-main".into(),
            project_id: "p".into(),
            name: "root".into(),
            title: None,
            path: PathBuf::from("/r"),
            kind: Kind::Main,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        };
        let folder = Workspace {
            kind: Kind::Folder,
            ..main.clone()
        };
        let tree = Workspace {
            id: "ws-kid".into(),
            kind: Kind::Worktree,
            ..main.clone()
        };
        assert!(!can_close(&main));
        assert!(!can_close(&folder));
        assert!(can_close(&tree));
    }

    #[test]
    fn expand_user_path_tilde() {
        let home = std::env::var("HOME").expect("HOME");
        assert_eq!(expand_user_path("~"), PathBuf::from(&home));
        assert_eq!(
            expand_user_path("~/src/sola"),
            PathBuf::from(&home).join("src/sola")
        );
        assert_eq!(expand_user_path("  ~/a  "), PathBuf::from(&home).join("a"));
        assert_eq!(expand_user_path("/tmp/x"), PathBuf::from("/tmp/x"));
        assert_eq!(expand_user_path("~root"), PathBuf::from("~root"));
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
            startup: String::new(),
        });
        c.workspaces.push(Workspace {
            id: "ws-main".into(),
            project_id: "proj-seed".into(),
            name: "main".into(),
            title: None,
            path: PathBuf::from("/tmp/sola"),
            kind: Kind::Main,
            parent: None,
            layout: None,
            active_pane: None,
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
        assert!(back.workspaces[0].layout.is_none());
        assert!(back.workspaces[0].agent.is_none());
    }

    #[test]
    fn layout_omitted_is_single_leaf() {
        let ws = Workspace {
            id: "ws-main".into(),
            project_id: "p".into(),
            name: "root".into(),
            title: None,
            path: PathBuf::from("/r"),
            kind: Kind::Main,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        };
        assert_eq!(ws.layout().leaves(), ["ws-main"]);
        assert_eq!(ws.active_pane_id(), "ws-main");
        assert!(ws.owns_pane("ws-main"));
        assert!(!ws.owns_pane("other"));
    }

    #[test]
    fn set_tree_clears_trivial_layout() {
        let mut ws = Workspace {
            id: "ws-main".into(),
            project_id: "p".into(),
            name: "root".into(),
            title: None,
            path: PathBuf::from("/r"),
            kind: Kind::Main,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        };
        let split = PaneNode::Split {
            id: "s1".into(),
            dir: SplitDir::Horizontal,
            ratio: 0.5,
            a: Box::new(PaneNode::Leaf("ws-main".into())),
            b: Box::new(PaneNode::Leaf("ws-main-p".into())),
        };
        ws.set_tree(split, "ws-main-p".into());
        assert!(!ws.layout().is_single());
        assert_eq!(ws.active_pane_id(), "ws-main-p");
        ws.set_tree(PaneNode::Leaf("ws-main".into()), "ws-main".into());
        assert!(ws.layout.is_none());
        assert!(ws.active_pane.is_none());
    }
}
