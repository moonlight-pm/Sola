//! The projects Design knows, and what it keeps of them.
//!
//! A project is a folder of `~/Projects` with canvases in its `design/`
//! (Joshua, 2026-11-10: "design doesn't 'create' design folders .. like
//! projects, it will scan the Projects/<...>/design folders which can be
//! opened for viewing... only agents will create those folders"). Design
//! finds them by scanning ([`scan`]) and opens a window on one; it never
//! makes, registers or drops one. A canvas an agent opens elsewhere (a
//! worktree) makes its folder a project too, while Design runs.
//!
//! `~/.config/sola/design/projects.json` keeps which projects have a
//! window (they come back at the next launch), the one last in front, and
//! each project's chip order and shown canvas ([`Saved`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::canvas::{self, Entry};

const VERSION: u32 = 2;

/// A project Design knows. Its id holds while Design runs and, for a
/// folder of `~/Projects`, across runs (`proj-<slug of its name>`): share
/// links and board hosts are keyed by it.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
}

/// What Design keeps of one project.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    /// Canvas dir names in chip order (dragged). Canvases not listed
    /// follow, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub canvas_order: Vec<String>,
    /// The canvas its window shows (a dir name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
}

/// `projects.json`, by project root.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    #[serde(default = "version")]
    pub version: u32,
    /// The projects with a window, in the order they opened.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open: Vec<PathBuf>,
    /// The project whose window was last in front.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub front: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub kept: BTreeMap<PathBuf, Kept>,
}

fn version() -> u32 {
    VERSION
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            version: VERSION,
            open: Vec::new(),
            front: None,
            kept: BTreeMap::new(),
        }
    }
}

/// A canvas inside a project: the `design/<canvas>` directory name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CanvasRef {
    pub project: String,
    pub canvas: String,
}

impl CanvasRef {
    /// Its key. Project ids and canvas dir names never contain `/`.
    pub fn key(&self) -> String {
        format!("{}/{}", self.project, self.canvas)
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let (project, canvas) = key.split_once('/')?;
        Some(Self {
            project: project.to_string(),
            canvas: canvas.to_string(),
        })
    }
}

/// The projects known while Design runs, and what is kept of them.
#[derive(Debug, Default)]
pub struct Catalog {
    /// Every project seen since Design started; none is forgotten.
    pub projects: Vec<Project>,
    /// The folders of `~/Projects` with canvases, by name (ids): what a
    /// navigator lists.
    pub listed: Vec<String>,
    pub saved: Saved,
}

impl Catalog {
    pub fn new(saved: Saved) -> Self {
        Self {
            saved,
            ..Self::default()
        }
    }

    pub fn project(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    /// The project at `root` exactly (not one above it).
    pub fn at_root(&self, root: &Path) -> Option<&Project> {
        self.projects.iter().find(|p| p.root == root)
    }

    /// The id of the project at `root` (canonical), known from now on.
    pub fn know(&mut self, root: &Path) -> String {
        if let Some(p) = self.at_root(root) {
            return p.id.clone();
        }
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        let id = self.unique_id(&format!("proj-{}", slug(&name)));
        self.projects.push(Project {
            id: id.clone(),
            name,
            root: root.to_path_buf(),
        });
        id
    }

    /// What a scan found in `~/Projects` (roots, by name) is the list.
    pub fn set_listed<'a>(&mut self, roots: impl IntoIterator<Item = &'a PathBuf>) {
        self.listed = roots.into_iter().map(|r| self.know(r)).collect();
    }

    fn kept(&self, id: &str) -> Option<&Kept> {
        self.saved.kept.get(&self.project(id)?.root)
    }

    /// The canvas project `id`'s window shows, as kept.
    pub fn selected(&self, id: &str) -> Option<&str> {
        self.kept(id)?.selected.as_deref()
    }

    /// Put `entries` (a scan, by name) in project `id`'s chip order.
    pub fn order_canvases(&self, id: &str, entries: &mut [Entry]) {
        let order = self.kept(id).map_or(&[][..], |k| k.canvas_order.as_slice());
        let rank = |name: &str| order.iter().position(|n| n == name).unwrap_or(usize::MAX);
        entries.sort_by_key(|e| rank(&e.name));
    }

    /// `at` is shown: its project's window shows it from now on, and that
    /// window opens in front next time. True when that changed.
    pub fn select(&mut self, at: &CanvasRef) -> bool {
        let Some(root) = self.project(&at.project).map(|p| p.root.clone()) else {
            return false;
        };
        let kept = self.saved.kept.entry(root.clone()).or_default();
        let changed =
            kept.selected.as_deref() != Some(at.canvas.as_str()) || self.saved.front.as_ref() != Some(&root);
        kept.selected = Some(at.canvas.clone());
        self.saved.front = Some(root);
        changed
    }

    /// Project `id` has a window (or no longer has). True when that changed.
    pub fn set_open(&mut self, id: &str, open: bool) -> bool {
        let Some(root) = self.project(id).map(|p| p.root.clone()) else {
            return false;
        };
        let at = self.saved.open.iter().position(|r| *r == root);
        match (open, at) {
            (true, None) => self.saved.open.push(root),
            (false, Some(i)) => {
                self.saved.open.remove(i);
            }
            _ => return false,
        }
        true
    }

    /// Move canvas `name` in front of `before` (`None` = last) within
    /// `project`. `listed` is the project's canvases in chip order now; it
    /// becomes the kept order (so deleted canvases fall out of it).
    pub fn move_canvas_before(
        &mut self,
        project: &str,
        name: &str,
        before: Option<&str>,
        listed: &[String],
    ) -> Result<(), String> {
        let root = self
            .project(project)
            .map(|p| p.root.clone())
            .ok_or_else(|| format!("unknown project '{project}'"))?;
        let known = |n: &str| listed.iter().any(|l| l == n);
        if !known(name) {
            return Err(format!("unknown canvas '{name}' in {project}"));
        }
        if let Some(b) = before.filter(|b| !known(b)) {
            return Err(format!("unknown canvas '{b}' in {project}"));
        }
        if before == Some(name) {
            return Ok(());
        }
        let mut order: Vec<String> = listed.iter().filter(|n| *n != name).cloned().collect();
        let at = before
            .and_then(|b| order.iter().position(|n| n == b))
            .unwrap_or(order.len());
        order.insert(at, name.to_string());
        self.saved.kept.entry(root).or_default().canvas_order = order;
        Ok(())
    }

    fn unique_id(&self, base: &str) -> String {
        let taken = |id: &str| self.projects.iter().any(|p| p.id == id);
        if !taken(base) {
            return base.to_string();
        }
        (2..)
            .map(|n| format!("{base}-{n}"))
            .find(|id| !taken(id))
            .expect("unbounded suffixes")
    }
}

/// One look at the disk: each folder of `~/Projects` with canvases (by
/// name), then each other root asked about, with its canvases (maybe none).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scan {
    pub listed: Vec<(PathBuf, Vec<Entry>)>,
    pub other: Vec<(PathBuf, Vec<Entry>)>,
}

/// Scan `dir` (`~/Projects`) and the `known` roots outside its list.
pub fn scan(dir: &Path, known: &[PathBuf]) -> Scan {
    let mut listed: Vec<(String, PathBuf, Vec<Entry>)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            if name.starts_with('.') {
                return None;
            }
            let root = e.path().canonicalize().ok()?;
            let canvases = canvas::scan(&root);
            (!canvases.is_empty()).then_some((name, root, canvases))
        })
        .collect();
    listed.sort_by_key(|(name, ..)| name.to_lowercase());
    let listed: Vec<(PathBuf, Vec<Entry>)> = listed.into_iter().map(|(_, r, c)| (r, c)).collect();
    let other = known
        .iter()
        .filter(|r| listed.iter().all(|(l, _)| l != *r))
        .map(|r| (r.clone(), canvas::scan(r)))
        .collect();
    Scan { listed, other }
}

/// `~/Projects`.
pub fn projects_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("/"), PathBuf::from)
        .join("Projects")
}

/// Where a chip dropped on `onto` in `order` goes: it takes `onto`'s
/// place. `Some(before)` (`None`: last); `None` when nothing moves.
pub fn drop_place(order: &[String], from: &str, onto: &str) -> Option<Option<String>> {
    let i = order.iter().position(|n| n == from)?;
    let j = order.iter().position(|n| n == onto)?;
    match i.cmp(&j) {
        std::cmp::Ordering::Equal => None,
        std::cmp::Ordering::Less => Some(order.get(j + 1).cloned()),
        std::cmp::Ordering::Greater => Some(Some(onto.to_string())),
    }
}

pub fn catalog_path() -> PathBuf {
    crate::profile::data_dir().join("projects.json")
}

/// The list before 2026-11-10: projects added by hand, every one with a
/// window.
#[derive(Deserialize)]
struct V1 {
    #[serde(default)]
    projects: Vec<V1Project>,
    #[serde(default)]
    selected: Option<CanvasRef>,
}

#[derive(Deserialize)]
struct V1Project {
    id: String,
    root: PathBuf,
    #[serde(default)]
    canvas_order: Vec<String>,
    #[serde(default)]
    selected: Option<String>,
}

impl From<V1> for Saved {
    /// Each project kept its window; nothing else of the list stays.
    fn from(v1: V1) -> Self {
        let front = v1
            .selected
            .and_then(|s| v1.projects.iter().find(|p| p.id == s.project))
            .map(|p| p.root.clone());
        let mut saved = Saved {
            front,
            ..Saved::default()
        };
        for p in v1.projects {
            saved.open.push(p.root.clone());
            let kept = Kept {
                canvas_order: p.canvas_order,
                selected: p.selected,
            };
            if kept != Kept::default() {
                saved.kept.insert(p.root, kept);
            }
        }
        saved
    }
}

/// Missing file → nothing kept. A corrupt file is logged and treated as
/// empty (it is not overwritten until the next real change).
pub fn load_from(path: &Path) -> Saved {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Saved::default(),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "read design projects");
            return Saved::default();
        }
    };
    parse(&text).unwrap_or_else(|e| {
        tracing::warn!(path = %path.display(), error = %e, "parse design projects");
        Saved::default()
    })
}

fn parse(text: &str) -> Result<Saved, serde_json::Error> {
    let value: serde_json::Value = serde_json::from_str(text)?;
    if value.get("version").and_then(serde_json::Value::as_u64) == Some(1) {
        return serde_json::from_value::<V1>(value).map(Saved::from);
    }
    serde_json::from_value(value)
}

/// Write via a temp file + rename so a crash never leaves half a list.
pub fn save_to(path: &Path, saved: &Saved) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(saved).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))
}

pub fn expand_home(raw: &str) -> PathBuf {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    if raw == "~" {
        return home().unwrap_or_else(|| PathBuf::from(raw));
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(h) = home() {
            return h.join(rest);
        }
    }
    PathBuf::from(raw)
}

/// Lowercase ASCII letters / digits joined by single `-`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sola-design-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn canvas_in(root: &Path, name: &str) {
        let dir = canvas::canvas_dir(root, name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(canvas::CANVAS_FILE), "").unwrap();
    }

    fn at(p: &str, c: &str) -> CanvasRef {
        CanvasRef {
            project: p.into(),
            canvas: c.into(),
        }
    }

    #[test]
    fn slug_collapses_punctuation() {
        assert_eq!(slug("My App (v2)"), "my-app-v2");
        assert_eq!(slug("--x--"), "x");
        assert_eq!(slug("Ünïcode"), "n-code");
    }

    #[test]
    fn canvas_ref_key_round_trips() {
        let r = at("proj-sola", "settings");
        assert_eq!(CanvasRef::from_key(&r.key()), Some(r));
        assert_eq!(CanvasRef::from_key("nope"), None);
    }

    #[test]
    fn a_scan_lists_the_folders_with_canvases_by_name_then_the_others() {
        let dir = temp_dir("scan");
        for name in ["wow", "Sola", "Hub", ".hidden"] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
        }
        canvas_in(&dir.join("wow"), "halo");
        canvas_in(&dir.join("Sola"), "foundation");
        canvas_in(&dir.join(".hidden"), "x");
        // A design/ folder with no canvas.toml is not a design.
        std::fs::create_dir_all(dir.join("Hub/design/notes")).unwrap();
        let tree = temp_dir("scan-tree");
        canvas_in(&tree, "draft");
        let known = [dir.join("Sola"), tree.clone(), dir.join("Hub")];
        let found = scan(&dir, &known);
        let roots: Vec<&PathBuf> = found.listed.iter().map(|(r, _)| r).collect();
        assert_eq!(roots, [&dir.join("Sola"), &dir.join("wow")]);
        assert_eq!(found.listed[0].1[0].name, "foundation");
        let other: Vec<(&PathBuf, usize)> = found.other.iter().map(|(r, c)| (r, c.len())).collect();
        assert_eq!(other, [(&tree, 1), (&dir.join("Hub"), 0)]);
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(tree);
    }

    #[test]
    fn ids_come_from_the_folder_name_and_stay() {
        let mut cat = Catalog::default();
        let sola = PathBuf::from("/home/j/Projects/Sola");
        let tree = PathBuf::from("/home/j/Projects/Sola/.worktrees/sola");
        cat.set_listed([&sola]);
        assert_eq!(cat.listed, ["proj-sola"]);
        assert_eq!(cat.know(&tree), "proj-sola-2");
        // Gone from the list, still known by the same id.
        cat.set_listed([]);
        assert!(cat.listed.is_empty());
        assert_eq!(cat.know(&sola), "proj-sola");
        assert_eq!(cat.project("proj-sola-2").unwrap().name, "sola");
    }

    #[test]
    fn select_and_open_are_kept_by_root_and_saved() {
        let dir = temp_dir("save");
        let mut cat = Catalog::default();
        let a = cat.know(Path::new("/p/a"));
        let b = cat.know(Path::new("/p/b"));
        assert!(cat.select(&at(&a, "home")));
        assert!(!cat.select(&at(&a, "home")), "nothing changed");
        assert!(cat.select(&at(&b, "settings")));
        assert_eq!(cat.selected(&a), Some("home"));
        assert_eq!(cat.saved.front, Some(PathBuf::from("/p/b")));
        assert!(cat.set_open(&b, true) && cat.set_open(&a, true));
        assert!(!cat.set_open(&a, true));
        assert!(cat.set_open(&b, false));
        assert_eq!(cat.saved.open, [PathBuf::from("/p/a")]);
        let path = dir.join("projects.json");
        save_to(&path, &cat.saved).unwrap();
        assert_eq!(load_from(&path), cat.saved);
        assert_eq!(load_from(&dir.join("absent.json")), Saved::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_hand_made_list_becomes_open_windows() {
        let v1 = r#"{"version":1,"projects":[
            {"id":"proj-sola","name":"Sola","root":"/p/Sola","selected":"foundation"},
            {"id":"proj-exit","name":"Exit","root":"/p/Exit","canvas_order":["b","a"],"collapsed":true}
        ],"selected":{"project":"proj-exit","canvas":"a"}}"#;
        let saved = parse(v1).unwrap();
        assert_eq!(saved.version, VERSION);
        assert_eq!(saved.open, [PathBuf::from("/p/Sola"), PathBuf::from("/p/Exit")]);
        assert_eq!(saved.front, Some(PathBuf::from("/p/Exit")));
        assert_eq!(saved.kept[Path::new("/p/Sola")].selected.as_deref(), Some("foundation"));
        assert_eq!(saved.kept[Path::new("/p/Exit")].canvas_order, ["b", "a"]);
    }

    #[test]
    fn a_dropped_chip_takes_the_place_it_lands_on() {
        let order: Vec<String> = ["a", "b", "c"].map(String::from).into();
        assert_eq!(drop_place(&order, "a", "b"), Some(Some("c".into())));
        assert_eq!(drop_place(&order, "a", "c"), Some(None));
        assert_eq!(drop_place(&order, "c", "a"), Some(Some("a".into())));
        assert_eq!(drop_place(&order, "b", "b"), None);
        assert_eq!(drop_place(&order, "z", "b"), None);
    }

    #[test]
    fn canvas_order_saves_and_sorts_new_canvases_last() {
        let entry = |n: &str| Entry { name: n.into() };
        let listed: Vec<String> = ["home", "onboarding", "settings"].map(String::from).into();
        let mut cat = Catalog::default();
        let a = cat.know(Path::new("/p/a"));
        let moved = cat.move_canvas_before(&a, "settings", Some("home"), &listed);
        assert_eq!(moved, Ok(()));
        assert_eq!(cat.saved.kept[Path::new("/p/a")].canvas_order, ["settings", "home", "onboarding"]);
        assert!(cat.move_canvas_before(&a, "gone", None, &listed).is_err());
        let unknown_before = cat.move_canvas_before(&a, "home", Some("gone"), &listed);
        assert!(unknown_before.is_err());

        // A scan (by name) with a new canvas and one deleted.
        let mut scan: Vec<Entry> = ["about", "home", "settings", "zebra"].map(entry).into();
        cat.order_canvases(&a, &mut scan);
        let names: Vec<&str> = scan.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["settings", "home", "about", "zebra"]);
    }
}
