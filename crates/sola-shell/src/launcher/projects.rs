//! The Projects chooser: the launcher's second stage when Projects is
//! picked (Joshua, 2026-10-26: "The projects app should have a kind of
//! intersticial that exists after the launcher but before the program").
//!
//! Every project lives in `~/Projects`. The chooser lists its folders and
//! marks the ones Projects has a window for (`project.list`). Picking an
//! open one brings up its window and workspace; a closed one opens in the
//! showing workspace (`project.open`, which adds a folder that is not a
//! project yet). Then a row makes a new one: a name, a folder under
//! `~/Projects`, then opened the same way. The last row opens a project on
//! another computer ([`super::remote`]): the chooser then lists that
//! computer's `~/Projects` (its `host`), and New makes one there.
//!
//! Under each folder, its sessions running on that computer
//! ([`sola_core::project_sessions`]), whichever computer started them:
//! picking one opens it alone (`project.open --session`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sola_core::project_sessions::HostSession;

/// Projects' app id, and its call-plane owner.
pub const APP_ID: &str = "sola-projects";
pub const CALL_OWNER: &str = "projects";

/// A folder of `~/Projects`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    /// Canonical, to match Projects' roots.
    pub path: PathBuf,
}

/// A row of the chooser, in the order shown.
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

/// What Projects says of a project it knows (`project.list`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    pub id: String,
    pub open: bool,
}

#[derive(Debug, Default)]
pub struct Chooser {
    /// The computer these folders are on (an ssh host); `None`: this one.
    pub host: Option<String>,
    /// Its `~/Projects` there (for a host).
    pub root: Option<PathBuf>,
    pub folders: Vec<Folder>,
    /// The sessions running on [`Self::host`] (each under its folder).
    pub sessions: Vec<HostSession>,
    /// Projects' projects on [`Self::host`] by canonical root (empty while
    /// Projects is not running, or until it answers).
    pub known: HashMap<PathBuf, Known>,
    /// `folders` passing the query, in order.
    pub filtered: Vec<usize>,
    /// The query, trimmed.
    query: String,
    /// Asking for the new project's name (the field is the name).
    pub naming: bool,
}

/// `~/Projects`.
pub fn projects_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
        .join("Projects")
}

/// The folders of `dir`, by name (case aside), without hidden ones.
pub fn scan(dir: &Path) -> Vec<Folder> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Folder> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            if name.starts_with('.') {
                return None;
            }
            let path = e.path().canonicalize().ok()?;
            path.is_dir().then_some(Folder { name, path })
        })
        .collect();
    out.sort_by_key(|f| f.name.to_lowercase());
    out
}

impl Chooser {
    pub fn new(folders: Vec<Folder>) -> Self {
        let mut c = Chooser {
            folders,
            ..Chooser::default()
        };
        c.apply_query("");
        c
    }

    /// `host`'s `~/Projects` (at `root` there), folder `names`.
    pub fn remote(host: &str, root: &Path, names: Vec<String>) -> Self {
        let folders = names
            .into_iter()
            .map(|name| Folder {
                path: root.join(&name),
                name,
            })
            .collect();
        Chooser {
            host: Some(host.to_string()),
            root: Some(root.to_path_buf()),
            ..Chooser::new(folders)
        }
    }

    /// With the sessions running on its computer.
    pub fn with_sessions(mut self, sessions: Vec<HostSession>) -> Self {
        self.sessions = sessions;
        self
    }

    /// The sessions running in folder `f`, in listing order.
    pub fn sessions_of(&self, f: usize) -> Vec<usize> {
        let folder = &self.folders[f].path;
        (0..self.sessions.len())
            .filter(|&s| self.sessions[s].project.as_deref() == Some(folder.as_path()))
            .collect()
    }

    /// The host's `~/Projects` there, for a host's chooser.
    pub fn folders_root(&self) -> Option<PathBuf> {
        self.root.clone()
    }

    /// Keep the folders whose name holds `query` (case aside).
    pub fn apply_query(&mut self, query: &str) {
        self.query = query.trim().to_string();
        let q = self.query.to_lowercase();
        self.filtered = (0..self.folders.len())
            .filter(|&i| q.is_empty() || self.folders[i].name.to_lowercase().contains(&q))
            .collect();
    }

    /// The rows: the matching folders, then New, then Remote (here only).
    /// Naming: New alone.
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

    /// What Projects knows of `folder`, if anything.
    pub fn known(&self, folder: &Folder) -> Option<&Known> {
        self.known.get(&folder.path)
    }

    /// The name New would make: the query, unless it is already a folder.
    pub fn new_name(&self) -> Option<&str> {
        let q = self.query.as_str();
        let taken = self.folders.iter().any(|f| f.name == q);
        (!q.is_empty() && !taken).then_some(q)
    }
}

/// A new project's folder name, or why not.
pub fn check_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name the project".into());
    }
    if name.starts_with('.') || name.contains('/') || name.contains('\0') {
        return Err(format!("“{name}” can't be a folder name"));
    }
    Ok(name)
}

/// Make `dir/name` (and `dir`); an existing folder is used as it is.
pub fn create(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let name = check_name(name)?;
    let path = dir.join(name);
    if path.is_dir() {
        return Ok(path);
    }
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

    #[test]
    fn a_folders_sessions_follow_it() {
        let session = |id: &str, project: &str| HostSession {
            id: id.into(),
            project: Some(PathBuf::from(project)),
            name: None,
            title: None,
            kind: None,
        };
        let mut c = Chooser::new(vec![folder("oath"), folder("sola")]).with_sessions(vec![
            session("ws-main", "/p/sola"),
            session("ws-x", "/elsewhere"),
            session("ws-zones", "/p/sola"),
        ]);
        assert_eq!(
            c.rows(),
            vec![
                Row::Folder(0),
                Row::Folder(1),
                Row::Session(1, 0),
                Row::Session(1, 2),
                Row::New,
                Row::Remote
            ]
        );
        c.apply_query("oa");
        assert_eq!(c.rows(), vec![Row::Folder(0), Row::New, Row::Remote]);
    }

    #[test]
    fn rows_are_the_matching_folders_then_new_and_remote() {
        let all = [Row::Folder(0), Row::Folder(1), Row::Folder(2), Row::New, Row::Remote];
        let mut c = Chooser::new(vec![folder("Oath"), folder("Sola"), folder("solar")]);
        assert_eq!(c.rows(), all);
        c.apply_query("SOL");
        assert_eq!(c.rows(), [Row::Folder(1), Row::Folder(2), Row::New, Row::Remote]);
        c.naming = true;
        assert_eq!(c.rows(), [Row::New]);
    }

    #[test]
    fn a_remote_chooser_is_its_folders_then_new() {
        let c = Chooser::remote("novus", Path::new("/home/j/Projects"), vec!["Sola".into()]);
        assert_eq!(c.host.as_deref(), Some("novus"));
        assert_eq!(c.folders[0].path, PathBuf::from("/home/j/Projects/Sola"));
        assert_eq!(c.rows(), [Row::Folder(0), Row::New]);
    }

    #[test]
    fn new_takes_the_query_unless_it_is_a_folder() {
        let mut c = Chooser::new(vec![folder("Sola")]);
        assert_eq!(c.new_name(), None);
        c.apply_query("  Lantern ");
        assert_eq!(c.new_name(), Some("Lantern"));
        c.apply_query("Sola");
        assert_eq!(c.new_name(), None);
    }

    #[test]
    fn names_that_are_not_one_folder_are_refused() {
        assert_eq!(check_name(" Lantern "), Ok("Lantern"));
        assert!(check_name("").is_err());
        assert!(check_name(".hidden").is_err());
        assert!(check_name("a/b").is_err());
    }

    #[test]
    fn scan_lists_folders_by_name_and_create_makes_one() {
        let dir = std::env::temp_dir().join(format!("sola-chooser-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("beta")).unwrap();
        std::fs::create_dir_all(dir.join("Alpha")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();
        let names: Vec<String> = scan(&dir).into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["Alpha", "beta"]);
        let made = create(&dir, "Gamma").unwrap();
        assert!(made.is_dir());
        // Again: the folder is there, so it is used.
        assert_eq!(create(&dir, "Gamma").unwrap(), made);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
