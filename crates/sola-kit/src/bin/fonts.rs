//! Enumerate font families installed on the host. Used by KitApp to populate
//! the typography editor's font pickers.
//!
//! TODO(post-CEF-port): the previous WebKit/GTK-based implementation walked
//! PangoCairo's default font map. With the move to CEF + sctk we no longer
//! pull in pango/pangocairo; reimplement against fontconfig directly (the
//! `fontconfig` crate, or `fc-match`/`fc-list` shell-out) when the typography
//! editor needs real data again. For now the picker shows an empty list.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
pub struct FontList {
    /// Proportional families. Sorted, de-duplicated.
    pub sans: Vec<String>,
    /// Monospace families. Sorted, de-duplicated.
    pub mono: Vec<String>,
}

/// Returns an empty FontList. Replace with a fontconfig-backed walk once the
/// typography editor is wired back up.
pub fn discover() -> FontList {
    FontList::default()
}
