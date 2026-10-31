//! A Projects window with no session yet: where a project is opened from
//! (Joshua, 2026-10-31: "the launcher should just open a project window
//! (and load the projects app by doing so, if it wasn't already).. then in
//! that new projects window, we'll navigate to the project to open. Esc (at
//! the top level of the flow) would simple close the window").
//!
//! The launcher's Projects row asks for one (`window.navigate`), as do
//! Project → Open Project… and New Project…. It lists the folders of
//! `~/Projects`, each with the sessions running in it on its computer, then
//! New and **On another computer…**: a host field (recent hosts first),
//! then that computer's `~/Projects` the same way ([`hosts`]). Picking a
//! folder opens the project as `project.open` does, a session opens it
//! alone; the navigator then closes and the session's window takes its
//! place. Esc goes back a step; at the top it closes the window. This was
//! the shell launcher's chooser until 2026-10-31.

mod glue;
pub mod hosts;
pub mod view;

use std::path::{Path, PathBuf};

use crate::sessions::HostSession;
use hosts::{Hosts, Listing};

/// A folder of `~/Projects` (here, or on a host).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    /// Canonical here; its full path there for a host's.
    pub path: PathBuf,
}

/// A row of the folder list, in the order shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// `folders[i]`.
    Folder(usize),
    /// `sessions[s]`, running in `folders[i]`.
    Session(usize, usize),
    /// Make a new project.
    New,
    /// Open a project on another computer.
    Remote,
}

/// One computer's `~/Projects` and the sessions running there.
#[derive(Debug, Default)]
pub struct Folders {
    /// `None`: this computer.
    pub host: Option<String>,
    /// Its `~/Projects` (the full path there, for a host).
    pub root: PathBuf,
    pub folders: Vec<Folder>,
    pub sessions: Vec<HostSession>,
    /// `folders` passing the query, in order.
    filtered: Vec<usize>,
    query: String,
    /// Asking for the new project's name (the field is the name).
    pub naming: bool,
}

impl Folders {
    pub fn new(host: Option<String>, root: PathBuf, folders: Vec<Folder>, sessions: Vec<HostSession>) -> Self {
        let mut f = Folders {
            host,
            root,
            folders,
            sessions,
            ..Folders::default()
        };
        f.apply_query("");
        f
    }

    /// This computer's.
    pub fn local() -> Self {
        let folders = crate::catalog::project_folders()
            .into_iter()
            .map(|(name, path)| Folder { name, path })
            .collect();
        let sessions = crate::sessions::list_local().unwrap_or_else(|e| {
            tracing::warn!("navigator: this computer's sessions: {e}");
            Vec::new()
        });
        Folders::new(None, crate::catalog::projects_dir(), folders, sessions)
    }

    /// A host's, as [`hosts::list`] found it.
    pub fn remote(host: &str, listing: Listing) -> Self {
        let folders = listing
            .names
            .into_iter()
            .map(|name| Folder {
                path: listing.root.join(&name),
                name,
            })
            .collect();
        Folders::new(Some(host.to_string()), listing.root, folders, listing.sessions)
    }

    /// The sessions running in folder `f`, in listing order.
    pub fn sessions_of(&self, f: usize) -> Vec<usize> {
        let folder = &self.folders[f].path;
        (0..self.sessions.len())
            .filter(|&s| self.sessions[s].project.as_deref() == Some(folder.as_path()))
            .collect()
    }

    /// Keep the folders whose name holds `query` (case aside).
    pub fn apply_query(&mut self, query: &str) {
        self.query = query.trim().to_string();
        let q = self.query.to_lowercase();
        self.filtered = (0..self.folders.len())
            .filter(|&i| q.is_empty() || self.folders[i].name.to_lowercase().contains(&q))
            .collect();
    }

    /// The matching folders with their sessions, then New, then Remote
    /// (this computer only). Naming: New alone.
    pub fn rows(&self) -> Vec<Row> {
        if self.naming {
            return vec![Row::New];
        }
        let mut rows: Vec<Row> = Vec::new();
        for &i in &self.filtered {
            rows.push(Row::Folder(i));
            rows.extend(self.sessions_of(i).into_iter().map(|s| Row::Session(i, s)));
        }
        rows.push(Row::New);
        if self.host.is_none() {
            rows.push(Row::Remote);
        }
        rows
    }

    /// The name New would make: the query, unless it is already a folder.
    pub fn new_name(&self) -> Option<&str> {
        let q = self.query.as_str();
        let taken = self.folders.iter().any(|f| f.name == q);
        (!q.is_empty() && !taken).then_some(q)
    }
}

/// What a pick asks Projects to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Nothing,
    /// `project.open` (`session`: that one alone).
    Open {
        path: PathBuf,
        host: Option<String>,
        session: Option<String>,
    },
    /// Make folder `name` in `root` (on `host`), then open it.
    Create {
        name: String,
        root: PathBuf,
        host: Option<String>,
    },
    /// List this host's `~/Projects` (over ssh, off the UI thread).
    List(String),
}

/// What a navigator waits on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Busy {
    /// Listing a host's `~/Projects`.
    Listing(String),
    /// Making a folder, then opening it.
    Creating(String),
    /// Opening a project (on a host: listing its sessions there).
    Opening(String),
}

impl Busy {
    pub fn note(&self) -> String {
        match self {
            Busy::Listing(host) => format!("Connecting to {host}…"),
            Busy::Creating(name) => format!("Making {name}…"),
            Busy::Opening(name) => format!("Opening {name}…"),
        }
    }
}

/// One navigator window's state.
#[derive(Debug, Default)]
pub struct Navigator {
    /// What the field holds.
    pub query: String,
    pub selected: usize,
    pub folders: Folders,
    /// The host stage, when picking a computer.
    pub hosts: Option<Hosts>,
    /// Waiting on ssh or a new folder: the rows wait too.
    pub busy: Option<Busy>,
    /// Why the last pick did not open.
    pub error: Option<String>,
}

impl Navigator {
    pub fn new(folders: Folders, naming: bool) -> Self {
        let mut n = Navigator {
            folders,
            ..Navigator::default()
        };
        n.folders.naming = naming;
        n
    }

    /// How many rows can be picked.
    pub fn len(&self) -> usize {
        if self.busy.is_some() {
            return 0;
        }
        match &self.hosts {
            Some(h) => h.rows().len(),
            None => self.folders.rows().len(),
        }
    }

    pub fn set_query(&mut self, query: String) {
        match &mut self.hosts {
            Some(h) => h.apply_query(&query),
            None => self.folders.apply_query(&query),
        }
        self.query = query;
        self.selected = 0;
        self.error = None;
    }

    /// Up (`-1`) or down (`1`), held to the rows.
    pub fn step(&mut self, by: i32) {
        let n = self.len();
        if n == 0 {
            return;
        }
        let at = self.selected as i32 + by;
        self.selected = at.clamp(0, n as i32 - 1) as usize;
    }

    /// Back a step: naming → the list, a host's list → the host field, the
    /// host field → this computer's list. False at the top: close.
    pub fn back(&mut self) -> bool {
        if self.busy.take().is_some() {
            return true;
        }
        if self.hosts.is_some() {
            self.hosts = None;
            self.folders = Folders::local();
        } else if self.folders.naming {
            self.folders.naming = false;
        } else if let Some(host) = self.folders.host.take() {
            self.hosts = Some(Hosts::new(hosts::load_recent()));
            self.set_query(host);
            return true;
        } else {
            return false;
        }
        self.set_query(String::new());
        true
    }

    /// Row `i` picked (Enter, or a click).
    pub fn pick(&mut self, i: usize) -> Pick {
        if let Some(hosts) = &self.hosts {
            return match hosts.rows().get(i).and_then(|r| hosts.host(*r)) {
                Some(host) => Pick::List(host.to_string()),
                None => Pick::Nothing,
            };
        }
        let f = &self.folders;
        match f.rows().get(i).copied() {
            Some(Row::Folder(at)) => Pick::Open {
                path: f.folders[at].path.clone(),
                host: f.host.clone(),
                session: None,
            },
            Some(Row::Session(at, s)) => Pick::Open {
                path: f.folders[at].path.clone(),
                host: f.host.clone(),
                session: Some(f.sessions[s].id.clone()),
            },
            Some(Row::New) => match (f.naming, f.new_name()) {
                (true, _) if !self.query.trim().is_empty() => self.create(self.query.trim().to_string()),
                (false, Some(name)) => self.create(name.to_string()),
                _ => {
                    self.folders.naming = true;
                    self.set_query(String::new());
                    Pick::Nothing
                }
            },
            Some(Row::Remote) => {
                self.hosts = Some(Hosts::new(hosts::load_recent()));
                self.set_query(String::new());
                Pick::Nothing
            }
            None => Pick::Nothing,
        }
    }

    fn create(&self, name: String) -> Pick {
        Pick::Create {
            name,
            root: self.folders.root.clone(),
            host: self.folders.host.clone(),
        }
    }

    /// `host`'s listing came back: its folders, or why not. One Esc
    /// cancelled is dropped.
    pub fn listed(&mut self, host: &str, listing: Result<Listing, String>) {
        if self.busy != Some(Busy::Listing(host.to_string())) {
            return;
        }
        self.busy = None;
        match listing {
            Ok(listing) => {
                hosts::remember(host);
                self.hosts = None;
                self.folders = Folders::remote(host, listing);
                self.set_query(String::new());
            }
            Err(e) => {
                if let Some(h) = &mut self.hosts {
                    h.error = Some(e);
                }
            }
        }
    }

    /// The field's hint for the step it is on.
    pub fn placeholder(&self) -> &'static str {
        match (&self.hosts, self.folders.naming) {
            (Some(_), _) => "A computer's name",
            (None, true) => "A name for its folder",
            (None, false) => "Find a project",
        }
    }

    /// The step's heading: "Projects", "New project", "On another
    /// computer", or "Projects on <host>".
    pub fn heading(&self) -> String {
        match (&self.hosts, self.folders.naming, &self.folders.host) {
            (Some(_), ..) => "On another computer".into(),
            (None, true, None) => "New project".into(),
            (None, true, Some(h)) => format!("New project on {h}"),
            (None, false, Some(h)) => format!("Projects on {h}"),
            (None, false, None) => "Projects".into(),
        }
    }
}

/// Make `root/name` here; an existing folder is used as it is.
pub fn create_local(root: &Path, name: &str) -> Result<PathBuf, String> {
    let name = crate::catalog::check_project_name(name)?;
    let path = root.join(name);
    std::fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> Folder {
        Folder {
            name: name.into(),
            path: PathBuf::from(format!("/p/{name}")),
        }
    }

    fn session(id: &str, project: &str) -> HostSession {
        HostSession {
            id: id.into(),
            project: Some(PathBuf::from(project)),
            name: None,
            title: None,
            kind: None,
        }
    }

    fn here(names: &[&str], sessions: Vec<HostSession>) -> Folders {
        Folders::new(None, PathBuf::from("/p"), names.iter().map(|n| folder(n)).collect(), sessions)
    }

    #[test]
    fn a_folders_sessions_follow_it() {
        let mut f = here(
            &["oath", "sola"],
            vec![session("ws-main", "/p/sola"), session("ws-x", "/elsewhere"), session("ws-zones", "/p/sola")],
        );
        assert_eq!(
            f.rows(),
            [Row::Folder(0), Row::Folder(1), Row::Session(1, 0), Row::Session(1, 2), Row::New, Row::Remote]
        );
        f.apply_query("oa");
        assert_eq!(f.rows(), [Row::Folder(0), Row::New, Row::Remote]);
    }

    #[test]
    fn a_hosts_list_is_its_folders_then_new() {
        let listing = Listing {
            root: PathBuf::from("/home/j/Projects"),
            names: vec!["Sola".into()],
            sessions: Vec::new(),
        };
        let f = Folders::remote("novus", listing);
        assert_eq!(f.folders[0].path, PathBuf::from("/home/j/Projects/Sola"));
        assert_eq!(f.rows(), [Row::Folder(0), Row::New]);
    }

    #[test]
    fn picking_opens_a_folder_or_a_session_alone() {
        let mut n = Navigator::new(here(&["sola"], vec![session("ws-main", "/p/sola")]), false);
        assert_eq!(
            n.pick(0),
            Pick::Open { path: "/p/sola".into(), host: None, session: None }
        );
        assert_eq!(
            n.pick(1),
            Pick::Open { path: "/p/sola".into(), host: None, session: Some("ws-main".into()) }
        );
    }

    #[test]
    fn new_names_then_creates_and_a_typed_new_name_creates_at_once() {
        let mut n = Navigator::new(here(&["sola"], Vec::new()), false);
        // New, nothing typed: ask for the name.
        assert_eq!(n.pick(1), Pick::Nothing);
        assert!(n.folders.naming);
        n.set_query("Lantern".into());
        assert_eq!(
            n.pick(0),
            Pick::Create { name: "Lantern".into(), root: "/p".into(), host: None }
        );
        // Typed in the list: New takes it.
        let mut n = Navigator::new(here(&["sola"], Vec::new()), false);
        n.set_query("Lantern".into());
        assert_eq!(n.folders.rows(), [Row::New, Row::Remote]);
        assert_eq!(
            n.pick(0),
            Pick::Create { name: "Lantern".into(), root: "/p".into(), host: None }
        );
    }

    #[test]
    fn esc_steps_back_and_at_the_top_says_close() {
        let mut n = Navigator::new(here(&["sola"], Vec::new()), true);
        assert!(n.back(), "naming → the list");
        assert!(!n.folders.naming);
        assert!(!n.back(), "the top: close the window");
    }

    #[test]
    fn steps_stay_on_the_rows() {
        let mut n = Navigator::new(here(&["a", "b"], Vec::new()), false);
        n.step(-1);
        assert_eq!(n.selected, 0);
        n.step(10);
        assert_eq!(n.selected, 3, "a, b, New, Remote");
    }
}
