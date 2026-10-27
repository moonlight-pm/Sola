//! A page's file chooser — `<input type=file>` (one file, several, a folder)
//! and `showSaveFilePicker`. CEF OSR draws no dialog of its own: the
//! helper's `DialogHandler` captures the request, the chrome shows the
//! kit's [`FilePicker`] over the page (the same panel every Sola app
//! opens and saves with), and the answer completes CEF's callback.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use sola_kit::components::file_picker::FilePicker;

/// True while the panel is over a page: the page gets no keys under it.
static OPEN: AtomicBool = AtomicBool::new(false);

pub fn is_open() -> bool {
    OPEN.load(Ordering::Relaxed)
}

pub fn set_open(open: bool) {
    OPEN.store(open, Ordering::Relaxed);
}

/// What the page asked for (CEF's `cef_file_dialog_mode_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    Open,
    OpenMultiple,
    OpenFolder,
    Save,
}

/// Helper → chrome: a page wants a file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Ipc {
    pub id: u64,
    pub tab_id: u64,
    pub mode: Mode,
    /// The page's title for the dialog (usually empty).
    pub title: String,
    /// A suggested file or folder (Save's name).
    pub default_path: String,
    /// `accept`: extensions (`.png`) and MIME types (`image/*`, `application/pdf`).
    pub accept: Vec<String>,
}

/// The panel for a page's request. `last_dir` is where the previous
/// upload came from (Downloads, the first time).
pub fn picker(ipc: &Ipc, last_dir: Option<&Path>) -> FilePicker {
    let picker = match ipc.mode {
        Mode::Open => FilePicker::open().title("Choose a file").action("Upload"),
        Mode::OpenMultiple => FilePicker::open()
            .multiple()
            .title("Choose files")
            .action("Upload"),
        Mode::OpenFolder => FilePicker::folder().title("Choose a folder").action("Upload"),
        Mode::Save => FilePicker::save().title("Save"),
    };
    let picker = if ipc.title.trim().is_empty() {
        picker
    } else {
        picker.title(ipc.title.trim())
    };
    let default = PathBuf::from(&ipc.default_path);
    let start = if !ipc.default_path.is_empty() && default.is_absolute() {
        default.clone()
    } else {
        last_dir.map(Path::to_path_buf).unwrap_or_else(downloads_dir)
    };
    let mut picker = picker.start_dir(start);
    if ipc.mode == Mode::Save
        && let Some(name) = default.file_name()
    {
        picker = picker.suggested_name(name.to_string_lossy());
    }
    match accepted_extensions(&ipc.accept) {
        Some(exts) if ipc.mode != Mode::OpenFolder && ipc.mode != Mode::Save => {
            let refs: Vec<&str> = exts.iter().map(String::as_str).collect();
            picker.filter(accept_label(&ipc.accept), &refs)
        }
        _ => picker,
    }
}

fn downloads_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into());
    let downloads = home.join("Downloads");
    if downloads.is_dir() { downloads } else { home }
}

const IMAGES: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "avif", "heic"];
const VIDEOS: &[&str] = &["mp4", "mov", "webm", "mkv", "m4v"];
const AUDIO: &[&str] = &["mp3", "m4a", "wav", "flac", "ogg", "opus", "aac"];

/// The extensions an `accept` list allows, or `None` to show every file:
/// an empty list, or anything this cannot read as extensions (better to
/// show too much than hide the file the page wants).
pub fn accepted_extensions(accept: &[String]) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for token in accept
        .iter()
        .flat_map(|a| a.split([',', ';']))
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        let lower = token.to_ascii_lowercase();
        let more: Vec<String> = if let Some(ext) = lower.strip_prefix('.') {
            vec![ext.to_string()]
        } else {
            match lower.as_str() {
                "image/*" => IMAGES.iter().map(|s| s.to_string()).collect(),
                "video/*" => VIDEOS.iter().map(|s| s.to_string()).collect(),
                "audio/*" => AUDIO.iter().map(|s| s.to_string()).collect(),
                "image/jpeg" => vec!["jpg".into(), "jpeg".into()],
                "image/svg+xml" => vec!["svg".into()],
                "application/pdf" => vec!["pdf".into()],
                "text/plain" => vec!["txt".into()],
                "text/csv" => vec!["csv".into()],
                "application/json" => vec!["json".into()],
                "application/zip" => vec!["zip".into()],
                m if m.starts_with("image/") || m.starts_with("video/") || m.starts_with("audio/") => {
                    vec![m.split('/').nth(1).unwrap_or("").to_string()]
                }
                _ => return None,
            }
        };
        for ext in more {
            if !ext.is_empty() && !out.contains(&ext) {
                out.push(ext);
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

fn accept_label(accept: &[String]) -> String {
    let joined = accept.join(", ");
    if joined.contains("image/*") {
        "Images".into()
    } else if joined.contains("video/*") {
        "Videos".into()
    } else if joined.contains("audio/*") {
        "Audio".into()
    } else {
        "Files the page accepts".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exts(accept: &[&str]) -> Option<Vec<String>> {
        accepted_extensions(&accept.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn accept_lists_become_extensions() {
        assert_eq!(exts(&[]), None);
        assert_eq!(exts(&[".pdf", ".DOCX"]).unwrap(), ["pdf", "docx"]);
        assert!(exts(&["image/*"]).unwrap().contains(&"png".to_string()));
        assert_eq!(exts(&["image/jpeg", "application/pdf"]).unwrap(), ["jpg", "jpeg", "pdf"]);
        // Something unknown: show everything rather than hide the file.
        assert_eq!(exts(&[".png", "application/x-thing"]), None);
    }

    #[test]
    fn save_suggests_the_pages_name() {
        let ipc = Ipc {
            id: 1,
            tab_id: 1,
            mode: Mode::Save,
            title: String::new(),
            default_path: "/tmp/report.csv".into(),
            accept: Vec::new(),
        };
        let p = picker(&ipc, None);
        assert_eq!(p.mode(), sola_kit::components::file_picker::Mode::Save);
    }
}
