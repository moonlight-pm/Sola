//! The picker's view of the filesystem: listings, places, the crumb trail.

use std::fs;
use std::path::{Path, PathBuf};

/// One row of a listing.
#[derive(Debug, Clone)]
pub(super) struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

/// Extension filter. Directories always stay visible.
#[derive(Debug, Clone)]
pub struct Filter {
    pub label: String,
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct Place {
    pub label: String,
    pub path: PathBuf,
}

/// What a listing shows.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Show<'a> {
    pub filter: Option<&'a Filter>,
    /// Dot files too (⌘⇧.).
    pub hidden: bool,
    /// Folders only (choosing a folder).
    pub dirs_only: bool,
}

/// Folders first, then files, each A–Z without regard to case.
pub(super) fn list_dir(dir: &Path, show: Show<'_>) -> Result<Vec<Entry>, String> {
    let rd = fs::read_dir(dir).map_err(|e| format!("Can't open {} — {e}", dir.display()))?;
    let mut entries = Vec::new();
    for ent in rd.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') && !show.hidden {
            continue;
        }
        let path = ent.path();
        // `file_type` is usually free (d_type); avoid a second stat per row.
        let is_dir = ent
            .file_type()
            .map(|t| t.is_dir())
            .unwrap_or_else(|_| path.is_dir());
        if !is_dir && (show.dirs_only || !matches_filter(&name, show.filter)) {
            continue;
        }
        let size = if is_dir {
            None
        } else {
            ent.metadata().ok().map(|m| m.len())
        };
        entries.push(Entry {
            path,
            name: name.into_owned(),
            is_dir,
            size,
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

pub(super) fn matches_filter(name: &str, filter: Option<&Filter>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    if filter.extensions.is_empty() {
        return true;
    }
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    filter
        .extensions
        .iter()
        .any(|want| ext.eq_ignore_ascii_case(want))
}

pub(super) fn resolve_typed(cwd: &Path, typed: &str) -> PathBuf {
    let typed = match typed.strip_prefix("~/") {
        Some(rest) => match std::env::var_os("HOME") {
            Some(home) => return PathBuf::from(home).join(rest),
            None => typed,
        },
        None => typed,
    };
    let path = PathBuf::from(typed);
    if path.is_absolute() {
        path
    } else {
        cwd.join(typed)
    }
}

/// `untitled folder`, or `untitled folder 2`, … — the first name free in
/// `dir`.
pub(super) fn free_folder_name(dir: &Path) -> String {
    const BASE: &str = "untitled folder";
    if !dir.join(BASE).exists() {
        return BASE.into();
    }
    (2..)
        .map(|n| format!("{BASE} {n}"))
        .find(|name| !dir.join(name).exists())
        .unwrap_or_else(|| BASE.into())
}

pub(super) fn default_places(home: Option<&Path>) -> Vec<Place> {
    let mut places = Vec::new();
    if let Some(home) = home {
        if home.is_dir() {
            places.push(Place {
                label: "Home".into(),
                path: home.to_path_buf(),
            });
        }
        for name in ["Desktop", "Documents", "Downloads", "Pictures", "Projects"] {
            let p = home.join(name);
            if p.is_dir() {
                places.push(Place {
                    label: name.into(),
                    path: p,
                });
            }
        }
    }
    let shots = PathBuf::from("/tmp/sola/screenshots");
    if shots.is_dir() {
        places.push(Place {
            label: "Screenshots".into(),
            path: shots,
        });
    }
    places.push(Place {
        label: "Disk".into(),
        path: PathBuf::from("/"),
    });
    places
}

/// Longest matching place prefix (skip bare `/` unless that's the only hit).
pub(super) fn closest_place(places: &[Place], cwd: &Path) -> Option<usize> {
    places
        .iter()
        .enumerate()
        .filter(|(_, p)| cwd.starts_with(&p.path))
        .max_by_key(|(_, p)| p.path.as_os_str().len())
        .map(|(i, _)| i)
}

pub(crate) fn breadcrumbs(cwd: &Path, home: Option<&Path>) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    if let Some(home) = home {
        if cwd == home {
            return vec![("Home".into(), home.to_path_buf())];
        }
        if cwd.starts_with(home) {
            out.push(("Home".into(), home.to_path_buf()));
            if let Ok(rel) = cwd.strip_prefix(home) {
                let mut acc = home.to_path_buf();
                for comp in rel.components() {
                    acc.push(comp);
                    out.push((comp.as_os_str().to_string_lossy().into_owned(), acc.clone()));
                }
            }
            return out;
        }
    }
    out.push(("/".into(), PathBuf::from("/")));
    if cwd == Path::new("/") {
        return out;
    }
    let mut acc = PathBuf::from("/");
    for comp in cwd.components().skip(1) {
        acc.push(comp);
        out.push((comp.as_os_str().to_string_lossy().into_owned(), acc.clone()));
    }
    out
}

const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "tga",
];

pub(super) fn is_image_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| IMAGE_EXTS.iter().any(|want| ext.eq_ignore_ascii_case(want)))
}

pub(crate) fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let n = bytes as f64;
    if n < KB {
        format!("{bytes} B")
    } else if n < MB {
        format!("{:.0} KB", n / KB)
    } else if n < GB {
        format!("{:.1} MB", n / MB)
    } else {
        format!("{:.1} GB", n / GB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kit-picker-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn crumbs_under_home() {
        let home = Path::new("/home/ada");
        let crumbs = breadcrumbs(&home.join("Pictures/shots"), Some(home));
        assert_eq!(crumbs.len(), 3);
        assert_eq!(crumbs[0].0, "Home");
        assert_eq!(crumbs[1].0, "Pictures");
        assert_eq!(crumbs[2].0, "shots");
        assert_eq!(crumbs[2].1, home.join("Pictures/shots"));
    }

    #[test]
    fn crumbs_at_home() {
        let home = Path::new("/home/ada");
        let crumbs = breadcrumbs(home, Some(home));
        assert_eq!(crumbs.len(), 1);
        assert_eq!(crumbs[0].0, "Home");
    }

    #[test]
    fn crumbs_from_root() {
        let crumbs = breadcrumbs(Path::new("/usr/share"), None);
        assert_eq!(
            crumbs.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(),
            ["/", "usr", "share"]
        );
    }

    #[test]
    fn filter_matches_extension_case() {
        let filter = Filter {
            label: "Images".into(),
            extensions: vec!["png".into(), "JPEG".into()],
        };
        assert!(matches_filter("Shot.PNG", Some(&filter)));
        assert!(matches_filter("x.jpeg", Some(&filter)));
        assert!(!matches_filter("notes.txt", Some(&filter)));
        assert!(matches_filter("any.bin", None));
    }

    #[test]
    fn human_size_steps() {
        assert_eq!(human_size(400), "400 B");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(human_size(1_572_864), "1.5 MB");
    }

    #[test]
    fn closest_place_prefers_longest_prefix() {
        let places = vec![
            Place {
                label: "Home".into(),
                path: PathBuf::from("/home/ada"),
            },
            Place {
                label: "Pictures".into(),
                path: PathBuf::from("/home/ada/Pictures"),
            },
            Place {
                label: "Disk".into(),
                path: PathBuf::from("/"),
            },
        ];
        assert_eq!(
            closest_place(&places, Path::new("/home/ada/Pictures/x")),
            Some(1)
        );
        assert_eq!(closest_place(&places, Path::new("/tmp")), Some(2));
    }

    #[test]
    fn listings_hide_dot_files_and_files_when_asked() {
        let dir = scratch("list");
        fs::create_dir(dir.join("Zed")).unwrap();
        fs::create_dir(dir.join("alpha")).unwrap();
        fs::write(dir.join("b.txt"), "x").unwrap();
        fs::write(dir.join(".secret"), "x").unwrap();
        let names = |show: Show<'_>| -> Vec<String> {
            list_dir(&dir, show).unwrap().into_iter().map(|e| e.name).collect()
        };
        assert_eq!(names(Show::default()), ["alpha", "Zed", "b.txt"]);
        assert_eq!(
            names(Show {
                hidden: true,
                ..Show::default()
            }),
            ["alpha", "Zed", ".secret", "b.txt"]
        );
        assert_eq!(
            names(Show {
                dirs_only: true,
                ..Show::default()
            }),
            ["alpha", "Zed"]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_folders_take_a_free_name() {
        let dir = scratch("names");
        assert_eq!(free_folder_name(&dir), "untitled folder");
        fs::create_dir(dir.join("untitled folder")).unwrap();
        assert_eq!(free_folder_name(&dir), "untitled folder 2");
        let _ = fs::remove_dir_all(&dir);
    }
}
