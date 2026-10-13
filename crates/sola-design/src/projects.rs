//! Design's own project list: `~/.config/sola/design/projects.json`.
//!
//! Separate from the Workspaces catalog by decision (2026-10-12). A
//! project is a folder; its canvases live under `<root>/design/`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    #[serde(default)]
    pub collapsed: bool,
}

/// A canvas inside a project: the `design/<canvas>` directory name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CanvasRef {
    pub project: String,
    pub canvas: String,
}

impl CanvasRef {
    /// Rail item id. Project ids and canvas dir names never contain `/`.
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<CanvasRef>,
}

fn version() -> u32 {
    VERSION
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            version: VERSION,
            projects: Vec::new(),
            selected: None,
        }
    }
}

impl Catalog {
    pub fn project(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    /// Add the folder at `raw` (`~` expands). Returns the new project id.
    pub fn add(&mut self, raw: &str) -> Result<String, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err("Enter a folder.".into());
        }
        let path = expand_home(raw);
        let root = path
            .canonicalize()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if !root.is_dir() {
            return Err(format!("{} is not a folder.", root.display()));
        }
        if self.projects.iter().any(|p| p.root == root) {
            return Err("That folder is already in the list.".into());
        }
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        let id = self.unique_id(&format!("proj-{}", slug(&name)));
        self.projects.push(Project {
            id: id.clone(),
            name,
            root,
            collapsed: false,
        });
        Ok(id)
    }

    /// Unregister a project. Files on disk are untouched.
    pub fn drop_project(&mut self, id: &str) {
        self.projects.retain(|p| p.id != id);
        if self.selected.as_ref().is_some_and(|s| s.project == id) {
            self.selected = None;
        }
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

pub fn catalog_path() -> PathBuf {
    crate::profile::data_dir().join("projects.json")
}

/// Missing file → empty list. A corrupt file is logged and treated as empty
/// (it is not overwritten until the next real change).
pub fn load_from(path: &Path) -> Catalog {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Catalog::default(),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "read design projects");
            return Catalog::default();
        }
    };
    serde_json::from_str(&text).unwrap_or_else(|e| {
        tracing::warn!(path = %path.display(), error = %e, "parse design projects");
        Catalog::default()
    })
}

/// Write via a temp file + rename so a crash never leaves half a list.
pub fn save_to(path: &Path, catalog: &Catalog) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(catalog).map_err(|e| e.to_string())?;
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
        dir
    }

    #[test]
    fn slug_collapses_punctuation() {
        assert_eq!(slug("My App (v2)"), "my-app-v2");
        assert_eq!(slug("--x--"), "x");
        assert_eq!(slug("Ünïcode"), "n-code");
    }

    #[test]
    fn canvas_ref_key_round_trips() {
        let r = CanvasRef {
            project: "proj-sola".into(),
            canvas: "settings".into(),
        };
        assert_eq!(CanvasRef::from_key(&r.key()), Some(r));
        assert_eq!(CanvasRef::from_key("nope"), None);
    }

    #[test]
    fn add_rejects_duplicates_and_dedupes_ids() {
        let root = temp_dir("add");
        let a = root.join("Sola");
        let b = root.join("other").join("Sola");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let mut cat = Catalog::default();
        assert_eq!(cat.add(a.to_str().unwrap()).unwrap(), "proj-sola");
        assert!(cat.add(a.to_str().unwrap()).is_err());
        assert_eq!(cat.add(b.to_str().unwrap()).unwrap(), "proj-sola-2");
        assert_eq!(cat.projects[1].name, "Sola");
        assert!(cat.add(root.join("missing").to_str().unwrap()).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn save_load_round_trip_and_drop_clears_selection() {
        let root = temp_dir("save");
        let proj = root.join("Thoxa");
        std::fs::create_dir_all(&proj).unwrap();
        let mut cat = Catalog::default();
        let id = cat.add(proj.to_str().unwrap()).unwrap();
        cat.selected = Some(CanvasRef {
            project: id.clone(),
            canvas: "home".into(),
        });
        let path = root.join("projects.json");
        save_to(&path, &cat).unwrap();
        assert_eq!(load_from(&path), cat);
        cat.drop_project(&id);
        assert!(cat.projects.is_empty());
        assert!(cat.selected.is_none());
        assert_eq!(load_from(&root.join("absent.json")), Catalog::default());
        let _ = std::fs::remove_dir_all(root);
    }
}
