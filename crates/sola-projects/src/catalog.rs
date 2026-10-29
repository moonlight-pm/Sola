//! The open projects, their tabs, catalog persist.
//!
//! A project is a folder in `~/Projects` (or in another computer's). The
//! catalog holds only the open ones: closing a project forgets it, and its
//! tabs.
//!
//! Live status stays off the catalog. Demo rows are gone — hooks
//! supply the marks. Worktree paths are `<root>/.worktrees/<slug>` (D4.2).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sola_bus::topics::SplitDir;
use sola_terminal::state::PaneNode;

use crate::machine::Machine;
use crate::spawn;
use crate::status::AgentStatus;

/// Stable pane / tmux id for a project's main checkout. Reused only
/// when the live `sws-ws-main` session is free or already in that
/// checkout — never stolen from another project.
pub const LIVE_ID: &str = "ws-main";

/// Stamped on every pane so restart can refuse a leftover session
/// from a deleted / other tab that reused the same id.
pub const SOLA_WS_PATH: &str = "SOLA_WS_PATH";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Main,
    Worktree,
    Folder,
    /// Extra tab in the project root checkout. No worktree; it works on
    /// whatever branch the root has checked out (usually master).
    Root,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub collapsed: bool,
    pub root: PathBuf,
    /// The computer `root` is on (an ssh host), when it is not this one.
    /// Its tabs are tmux sessions there ([`sola_terminal::remote`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Read from a catalog written before startup scripts moved to
    /// [`crate::startup`]'s store ([`load`] moves it there); never written.
    #[serde(default, skip_serializing)]
    pub startup: String,
    /// The tab its window last showed (each project has a window).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
    /// Read from a catalog written while closed projects were kept
    /// ([`load`] drops them); never written.
    #[serde(default, skip_serializing)]
    pub closed: bool,
}

/// Binary split tree. Omitted in the catalog when the tab is a
/// single leaf whose id is the tab id (the pre-split shape).
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
pub struct Tab {
    pub id: String,
    pub project_id: String,
    pub name: String,
    /// Rail display when set. Empty = show [`Self::name`] (the worktree slug).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub path: PathBuf,
    /// Its project's [`Project::host`]: the computer `path` is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
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

impl Tab {
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
    /// Saved as `workspaces` before the app was Projects.
    #[serde(alias = "workspaces")]
    pub tabs: Vec<Tab>,
}

impl Catalog {
    pub fn empty() -> Self {
        Self {
            version: 1,
            selected: None,
            projects: Vec::new(),
            tabs: Vec::new(),
        }
    }
}

/// CLI spawn is quiet unless `select`. UI / `--select` take the rail.
pub fn apply_spawn_focus(selected: &mut String, focused: &mut String, new_id: &str, select: bool) {
    if !select {
        return;
    }
    *selected = new_id.to_string();
    *focused = new_id.to_string();
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
            migrate(&mut c);
            c
        }
        Err(e) => {
            tracing::warn!("catalog parse failed: {e}");
            Catalog::empty()
        }
    }
}

/// An older catalog: a project's startup script moves to its store, and a
/// closed project (kept then) is forgotten with its tabs.
fn migrate(c: &mut Catalog) {
    for p in &mut c.projects {
        let script = std::mem::take(&mut p.startup);
        if !script.trim().is_empty() && crate::startup::script_for(p).is_empty() {
            if let Err(e) = crate::startup::set_script(p, &script) {
                tracing::warn!(project = %p.id, "startup script move failed: {e}");
            }
        }
    }
    let closed: Vec<String> = c.projects.iter().filter(|p| p.closed).map(|p| p.id.clone()).collect();
    for id in closed {
        tracing::info!(project = %id, "forgetting a closed project");
        unregister_project(c, &id);
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
/// tab. Never adopts the newest session blindly — a sibling or
/// another project can be more recently active.
///
/// Returns the previous pane id so hooks from a still-running Grok
/// still match, or `None`.
pub fn adopt_orphan_session(
    tab_path: &Path,
    catalog_ids: &HashSet<String>,
) -> Option<String> {
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
    let old = pick_adoptable(&candidates, &want, tab_path, &claimed)?;
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
/// from a deleted tab (or another checkout that reused the id)
/// is renamed out of the way so this tab starts clean.
pub fn bind_session(id: &str, tab_path: &Path) -> String {
    let name = sola_terminal::tmux::session_name(id);
    if session_is_free_or_ours(&name, tab_path) {
        return name;
    }
    if quarantine_session(&name, id) {
        return name;
    }
    tracing::error!(
        session = %name,
        tab = %tab_path.display(),
        "tmux session belongs to another checkout and could not be quarantined"
    );
    String::new()
}

fn session_is_free_or_ours(session: &str, tab_path: &Path) -> bool {
    if !sola_terminal::tmux::has_session(session) {
        return true;
    }
    session_matches_tab(session, tab_path)
}

fn session_matches_tab(session: &str, tab_path: &Path) -> bool {
    if let Some(stamped) = sola_terminal::tmux::get_environment(session, SOLA_WS_PATH) {
        return path_same(Path::new(&stamped), tab_path);
    }
    sola_terminal::tmux::pane_current_path(session)
        .is_some_and(|cwd| path_belongs(Path::new(&cwd), tab_path))
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
        "quarantined tmux session (path does not match tab)"
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

pub fn project_from_root(root: &Path, project_id: &str, main_id: &str) -> (Project, Tab) {
    let name = root
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("project")
        .to_string();
    let project = Project {
        selected: None,
        id: project_id.into(),
        name,
        collapsed: false,
        root: root.to_path_buf(),
        host: None,
        startup: String::new(),
        closed: false,
    };
    let kind = if spawn::is_git_checkout(&Machine::Local, root) {
        Kind::Main
    } else {
        Kind::Folder
    };
    let ws = Tab {
        id: main_id.into(),
        project_id: project.id.clone(),
        name: "root".into(),
        title: None,
        path: root.to_path_buf(),
        host: None,
        kind,
        parent: None,
        layout: None,
        active_pane: None,
        status: AgentStatus::Idle,
        agent: None,
    };
    (project, ws)
}

/// A project in folder `root` on `host`, and its root tab `main_id`.
/// Named for the folder and the computer ("Sola on novus"), so it never
/// takes a name a project here has.
pub fn project_from_remote(
    host: &str,
    root: &Path,
    project_id: &str,
    main_id: &str,
) -> (Project, Tab) {
    let folder = root
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("project");
    let project = Project {
        id: project_id.into(),
        name: format!("{folder} on {host}"),
        collapsed: false,
        root: root.to_path_buf(),
        host: Some(host.into()),
        startup: String::new(),
        selected: None,
        closed: false,
    };
    let ws = Tab {
        id: main_id.into(),
        project_id: project.id.clone(),
        name: "root".into(),
        title: None,
        path: root.to_path_buf(),
        host: Some(host.into()),
        kind: Kind::Main,
        parent: None,
        layout: None,
        active_pane: None,
        status: AgentStatus::Idle,
        agent: None,
    };
    (project, ws)
}

/// What an agent island calls a project: its name, but a project on
/// another computer by its folder alone ("Oath", not "Oath on buccina";
/// Joshua, 2026-10-29).
pub fn island_name(project: &Project) -> &str {
    if project.host.is_none() {
        return &project.name;
    }
    project
        .root
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(&project.name)
}

/// Reuse `ws-main` only when that tmux session is free or already in
/// this checkout. Stops a first-added other project from stealing a
/// leftover pane — including when the path query fails.
pub fn main_tab_id(root: &Path, taken: &HashSet<String>) -> String {
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

/// `got` is this tab, or a directory under it (user `cd`'d in).
/// Component-aware — `/proj-old` does not belong to `/proj`.
pub fn path_belongs(got: &Path, tab: &Path) -> bool {
    match (canon(got), canon(tab)) {
        (Ok(g), Ok(w)) => g == w || g.starts_with(&w),
        _ => got == tab || got.starts_with(tab),
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
pub fn lineage_depth(ws: &Tab, all: &[Tab]) -> u8 {
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

/// Catalog encounter order for this project (rail + `ps`). Drag-reorder
/// writes that order back; spawn appends.
pub fn ordered_for_project<'a>(project_id: &str, all: &'a [Tab]) -> Vec<&'a Tab> {
    all.iter().filter(|w| w.project_id == project_id).collect()
}

fn splice_ids(ids: &mut Vec<String>, id: &str, before: Option<&str>) {
    ids.retain(|x| x != id);
    let at = before
        .and_then(|b| ids.iter().position(|x| x == b))
        .unwrap_or(ids.len());
    ids.insert(at.min(ids.len()), id.to_string());
}

/// Where a tab dragged from `from` and let go over `onto` goes, as the
/// `before` of [`move_tab_before`]: it takes `onto`'s place, so after it
/// when dragged rightwards and before it when leftwards. `None` when
/// nothing moves.
pub fn drop_place(order: &[String], from: &str, onto: &str) -> Option<Option<String>> {
    let i = order.iter().position(|id| id == from)?;
    let j = order.iter().position(|id| id == onto)?;
    match i.cmp(&j) {
        std::cmp::Ordering::Equal => None,
        std::cmp::Ordering::Less => Some(order.get(j + 1).cloned()),
        std::cmp::Ordering::Greater => Some(Some(onto.to_string())),
    }
}

/// Reorder `id` among this project's tabs. `before` is another
/// tab in the same project (`None` = end).
pub fn move_tab_before(
    all: &mut Vec<Tab>,
    id: &str,
    before: Option<&str>,
) -> Result<(), String> {
    let project_id = all
        .iter()
        .find(|w| w.id == id)
        .map(|w| w.project_id.clone())
        .ok_or_else(|| format!("unknown tab '{id}'"))?;
    if let Some(b) = before {
        let other = all
            .iter()
            .find(|w| w.id == b)
            .ok_or_else(|| format!("unknown tab '{b}'"))?;
        if other.project_id != project_id {
            return Err("cannot move a tab into another project".into());
        }
        if other.id == id {
            return Ok(());
        }
    }
    let mut order: Vec<String> = all
        .iter()
        .filter(|w| w.project_id == project_id)
        .map(|w| w.id.clone())
        .collect();
    splice_ids(&mut order, id, before);
    apply_tab_order(all, &project_id, &order);
    Ok(())
}

fn apply_tab_order(all: &mut Vec<Tab>, project_id: &str, order: &[String]) {
    let mut mine: Vec<Tab> = Vec::new();
    let mut slots = Vec::new();
    for (i, w) in all.iter().enumerate() {
        if w.project_id == project_id {
            slots.push(i);
            mine.push(w.clone());
        }
    }
    let mut next = Vec::new();
    for id in order {
        if let Some(j) = mine.iter().position(|w| w.id == *id) {
            next.push(mine.remove(j));
        }
    }
    next.extend(mine);
    for (slot, w) in slots.into_iter().zip(next) {
        all[slot] = w;
    }
}

/// Sibling worktrees and root tabs can close from the row. The project's
/// main tab cannot — drop the whole project instead (`unregister_project`).
pub fn can_close(ws: &Tab) -> bool {
    matches!(ws.kind, Kind::Worktree | Kind::Root)
}

/// This tab owns a `.worktrees/<slug>` checkout (rename / branch /
/// `git worktree remove` apply). Root tabs share the project root.
pub fn has_worktree(ws: &Tab) -> bool {
    ws.kind == Kind::Worktree
}

/// Another tab in this project already uses this worktree slug or
/// `.worktrees/<slug>` folder. `except_id` is the row being renamed.
pub fn worktree_name_taken(
    tabs: &[Tab],
    project_id: &str,
    slug: &str,
    except_id: &str,
) -> bool {
    tabs.iter().any(|w| {
        w.id != except_id
            && w.project_id == project_id
            && (w.name.eq_ignore_ascii_case(slug)
                || w.path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case(slug)))
    })
}

/// Worktree tabs whose checkout is already gone. The rail should reap
/// them — `git worktree remove` from inside the pane cannot call
/// `tab.rm` afterward (cwd vanished).
pub fn missing_worktree_ids(tabs: &[Tab]) -> Vec<String> {
    tabs
        .iter()
        .filter(|w| w.host.is_none() && has_worktree(w) && !w.path.exists())
        .map(|w| w.id.clone())
        .collect()
}

/// Unregister a project and every tab under it. Returns the
/// tab ids that left the catalog so the caller can kill tmux.
/// Does not touch git worktrees or folders on disk.
pub fn unregister_project(catalog: &mut Catalog, project_id: &str) -> Vec<String> {
    if !catalog.projects.iter().any(|p| p.id == project_id) {
        return Vec::new();
    }
    let removed: Vec<String> = catalog
        .tabs
        .iter()
        .filter(|w| w.project_id == project_id)
        .map(|w| w.id.clone())
        .collect();
    catalog.tabs.retain(|w| w.project_id != project_id);
    catalog.projects.retain(|p| p.id != project_id);
    if catalog
        .selected
        .as_ref()
        .is_some_and(|id| removed.iter().any(|r| r == id))
    {
        catalog.selected = catalog.tabs.first().map(|w| w.id.clone());
    }
    removed
}

pub fn find_project<'a>(projects: &'a [Project], id: &str) -> Option<&'a Project> {
    projects.iter().find(|p| p.id == id)
}

/// `~/Projects`: every project here is a folder in it.
pub fn projects_dir() -> PathBuf {
    home_dir().unwrap_or_else(|| PathBuf::from("/")).join("Projects")
}

/// The folders of `~/Projects` (not hidden ones), by name, each with its
/// canonical path.
pub fn project_folders() -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(projects_dir()) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let path = canon(&e.path()).ok()?;
            (!name.starts_with('.') && path.is_dir()).then_some((name, path))
        })
        .collect();
    out.sort_by_key(|(name, _)| name.to_lowercase());
    out
}

/// `root` (canonical) is a folder of `~/Projects`.
pub fn in_projects_dir(root: &Path) -> bool {
    let dir = projects_dir();
    let dir = canon(&dir).unwrap_or(dir);
    root.parent() == Some(dir.as_path())
}

/// A new project's folder name, or why not.
pub fn check_project_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name the project".into());
    }
    if name.starts_with('.') || name.contains('/') || name.contains('\0') {
        return Err(format!("“{name}” can't be a folder name"));
    }
    Ok(name)
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

/// A folder on another computer: its full path there (as the launcher's
/// listing gives it). `~` would be this computer's home, so it is refused.
pub fn remote_root(path: &str) -> Result<PathBuf, String> {
    let root = PathBuf::from(path.trim().trim_end_matches('/'));
    if !root.is_absolute() || root.file_name().is_none() {
        return Err(format!("“{}” is not a folder's full path there", path.trim()));
    }
    Ok(root)
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

pub fn resolve_tab<'a>(
    tabs: &'a [Tab],
    q: &str,
) -> Result<&'a Tab, String> {
    if let Some(w) = tabs.iter().find(|w| w.id == q) {
        return Ok(w);
    }
    if let Some(w) = tabs.iter().find(|w| w.owns_pane(q)) {
        return Ok(w);
    }
    let as_path = Path::new(q);
    if as_path.is_absolute() {
        let hits: Vec<&Tab> = tabs
            .iter()
            .filter(|w| path_matches(w, as_path))
            .collect();
        match hits.as_slice() {
            [one] => return Ok(one),
            [] => {}
            _ => return Err(format!("tab path '{q}' is ambiguous")),
        }
    }
    let hits: Vec<&Tab> = tabs
        .iter()
        .filter(|w| w.name.eq_ignore_ascii_case(q))
        .collect();
    match hits.as_slice() {
        [one] => return Ok(one),
        [] => {}
        _ => return Err(format!("tab '{q}' is ambiguous")),
    }
    let hits: Vec<&Tab> = tabs
        .iter()
        .filter(|w| {
            w.title
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_some_and(|t| t.eq_ignore_ascii_case(q))
        })
        .collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("unknown tab '{q}'")),
        _ => Err(format!("tab '{q}' is ambiguous")),
    }
}

pub fn find_tab_mut<'a>(
    tabs: &'a mut [Tab],
    id: &str,
) -> Option<&'a mut Tab> {
    tabs.iter_mut().find(|w| w.id == id)
}

/// `ws` is the checkout at `path` here. A tab on another computer never
/// is, whatever its path.
pub fn path_matches(ws: &Tab, path: &Path) -> bool {
    if ws.host.is_some() {
        return false;
    }
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
    fn cli_spawn_leaves_selected_on_previous() {
        let mut selected = "ws-main".into();
        let mut focused = "ws-main".into();
        apply_spawn_focus(&mut selected, &mut focused, "ws-bg-test", false);
        assert_eq!(selected, "ws-main");
        assert_eq!(focused, "ws-main");
    }

    #[test]
    fn select_and_ui_spawn_switch_rail() {
        let mut selected = "ws-main".into();
        let mut focused = "ws-main".into();
        apply_spawn_focus(&mut selected, &mut focused, "ws-bg-test", true);
        assert_eq!(selected, "ws-bg-test");
        assert_eq!(focused, "ws-bg-test");
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
        assert!(pick_adoptable(&listed, "sws-ws-main", Path::new("/a"), &HashSet::new()).is_none());
    }

    #[test]
    fn pick_adoptable_none_when_only_foreign() {
        let listed = vec![orphan("sws-other", 50, Some("/b"))];
        assert!(pick_adoptable(&listed, "sws-ws-main", Path::new("/a"), &HashSet::new()).is_none());
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
        assert_eq!(
            choose_main_id(root, &taken, LiveSession::Absent),
            "ws-main-2"
        );
    }

    #[test]
    fn resolve_project_by_name() {
        let projects = vec![Project {
            selected: None,
            id: "proj-seed".into(),
            name: "Sola".into(),
            collapsed: false,
            root: PathBuf::from("/tmp/sola"),
            host: None,
            startup: String::new(),
            closed: false,
        }];
        assert_eq!(resolve_project(&projects, "Sola").unwrap().id, "proj-seed");
        assert_eq!(resolve_project(&projects, "sola").unwrap().id, "proj-seed");
        assert!(resolve_project(&projects, "nope").is_err());
    }

    #[test]
    fn resolve_tab_by_pane_and_path() {
        let ws = Tab {
            id: "ws-kid".into(),
            project_id: "p".into(),
            name: "kid".into(),
            title: None,
            path: PathBuf::from("/tmp/sola-ws-kid-resolve"),
            host: None,
            kind: Kind::Worktree,
            parent: None,
            layout: Some(Layout::single("ws-kid-p2")),
            active_pane: Some("ws-kid-p2".into()),
            status: AgentStatus::Idle,
            agent: None,
        };
        let all = vec![ws];
        assert_eq!(resolve_tab(&all, "kid").unwrap().id, "ws-kid");
        assert_eq!(resolve_tab(&all, "ws-kid-p2").unwrap().id, "ws-kid");
        assert_eq!(
            resolve_tab(&all, "/tmp/sola-ws-kid-resolve")
                .unwrap()
                .id,
            "ws-kid"
        );
        assert!(resolve_tab(&all, "nope").is_err());
    }

    #[test]
    fn resolve_tab_by_title_after_name() {
        let named = Tab {
            id: "ws-kid".into(),
            project_id: "p".into(),
            name: "kid".into(),
            title: Some("fix login".into()),
            path: PathBuf::from("/tmp/sola-ws-kid-title"),
            host: None,
            kind: Kind::Worktree,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        };
        let other = Tab {
            id: "ws-other".into(),
            project_id: "p".into(),
            name: "other".into(),
            title: Some("kid".into()),
            path: PathBuf::from("/tmp/sola-ws-other-title"),
            host: None,
            kind: Kind::Worktree,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        };
        let all = vec![named, other];
        assert_eq!(resolve_tab(&all, "kid").unwrap().id, "ws-kid");
        assert_eq!(resolve_tab(&all, "fix login").unwrap().id, "ws-kid");
        assert_eq!(resolve_tab(&all, "other").unwrap().id, "ws-other");
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
        let main = Tab {
            id: "ws-main".into(),
            project_id: p.clone(),
            name: "main".into(),
            title: None,
            path: PathBuf::from("/r"),
            host: None,
            kind: Kind::Main,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        };
        let child = Tab {
            id: "ws-kid".into(),
            project_id: p.clone(),
            name: "kid".into(),
            title: None,
            path: PathBuf::from("/r/.worktrees/kid"),
            host: None,
            kind: Kind::Worktree,
            parent: Some("ws-main".into()),
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        };
        let other = Tab {
            id: "ws-z".into(),
            project_id: p.clone(),
            name: "zeta".into(),
            title: None,
            path: PathBuf::from("/r/.worktrees/zeta"),
            host: None,
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
        assert_eq!(ids, ["ws-z", "ws-kid", "ws-main"]);
        assert_eq!(lineage_depth(&child, &all), 1);
        assert_eq!(lineage_depth(&main, &all), 0);
    }

    fn ws(id: &str, project: &str, name: &str, kind: Kind) -> Tab {
        Tab {
            id: id.into(),
            project_id: project.into(),
            name: name.into(),
            title: None,
            path: PathBuf::from(format!("/{name}")),
            host: None,
            kind,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        }
    }

    #[test]
    fn move_tab_before_stays_in_project() {
        let mut all = vec![
            ws("a-root", "a", "root", Kind::Main),
            ws("a-kid", "a", "kid", Kind::Worktree),
            ws("b-root", "b", "root", Kind::Main),
            ws("a-z", "a", "z", Kind::Worktree),
        ];
        move_tab_before(&mut all, "a-z", Some("a-kid")).unwrap();
        let ids: Vec<&str> = ordered_for_project("a", &all)
            .into_iter()
            .map(|w| w.id.as_str())
            .collect();
        assert_eq!(ids, ["a-root", "a-z", "a-kid"]);
        assert_eq!(all[2].id, "b-root");
        assert!(move_tab_before(&mut all, "a-z", Some("b-root")).is_err());
    }

    #[test]
    fn unregister_project_drops_group_keeps_others() {
        let mut c = Catalog::empty();
        c.selected = Some("ws-kid".into());
        c.projects.push(Project {
            selected: None,
            id: "proj-a".into(),
            name: "A".into(),
            collapsed: false,
            root: PathBuf::from("/a"),
            host: None,
            startup: String::new(),
            closed: false,
        });
        c.projects.push(Project {
            selected: None,
            id: "proj-b".into(),
            name: "B".into(),
            collapsed: false,
            root: PathBuf::from("/b"),
            host: None,
            startup: String::new(),
            closed: false,
        });
        c.tabs.push(Tab {
            id: "ws-main".into(),
            project_id: "proj-a".into(),
            name: "root".into(),
            title: None,
            path: PathBuf::from("/a"),
            host: None,
            kind: Kind::Main,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        });
        c.tabs.push(Tab {
            id: "ws-kid".into(),
            project_id: "proj-a".into(),
            name: "kid".into(),
            title: None,
            path: PathBuf::from("/a/.worktrees/kid"),
            host: None,
            kind: Kind::Worktree,
            parent: Some("ws-main".into()),
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        });
        c.tabs.push(Tab {
            id: "ws-b".into(),
            project_id: "proj-b".into(),
            name: "root".into(),
            title: None,
            path: PathBuf::from("/b"),
            host: None,
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
        assert_eq!(c.tabs.len(), 1);
        assert_eq!(c.tabs[0].id, "ws-b");
        assert_eq!(c.selected.as_deref(), Some("ws-b"));
        assert!(unregister_project(&mut c, "nope").is_empty());
    }

    #[test]
    fn worktrees_and_root_tabs_can_close() {
        let main = Tab {
            id: "ws-main".into(),
            project_id: "p".into(),
            name: "root".into(),
            title: None,
            path: PathBuf::from("/r"),
            host: None,
            kind: Kind::Main,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        };
        let folder = Tab {
            kind: Kind::Folder,
            ..main.clone()
        };
        let tree = Tab {
            id: "ws-kid".into(),
            kind: Kind::Worktree,
            ..main.clone()
        };
        let root_tab = Tab {
            id: "ws-hotfix".into(),
            kind: Kind::Root,
            ..main.clone()
        };
        assert!(!can_close(&main));
        assert!(!can_close(&folder));
        assert!(can_close(&tree));
        assert!(can_close(&root_tab));
        assert!(has_worktree(&tree));
        assert!(!has_worktree(&root_tab));
        assert!(!has_worktree(&main));
    }

    #[test]
    fn missing_worktree_ids_skips_root_and_live_paths() {
        let dir = std::env::temp_dir().join(format!(
            "ws-missing-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let live = Tab {
            id: "ws-kid".into(),
            project_id: "p".into(),
            name: "kid".into(),
            title: None,
            path: dir.clone(),
            host: None,
            kind: Kind::Worktree,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        };
        let gone = Tab {
            id: "ws-gone".into(),
            path: dir.join("nope"),
            ..live.clone()
        };
        let root = Tab {
            id: "ws-main".into(),
            path: PathBuf::from("/no/such/root"),
            kind: Kind::Main,
            ..live.clone()
        };
        let root_tab = Tab {
            id: "ws-hotfix".into(),
            path: PathBuf::from("/no/such/root"),
            kind: Kind::Root,
            ..live.clone()
        };
        assert!(missing_worktree_ids(&[live.clone(), root, root_tab]).is_empty());
        assert_eq!(missing_worktree_ids(&[live, gone]), ["ws-gone"]);
        let _ = std::fs::remove_dir_all(&dir);
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
    fn a_remote_project_keeps_its_host_and_never_matches_a_path_here() {
        let root = std::env::temp_dir();
        let (project, tab) = project_from_remote("novus", &root, "proj-novus-tmp", "ws-novus-tmp");
        let folder = root.file_name().unwrap().to_str().unwrap();
        assert_eq!(project.name, format!("{folder} on novus"));
        assert_eq!(island_name(&project), folder);
        assert_eq!(tab.host.as_deref(), Some("novus"));
        assert_eq!(tab.kind, Kind::Main);
        // The same path here is not this tab.
        assert!(!path_matches(&tab, &root));
        let local = Tab {
            host: None,
            ..tab.clone()
        };
        assert!(path_matches(&local, &root));
        // The host round-trips; a project here saves none.
        let text = serde_json::to_string(&project).unwrap();
        assert!(text.contains("\"host\":\"novus\""));
        let back: Project = serde_json::from_str(&text).unwrap();
        assert_eq!(back.host.as_deref(), Some("novus"));
        let here = Project { host: None, ..project };
        assert!(!serde_json::to_string(&here).unwrap().contains("host"));
        assert_eq!(island_name(&here), here.name);
    }

    #[test]
    fn an_older_catalog_forgets_its_closed_projects() {
        let text = r#"{"selected":"ws-b","projects":[
            {"id":"proj-a","name":"A","root":"/p/A"},
            {"id":"proj-b","name":"B","root":"/p/B","closed":true}],
          "tabs":[
            {"id":"ws-a","project_id":"proj-a","name":"root","path":"/p/A","kind":"main"},
            {"id":"ws-b","project_id":"proj-b","name":"root","path":"/p/B","kind":"main"}]}"#;
        let mut c: Catalog = serde_json::from_str(text).unwrap();
        migrate(&mut c);
        assert_eq!(c.projects.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["proj-a"]);
        assert_eq!(c.tabs.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["ws-a"]);
        assert_eq!(c.selected.as_deref(), Some("ws-a"));
        // Neither is ever written again.
        let saved = serde_json::to_string(&c).unwrap();
        assert!(!saved.contains("closed") && !saved.contains("startup"));
    }

    #[test]
    fn a_remote_root_is_a_full_path() {
        assert_eq!(
            remote_root(" /home/j/Projects/Sola/ "),
            Ok(PathBuf::from("/home/j/Projects/Sola"))
        );
        assert!(remote_root("~/Projects/Sola").is_err());
        assert!(remote_root("Projects/Sola").is_err());
        assert!(remote_root("/").is_err());
    }

    #[test]
    fn catalog_round_trip_skips_live_status() {
        let mut c = Catalog::empty();
        c.version = 1;
        c.selected = Some("ws-main".into());
        c.projects.push(Project {
            selected: None,
            id: "proj-seed".into(),
            name: "Sola".into(),
            collapsed: true,
            root: PathBuf::from("/tmp/sola"),
            host: None,
            startup: String::new(),
            closed: false,
        });
        c.tabs.push(Tab {
            id: "ws-main".into(),
            project_id: "proj-seed".into(),
            name: "main".into(),
            title: None,
            path: PathBuf::from("/tmp/sola"),
            host: None,
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
        assert_eq!(back.tabs[0].kind, Kind::Main);
        assert_eq!(back.tabs[0].status, AgentStatus::Idle);
        assert!(back.tabs[0].layout.is_none());
        assert!(back.tabs[0].agent.is_none());
    }

    #[test]
    fn layout_omitted_is_single_leaf() {
        let ws = Tab {
            id: "ws-main".into(),
            project_id: "p".into(),
            name: "root".into(),
            title: None,
            path: PathBuf::from("/r"),
            host: None,
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
        let mut ws = Tab {
            id: "ws-main".into(),
            project_id: "p".into(),
            name: "root".into(),
            title: None,
            path: PathBuf::from("/r"),
            host: None,
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

    #[test]
    fn worktree_name_taken_matches_name_or_folder() {
        let kid = Tab {
            id: "ws-kid".into(),
            project_id: "p".into(),
            name: "adhoc".into(),
            title: None,
            path: PathBuf::from("/r/.worktrees/adhoc"),
            host: None,
            kind: Kind::Worktree,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        };
        let other = Tab {
            id: "ws-other".into(),
            project_id: "p".into(),
            name: "Fix Login".into(),
            title: None,
            path: PathBuf::from("/r/.worktrees/fix-login"),
            host: None,
            kind: Kind::Worktree,
            parent: None,
            layout: None,
            active_pane: None,
            status: AgentStatus::Idle,
            agent: None,
        };
        let all = [kid, other];
        assert!(worktree_name_taken(&all, "p", "adhoc", "ws-new"));
        assert!(worktree_name_taken(&all, "p", "fix-login", "ws-new"));
        assert!(!worktree_name_taken(&all, "p", "adhoc", "ws-kid"));
        assert!(!worktree_name_taken(&all, "p", "sc-1234", "ws-kid"));
        assert!(!worktree_name_taken(&all, "other", "adhoc", "ws-new"));
    }

    #[test]
    fn a_catalog_saved_as_workspaces_still_loads() {
        let c: Catalog = serde_json::from_str(
            r#"{"version":1,"selected":"ws-main","projects":[],"workspaces":[
                {"id":"ws-main","project_id":"proj-sola","name":"root",
                 "path":"/tmp/sola","kind":"main"}]}"#,
        )
        .unwrap();
        assert_eq!(c.tabs.len(), 1);
        assert_eq!(c.tabs[0].id, "ws-main");
        assert!(serde_json::to_string(&c).unwrap().contains("\"tabs\""));
    }

    #[test]
    fn a_dropped_tab_takes_the_place_it_lands_on() {
        let order: Vec<String> = ["root", "a", "b", "c"].map(String::from).to_vec();
        // Rightwards: after the target.
        assert_eq!(drop_place(&order, "a", "b"), Some(Some("c".into())));
        assert_eq!(drop_place(&order, "a", "c"), Some(None));
        // Leftwards: before the target.
        assert_eq!(drop_place(&order, "c", "a"), Some(Some("a".into())));
        assert_eq!(drop_place(&order, "b", "b"), None);
        assert_eq!(drop_place(&order, "x", "b"), None);
    }
}
