//! Per-title nest size: **Fit to window** or a locked resolution.
//!
//! Mutually exclusive. Default is 1080p. Arcade applies this as gamescope
//! `-w/-h` (virtual monitor). Fit starts from the Sola output at Play, then
//! follows the gamescope host frame (zone/float) via nested X mode-control.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sola_core::config::JsonConfig;

use crate::launch::{DEFAULT_HOST_HEIGHT, DEFAULT_HOST_WIDTH};

/// 16:9 ladder offered in the dropdown, in addition to the display's native
/// mode when that is not already on the list. Entries larger than native are
/// dropped once output geometry is known.
const STANDARD_RES: &[(u32, u32)] = &[(1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)];

/// One title's nest setting. Fit and a resolution cannot both be on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NestChoice {
    Fit,
    Resolution { width: u32, height: u32 },
}

impl Default for NestChoice {
    fn default() -> Self {
        Self::Resolution {
            width: DEFAULT_HOST_WIDTH,
            height: DEFAULT_HOST_HEIGHT,
        }
    }
}

impl NestChoice {
    /// Nested `-w/-h` (and initial `-W/-H`) for this Play.
    pub fn resolve(self, native: Option<(u32, u32)>) -> (u32, u32) {
        match self {
            Self::Fit => native.unwrap_or((DEFAULT_HOST_WIDTH, DEFAULT_HOST_HEIGHT)),
            Self::Resolution { width, height } => (width.max(1), height.max(1)),
        }
    }

    /// Compact trigger label on the gallery row.
    pub fn trigger_label(self) -> String {
        match self {
            Self::Fit => "Fit".into(),
            Self::Resolution { width, height } => res_label(width, height),
        }
    }
}

/// Persisted map of Steam app id → nest choice.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NestFile {
    #[serde(default)]
    games: HashMap<String, NestChoice>,
}

impl JsonConfig for NestFile {
    const FILE_NAME: &'static str = "arcade-nest.json";
}

impl NestFile {
    pub fn get(&self, steam_app_id: u32) -> NestChoice {
        self.games
            .get(&steam_app_id.to_string())
            .copied()
            .unwrap_or_default()
    }

    pub fn set(&mut self, steam_app_id: u32, choice: NestChoice) {
        self.games.insert(steam_app_id.to_string(), choice);
        self.save();
    }
}

/// Dropdown rows: Fit first, then resolutions up to native. One selected.
pub fn menu_entries(native: Option<(u32, u32)>) -> Vec<(NestChoice, String)> {
    let mut out = Vec::with_capacity(STANDARD_RES.len() + 2);
    out.push((NestChoice::Fit, "Fit to window".into()));
    for &(width, height) in &resolution_list(native) {
        out.push((
            NestChoice::Resolution { width, height },
            menu_res_label(width, height, native),
        ));
    }
    out
}

/// Locked-resolution candidates, smallest first, native last when distinct.
pub fn resolution_list(native: Option<(u32, u32)>) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = STANDARD_RES
        .iter()
        .copied()
        .filter(|&(w, h)| fits_native(w, h, native))
        .collect();
    if let Some(n) = native {
        if n.0 > 0 && n.1 > 0 && !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

fn fits_native(w: u32, h: u32, native: Option<(u32, u32)>) -> bool {
    match native {
        Some((nw, nh)) if nw > 0 && nh > 0 => w <= nw && h <= nh,
        _ => true,
    }
}

fn res_label(width: u32, height: u32) -> String {
    match (width, height) {
        (1280, 720) => "720p".into(),
        (1920, 1080) => "1080p".into(),
        (2560, 1440) => "1440p".into(),
        (3840, 2160) => "4K".into(),
        _ => format!("{width}\u{00d7}{height}"),
    }
}

fn menu_res_label(width: u32, height: u32, native: Option<(u32, u32)>) -> String {
    let base = res_label(width, height);
    if native == Some((width, height)) {
        format!("{base} \u{00b7} native")
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_1080p() {
        assert_eq!(
            NestChoice::default(),
            NestChoice::Resolution {
                width: 1920,
                height: 1080
            }
        );
    }

    #[test]
    fn fit_and_resolution_are_distinct() {
        assert_ne!(
            NestChoice::Fit,
            NestChoice::Resolution {
                width: 1920,
                height: 1080
            }
        );
    }

    #[test]
    fn catalog_stops_at_native_1440() {
        let list = resolution_list(Some((2560, 1440)));
        assert_eq!(list, vec![(1280, 720), (1920, 1080), (2560, 1440)]);
    }

    #[test]
    fn ultrawide_native_appended() {
        let list = resolution_list(Some((5120, 2160)));
        assert!(list.contains(&(3840, 2160)));
        assert_eq!(*list.last().unwrap(), (5120, 2160));
    }

    #[test]
    fn unknown_native_keeps_full_ladder() {
        let list = resolution_list(None);
        assert_eq!(
            list,
            vec![(1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)]
        );
    }

    #[test]
    fn four_k_display_does_not_duplicate_native() {
        let list = resolution_list(Some((3840, 2160)));
        assert_eq!(list.iter().filter(|&&r| r == (3840, 2160)).count(), 1);
        assert_eq!(*list.last().unwrap(), (3840, 2160));
    }

    #[test]
    fn fit_resolves_to_native_else_1080p() {
        assert_eq!(NestChoice::Fit.resolve(Some((5120, 2160))), (5120, 2160));
        assert_eq!(NestChoice::Fit.resolve(None), (1920, 1080));
    }

    #[test]
    fn locked_res_ignores_native() {
        let c = NestChoice::Resolution {
            width: 1280,
            height: 720,
        };
        assert_eq!(c.resolve(Some((5120, 2160))), (1280, 720));
    }

    #[test]
    fn menu_is_fit_then_resolutions_one_selected() {
        let entries = menu_entries(Some((1920, 1080)));
        assert_eq!(entries[0].0, NestChoice::Fit);
        assert_eq!(entries[0].1, "Fit to window");
        let choice = NestChoice::default();
        let selected: Vec<_> = entries.iter().filter(|(c, _)| *c == choice).collect();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].1, "1080p \u{00b7} native");
    }

    #[test]
    fn trigger_labels_are_short() {
        assert_eq!(NestChoice::Fit.trigger_label(), "Fit");
        assert_eq!(
            NestChoice::Resolution {
                width: 1920,
                height: 1080
            }
            .trigger_label(),
            "1080p"
        );
        assert_eq!(
            NestChoice::Resolution {
                width: 5120,
                height: 2160
            }
            .trigger_label(),
            "5120\u{00d7}2160"
        );
    }

    #[test]
    fn json_roundtrip_fit_and_res() {
        let mut file = NestFile::default();
        file.games.insert("427520".into(), NestChoice::Fit);
        file.games.insert(
            "400".into(),
            NestChoice::Resolution {
                width: 2560,
                height: 1440,
            },
        );
        let json = serde_json::to_string(&file).unwrap();
        let back: NestFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.get(427520), NestChoice::Fit);
        assert_eq!(
            back.get(400),
            NestChoice::Resolution {
                width: 2560,
                height: 1440
            }
        );
        assert_eq!(back.get(1), NestChoice::default());
    }
}
