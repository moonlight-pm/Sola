//! Enumerate font families installed on the host via Pango/fontconfig.
//! Used by KitApp to populate the typography editor's font pickers — sola
//! is a system app, so we know exactly what's available and only let the
//! user pick from that set (no generic-stack fallbacks).

use pango::prelude::*;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
pub struct FontList {
    /// Proportional families. Sorted, de-duplicated.
    pub sans: Vec<String>,
    /// Monospace families. Sorted, de-duplicated.
    pub mono: Vec<String>,
}

/// Walk PangoCairo's default font map (which delegates to fontconfig on
/// Linux), split into mono vs proportional, sort each list. Must be
/// called on the main thread (PangoCairo is single-threaded).
pub fn discover() -> FontList {
    use pango::prelude::FontMapExt;
    let map = pangocairo::FontMap::default();
    let mut sans = Vec::new();
    let mut mono = Vec::new();
    for family in map.list_families() {
        let name = family.name().to_string();
        if name.is_empty() {
            continue;
        }
        if family.is_monospace() {
            mono.push(name);
        } else {
            sans.push(name);
        }
    }
    sans.sort();
    sans.dedup();
    mono.sort();
    mono.dedup();
    FontList { sans, mono }
}
