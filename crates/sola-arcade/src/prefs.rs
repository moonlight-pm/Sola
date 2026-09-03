//! Gallery chrome prefs (sort). Nest size stays in `arcade-nest.json`.

use serde::{Deserialize, Serialize};
use sola_core::config::JsonConfig;

use crate::steam::SortMode;

/// Persisted Arcade UI prefs (`~/.config/sola/arcade-prefs.json`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArcadePrefs {
    #[serde(default)]
    pub sort: SortMode,
}

impl JsonConfig for ArcadePrefs {
    const FILE_NAME: &'static str = "arcade-prefs.json";
}

impl ArcadePrefs {
    pub fn set_sort(&mut self, sort: SortMode) {
        if self.sort == sort {
            return;
        }
        self.sort = sort;
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_sort_is_alphabetical() {
        assert_eq!(ArcadePrefs::default().sort, SortMode::Alphabetical);
        let empty: ArcadePrefs = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.sort, SortMode::Alphabetical);
    }

    #[test]
    fn recency_roundtrips() {
        let json = serde_json::to_string(&ArcadePrefs {
            sort: SortMode::Recency,
        })
        .unwrap();
        assert!(json.contains("recency"), "{json}");
        let back: ArcadePrefs = serde_json::from_str(&json).unwrap();
        assert_eq!(back.sort, SortMode::Recency);
    }
}
