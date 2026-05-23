//! Icon-pack path helpers.
//!
//! Icons live at `/opt/sola/share/icons/<pack>/<name>.svg`. Use [`path`] to
//! check whether a named icon is actually present on disk.

use std::path::PathBuf;

use crate::resolve;

/// Filesystem path to an icon, if it exists.
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
pub fn split_ref(s: &str) -> Option<(&str, &str)> {
    let (pack, name) = s.split_once('/')?;
    (!pack.is_empty() && !name.is_empty()).then_some((pack, name))
}

/// Read the raw SVG bytes for `"<pack>/<name>"` (e.g. `"lucide/menu"`).
///
/// Returns `None` when the icon does not exist under `ASSETS_DIR`.
/// The bytes are read fresh from disk on each call; callers that render
/// frequently should cache the result (e.g. as an `iced::widget::svg::Handle`).
pub fn read_svg(icon_ref: &str) -> Option<Vec<u8>> {
    let (pack, name) = split_ref(icon_ref)?;
    let file_path = path(pack, name)?;
    std::fs::read(&file_path).ok()
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
    fn split_ref_rejects_malformed() {
        assert_eq!(split_ref(""), None);
        assert_eq!(split_ref("lucide"), None);
        assert_eq!(split_ref("/terminal"), None);
        assert_eq!(split_ref("lucide/"), None);
    }
}
