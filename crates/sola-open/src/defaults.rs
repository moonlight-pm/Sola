//! The handlers you chose: `handlers/defaults.json`, a scheme (`mailto:`) or
//! a type (`.png`) to an app id. Written by Settings and by a request you
//! confirmed, never by a claim.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::target::Kind;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default)]
    pub defaults: BTreeMap<String, String>,
}

impl Defaults {
    pub fn load_from(path: &Path) -> Defaults {
        sola_core::config::load_json_or_default(path).unwrap_or_default()
    }

    pub fn load() -> Defaults {
        Self::load_from(&path())
    }

    /// The app you chose for `kind`.
    pub fn of(&self, kind: &Kind) -> Option<&str> {
        self.defaults.get(&kind.key()).map(String::as_str)
    }

    /// Choose `app_id` for `kind`.
    pub fn set_in(path: &Path, kind: &Kind, app_id: &str) -> Result<(), String> {
        let mut d = Self::load_from(path);
        d.defaults.insert(kind.key(), app_id.to_string());
        sola_core::config::save_json_pretty(path, &d).map_err(|e| e.to_string())
    }
}

/// `~/.config/sola/handlers/defaults.json`.
pub fn path() -> PathBuf {
    crate::claims::root().join("defaults.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_reads_back_by_kind() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("defaults.json");
        let mailto = Kind::Scheme("mailto".into());
        assert_eq!(Defaults::load_from(&p).of(&mailto), None);
        Defaults::set_in(&p, &mailto, "foo-mail").unwrap();
        let d = Defaults::load_from(&p);
        assert_eq!(d.of(&mailto), Some("foo-mail"));
        assert_eq!(d.of(&Kind::Type("mailto".into())), None);
    }
}
