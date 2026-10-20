//! `<project>/design/<canvas>/canvas.toml` — the layout of one canvas.
//!
//! The artboards themselves are plain HTML files next to it. This module
//! only reads and validates the layout; it never edits a design.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::annot::{self, Annotations};

pub const DESIGN_DIR: &str = "design";
pub const CANVAS_FILE: &str = "canvas.toml";

/// Largest artboard side in CSS px. Past 8192 a board rasters under 1×
/// (its texture would not fit) and stays sharp through detail renders.
pub const MAX_SIDE: u32 = 32768;
/// Viewport height an `auto` artboard starts at, before it is measured.
pub const AUTO_START_HEIGHT: u32 = 900;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CanvasFile {
    title: Option<String>,
    /// Web root inside the project (project-relative): root-absolute URLs
    /// look here first, then at the project root.
    root: Option<String>,
    #[serde(default, rename = "artboard")]
    artboards: Vec<ArtboardSpec>,
    #[serde(default, rename = "section")]
    sections: Vec<annot::SectionSpec>,
    #[serde(default, rename = "note")]
    notes: Vec<annot::NoteSpec>,
    #[serde(default, rename = "link")]
    links: Vec<annot::LinkSpec>,
    #[serde(default, rename = "flow")]
    flows: Vec<annot::FlowSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtboardSpec {
    id: Option<String>,
    src: String,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    width: u32,
    height: HeightSpec,
    title: Option<String>,
}

/// `height = 900` or `height = "auto"` (fit the page's content).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum HeightSpec {
    Px(u32),
    Word(String),
}

/// One validated artboard: where it sits on the plane and what it loads.
#[derive(Debug, Clone, PartialEq)]
pub struct Artboard {
    pub id: String,
    pub title: String,
    /// `file://` for a path, or the http(s) URL as written.
    pub url: String,
    pub x: f32,
    pub y: f32,
    pub width: u32,
    /// Viewport height. For `auto` artboards: the last measured content
    /// height ([`AUTO_START_HEIGHT`] until the page has been measured).
    pub height: u32,
    /// `height = "auto"`: the viewport grows to the page's content.
    pub auto_height: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Canvas {
    pub title: String,
    pub artboards: Vec<Artboard>,
    /// `root` resolved: an existing directory inside the project.
    pub web_root: Option<PathBuf>,
    /// Sections, notes, board links, and flows.
    pub annot: Annotations,
}

/// A canvas as the rail lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Directory name under `design/` (the canvas id).
    pub name: String,
    pub title: String,
}

pub fn design_dir(root: &Path) -> PathBuf {
    root.join(DESIGN_DIR)
}

pub fn canvas_dir(root: &Path, name: &str) -> PathBuf {
    design_dir(root).join(name)
}

pub fn load(dir: &Path) -> Result<Canvas, String> {
    let path = dir.join(CANVAS_FILE);
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(dir, &text)
}

pub fn parse(dir: &Path, text: &str) -> Result<Canvas, String> {
    let file: CanvasFile = toml::from_str(text).map_err(|e| format!("{CANVAS_FILE}: {e}"))?;
    let mut seen = HashSet::new();
    let mut artboards = Vec::with_capacity(file.artboards.len());
    for (i, spec) in file.artboards.into_iter().enumerate() {
        let n = i + 1;
        let id = match spec.id {
            Some(id) if !id.trim().is_empty() => id.trim().to_string(),
            _ if is_web_url(&spec.src) => {
                return Err(format!("artboard {n}: a URL artboard needs an id"));
            }
            _ => file_stem(&spec.src).ok_or_else(|| format!("artboard {n}: empty src"))?,
        };
        if !seen.insert(id.clone()) {
            return Err(format!("artboard {n}: duplicate id {id:?}"));
        }
        let (height, auto_height) = match spec.height {
            HeightSpec::Px(h) => (h, false),
            HeightSpec::Word(w) if w == "auto" => (AUTO_START_HEIGHT, true),
            HeightSpec::Word(w) => {
                return Err(format!("artboard {id:?}: height must be a number or \"auto\", not {w:?}"));
            }
        };
        for (side, v) in [("width", spec.width), ("height", height)] {
            if v == 0 || v > MAX_SIDE {
                return Err(format!("artboard {id:?}: {side} must be 1..={MAX_SIDE}"));
            }
        }
        if !spec.x.is_finite() || !spec.y.is_finite() {
            return Err(format!("artboard {id:?}: x / y must be numbers"));
        }
        let url = resolve_src(dir, &spec.src);
        artboards.push(Artboard {
            title: spec.title.unwrap_or_else(|| id.clone()),
            id,
            url,
            x: spec.x,
            y: spec.y,
            width: spec.width,
            height,
            auto_height,
        });
    }
    let annot = annot::check(file.sections, file.notes, file.links, file.flows, &seen)?;
    let title = file.title.unwrap_or_else(|| dir_name(dir));
    let web_root = file.root.map(|r| web_root(dir, &r)).transpose()?;
    Ok(Canvas {
        title,
        artboards,
        web_root,
        annot,
    })
}

/// `root = "…"`: a directory inside the project (the canvas dir is
/// `<project>/design/<canvas>`), no `..`, no dotfile segments.
fn web_root(dir: &Path, root: &str) -> Result<PathBuf, String> {
    let project = dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "root: the canvas is not under <project>/design/".to_string())?;
    let rel = Path::new(root.trim().trim_start_matches("./"));
    let mut path = project.to_path_buf();
    for part in rel.components() {
        match part {
            std::path::Component::Normal(seg) if !seg.to_string_lossy().starts_with('.') => {
                path.push(seg)
            }
            _ => {
                return Err(format!(
                    "root: {root:?} must be a project-relative path (no .., no dotfiles)"
                ));
            }
        }
    }
    if !path.is_dir() {
        return Err(format!("root: {} is not a directory", path.display()));
    }
    Ok(path)
}

/// Canvases under `<root>/design/`, sorted by directory name. A directory
/// counts when it holds a `canvas.toml`; an unreadable one still lists
/// (the error shows when it is opened).
pub fn scan(root: &Path) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(design_dir(root)) else {
        return Vec::new();
    };
    let mut out: Vec<Entry> = rd
        .filter_map(Result::ok)
        .filter(|e| e.path().join(CANVAS_FILE).is_file())
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            if name.starts_with('.') {
                return None;
            }
            let title = load(&e.path())
                .map(|c| c.title)
                .unwrap_or_else(|_| name.clone());
            Some(Entry { name, title })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The canvas holding `path`: the nearest ancestor `<root>/design/<name>/`
/// with a `canvas.toml`. Returns `(root, name)`. `path` may be the canvas
/// dir, its toml, or any file inside it.
pub fn locate(path: &Path) -> Option<(PathBuf, String)> {
    path.ancestors().find_map(|dir| {
        let design = dir.parent()?;
        if design.file_name()? != DESIGN_DIR || !dir.join(CANVAS_FILE).is_file() {
            return None;
        }
        let name = dir.file_name()?.to_str()?.to_string();
        Some((design.parent()?.to_path_buf(), name))
    })
}

/// New empty canvas `design/<slug>/canvas.toml`. Returns the dir name.
pub fn create(root: &Path, title: &str) -> Result<String, String> {
    let title = title.trim();
    let name = crate::projects::slug(title);
    if name.is_empty() {
        return Err("Use letters or digits in the name.".into());
    }
    let dir = canvas_dir(root, &name);
    if dir.exists() {
        return Err(format!("design/{name} already exists."));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let body = format!("{}title = {}\n", TEMPLATE_HEADER, toml_string(title));
    std::fs::write(dir.join(CANVAS_FILE), body)
        .map_err(|e| format!("write {CANVAS_FILE}: {e}"))?;
    Ok(name)
}

const TEMPLATE_HEADER: &str = "\
# sola-design canvas. Artboards are HTML files in this directory.
#
# root = \"web/public\"     # optional: root-absolute URLs (/fonts/x) look in
#                         # this project folder first, then the project root
#
# [[artboard]]
# id = \"desktop\"          # unique; defaults to the src file stem
# src = \"desktop.html\"    # relative path, or an http(s) URL
# x = 0                   # canvas position (CSS px at 100%)
# y = 0
# width = 1440            # the artboard's viewport (CSS px)
# height = 900            # or \"auto\": grow to the page's content
# title = \"Desktop\"       # optional label
#
# [[section]]             # a titled region around some artboards
# title = \"Sign in\"
# boards = [\"desktop\"]
#
# [[note]]                # an annotation: board (+ at = \"css selector\"), or x / y
# board = \"desktop\"
# text = \"What is not obvious here.\"
#
# [[link]]                # an arrow between whole artboards; in a page,
# from = \"desktop\"        # data-design-to=\"<id>\" on an element is an exit
# to = \"phone\"
#
# [[flow]]                # a tour that follows exits and links
# title = \"Sign in\"
# start = \"desktop\"

";

fn toml_string(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

fn is_web_url(src: &str) -> bool {
    src.starts_with("http://") || src.starts_with("https://")
}

/// A path `src` becomes a `file://` URL; its `?query` / `#fragment` stay
/// as written, so one page can show two states (`light.html?theme=dark`).
fn resolve_src(dir: &Path, src: &str) -> String {
    if is_web_url(src) || src.starts_with("file://") {
        return src.to_string();
    }
    let (path, suffix) = split_suffix(src);
    format!("{}{suffix}", file_url(&dir.join(path)))
}

/// `page.html?x=1#y` → (`page.html`, `?x=1#y`).
fn split_suffix(src: &str) -> (&str, &str) {
    src.split_at(src.find(['?', '#']).unwrap_or(src.len()))
}

fn file_stem(src: &str) -> Option<String> {
    let stem = Path::new(split_suffix(src).0).file_stem()?.to_str()?.trim();
    (!stem.is_empty()).then(|| stem.to_string())
}

fn dir_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Path of a `file://` URL (query / fragment dropped, `%XX` decoded).
pub fn file_url_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let (path, _) = split_suffix(rest);
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            (b, ..) => {
                out.push(b);
                i += 1;
            }
        }
    }
    let path = PathBuf::from(String::from_utf8(out).ok()?);
    path.is_absolute().then_some(path)
}

/// `file://` URL with each path byte outside the unreserved set escaped.
pub fn file_url(path: &Path) -> String {
    let mut out = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        let keep = b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~');
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = "/w/proj/design/home";

    #[test]
    fn parses_defaults_and_resolves_paths() {
        let c = parse(
            Path::new(DIR),
            r#"
            title = "Home"
            [[artboard]]
            src = "desktop.html"
            width = 1440
            height = 900

            [[artboard]]
            id = "dev"
            src = "http://localhost:5173/"
            x = 1600
            y = -20.5
            width = 390
            height = 844
            title = "Live"
            "#,
        )
        .unwrap();
        assert_eq!(c.title, "Home");
        assert_eq!(c.artboards[0].id, "desktop");
        assert_eq!(c.artboards[0].title, "desktop");
        assert_eq!(c.artboards[0].url, "file:///w/proj/design/home/desktop.html");
        assert_eq!((c.artboards[0].x, c.artboards[0].y), (0.0, 0.0));
        assert_eq!(c.artboards[1].url, "http://localhost:5173/");
        assert_eq!(c.artboards[1].title, "Live");
        assert_eq!(c.artboards[1].y, -20.5);
    }

    #[test]
    fn web_root_stays_in_the_project() {
        let project = std::env::temp_dir().join(format!("sola-design-root-{}", std::process::id()));
        let dir = project.join("design/home");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(project.join("web/public")).unwrap();
        let c = parse(&dir, "root = \"web/public\"").unwrap();
        assert_eq!(c.web_root, Some(project.join("web/public")));
        assert_eq!(parse(&dir, "").unwrap().web_root, None);
        for bad in ["../x", "/etc", ".git", "web/missing"] {
            let err = parse(&dir, &format!("root = {bad:?}")).unwrap_err();
            assert!(err.starts_with("root:"), "{bad}: {err}");
        }
        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn title_defaults_to_dir_and_empty_is_ok() {
        let c = parse(Path::new(DIR), "").unwrap();
        assert_eq!(c.title, "home");
        assert!(c.artboards.is_empty());
    }

    #[test]
    fn auto_height_starts_at_the_default() {
        let c = parse(
            Path::new(DIR),
            "[[artboard]]\nsrc='long.html'\nwidth=1440\nheight='auto'\n",
        )
        .unwrap();
        assert!(c.artboards[0].auto_height);
        assert_eq!(c.artboards[0].height, AUTO_START_HEIGHT);
        let bad = "[[artboard]]\nsrc='a.html'\nwidth=10\nheight='tall'\n";
        assert!(parse(Path::new(DIR), bad).unwrap_err().contains("auto"));
    }

    #[test]
    fn rejects_bad_layouts() {
        let dup = "[[artboard]]\nsrc='a.html'\nwidth=1\nheight=1\n[[artboard]]\nsrc='sub/a.html'\nwidth=1\nheight=1\n";
        assert!(parse(Path::new(DIR), dup).unwrap_err().contains("duplicate"));
        let zero = "[[artboard]]\nsrc='a.html'\nwidth=0\nheight=10\n";
        assert!(parse(Path::new(DIR), zero).unwrap_err().contains("width"));
        let url = "[[artboard]]\nsrc='https://x.dev'\nwidth=10\nheight=10\n";
        assert!(parse(Path::new(DIR), url).unwrap_err().contains("needs an id"));
        let typo = "[[artboard]]\nsrc='a.html'\nwidht=10\nheight=10\n";
        assert!(parse(Path::new(DIR), typo).is_err());
    }

    #[test]
    fn src_keeps_query_and_fragment() {
        let c = parse(
            Path::new(DIR),
            "[[artboard]]\nsrc='sheet page.html?theme=dark#top'\nwidth=10\nheight=10\n",
        )
        .unwrap();
        assert_eq!(c.artboards[0].id, "sheet page");
        assert_eq!(
            c.artboards[0].url,
            "file:///w/proj/design/home/sheet%20page.html?theme=dark#top"
        );
    }

    #[test]
    fn file_urls_round_trip_to_paths() {
        let p = Path::new("/a b/c#d.html");
        assert_eq!(file_url_path(&file_url(p)).as_deref(), Some(p));
        assert_eq!(
            file_url_path("file:///w/x%20y.css?v=2#top").as_deref(),
            Some(Path::new("/w/x y.css"))
        );
        assert_eq!(file_url_path("https://x.dev/a.css"), None);
    }

    #[test]
    fn file_url_escapes_spaces() {
        assert_eq!(
            file_url(Path::new("/a b/c#d.html")),
            "file:///a%20b/c%23d.html"
        );
    }

    #[test]
    fn smoke_fixture_loads() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixture");
        let listed = scan(&root);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "Demo");
        let c = load(&canvas_dir(&root, "demo")).unwrap();
        assert_eq!(c.artboards.len(), 3);
        assert!(c.artboards[1].url.ends_with("/fixture/design/demo/settings.html"));
        assert!(c.artboards[2].auto_height, "landing is the auto-height smoke");
    }

    #[test]
    fn locate_climbs_to_the_canvas() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixture");
        let dir = canvas_dir(&root, "demo");
        let want = Some((root.clone(), "demo".to_string()));
        assert_eq!(locate(&dir), want);
        assert_eq!(locate(&dir.join(CANVAS_FILE)), want);
        assert_eq!(locate(&dir.join("settings.html")), want);
        assert_eq!(locate(&design_dir(&root)), None);
        assert_eq!(locate(&root), None);
    }

    #[test]
    fn create_scaffolds_and_scan_lists() {
        let root = std::env::temp_dir().join(format!(
            "sola-design-canvas-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(create(&root, "Settings Redesign").unwrap(), "settings-redesign");
        assert!(create(&root, "settings redesign").is_err());
        assert!(create(&root, "!!").is_err());
        std::fs::create_dir_all(design_dir(&root).join("no-toml")).unwrap();
        let listed = scan(&root);
        assert_eq!(
            listed,
            vec![Entry {
                name: "settings-redesign".into(),
                title: "Settings Redesign".into(),
            }]
        );
        let c = load(&canvas_dir(&root, "settings-redesign")).unwrap();
        assert!(c.artboards.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
