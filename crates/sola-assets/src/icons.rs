//! Icon-pack path helpers.
//!
//! Pack icons live at `/opt/sola/share/icons/<pack>/<name>.{svg,png,…}`.
//! User config also accepts an absolute (or `~/…`) filesystem path for
//! full-color app icons extracted from AppImages, `.desktop` files, etc.
//!
//! Use [`path`] / [`read_svg`] for the monochrome SVG packs (lucide,
//! simpleicons, sola). Use [`raster_path`] when the ref should render
//! as an untinted bitmap (launcher / switcher app faces).

use std::path::{Path, PathBuf};

use crate::resolve;

/// Raster extensions we will load as full-color app icons.
const RASTER_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];

/// Filesystem path to an SVG pack icon, if it exists.
pub fn path(pack: &str, name: &str) -> Option<PathBuf> {
    resolve(&format!("icons/{pack}/{name}.svg"))
}

/// URI for referencing an icon from a WebView.
///
/// Does not check whether the icon exists — WebKit's broken-image handling
/// covers missing files and keeps callers decoupled from disk I/O.
pub fn uri(pack: &str, name: &str) -> String {
    format!("sola-assets://icons/{pack}/{name}.svg")
}

/// Parse `"<pack>/<name>"` (the form used in user config) into `(pack, name)`.
///
/// Absolute paths and `~/…` refs are **not** pack refs — use [`raster_path`].
pub fn split_ref(s: &str) -> Option<(&str, &str)> {
    if s.starts_with('/') || s.starts_with('~') {
        return None;
    }
    let (pack, name) = s.split_once('/')?;
    (!pack.is_empty() && !name.is_empty()).then_some((pack, name))
}

/// Read the raw SVG bytes for `"<pack>/<name>"` (e.g. `"lucide/menu"`).
///
/// Returns `None` when the icon does not exist under `ASSETS_DIR`, or
/// when `icon_ref` is a filesystem path rather than a pack ref.
/// The bytes are read fresh from disk on each call; callers that render
/// frequently should cache the result (e.g. as an `iced::widget::svg::Handle`).
pub fn read_svg(icon_ref: &str) -> Option<Vec<u8>> {
    let (pack, name) = split_ref(icon_ref)?;
    let file_path = path(pack, name)?;
    std::fs::read(&file_path).ok()
}

/// Resolve an icon ref to a raster image on disk, if one applies.
///
/// Accepts:
/// - absolute path: `/home/…/orca-ide.png`
/// - home-relative: `~/…/orca-ide.png`
/// - pack name with a raster under `/opt/sola/share/icons/<pack>/<name>.png`
///   (and other [`RASTER_EXTS`]) when no `.svg` is required by the caller
///
/// Returns `None` for missing files or non-raster pack-only refs.
pub fn raster_path(icon_ref: &str) -> Option<PathBuf> {
    let trimmed = icon_ref.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(path) = filesystem_ref(trimmed) {
        return is_raster_file(&path).then_some(path);
    }

    let (pack, name) = split_ref(trimmed)?;
    for ext in RASTER_EXTS {
        if let Some(p) = resolve(&format!("icons/{pack}/{name}.{ext}")) {
            return Some(p);
        }
    }
    None
}

/// Expand `~/…` and treat absolute paths as filesystem icon refs.
fn filesystem_ref(icon_ref: &str) -> Option<PathBuf> {
    if let Some(rest) = icon_ref.strip_prefix("~/") {
        let home = std::env::var_os("HOME")?;
        return Some(PathBuf::from(home).join(rest));
    }
    if icon_ref == "~" {
        return std::env::var_os("HOME").map(PathBuf::from);
    }
    let p = Path::new(icon_ref);
    p.is_absolute().then(|| p.to_path_buf())
}

fn is_raster_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    RASTER_EXTS.iter().any(|e| *e == ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_is_well_formed() {
        assert_eq!(
            uri("lucide", "terminal"),
            "sola-assets://icons/lucide/terminal.svg"
        );
    }

    #[test]
    fn split_ref_parses_normal_input() {
        assert_eq!(split_ref("lucide/terminal"), Some(("lucide", "terminal")));
    }

    #[test]
    fn split_ref_rejects_malformed_and_paths() {
        assert_eq!(split_ref(""), None);
        assert_eq!(split_ref("lucide"), None);
        assert_eq!(split_ref("/terminal"), None);
        assert_eq!(split_ref("lucide/"), None);
        assert_eq!(split_ref("/home/user/icon.png"), None);
        assert_eq!(split_ref("~/icon.png"), None);
    }

    #[test]
    fn filesystem_ref_expands_home() {
        let home = std::env::var("HOME").expect("HOME");
        let got = filesystem_ref("~/Applications/icon.png").expect("expand");
        assert_eq!(got, PathBuf::from(home).join("Applications/icon.png"));
    }

    #[test]
    fn filesystem_ref_absolute() {
        let got = filesystem_ref("/tmp/orca.png").expect("abs");
        assert_eq!(got, PathBuf::from("/tmp/orca.png"));
    }
}
