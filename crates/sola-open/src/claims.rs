//! Claims: an app and the link schemes and file types it opens.
//!
//! Sola's own apps ship theirs ([`builtin`]), so a fresh machine opens
//! images before Paint has run. Any kit app claims at start
//! (`BusSetup::opens`): its claim goes to `handlers/claims/<app id>.json`,
//! one file per app, so apps never write each other's. `since` is kept from
//! the first time: the earlier claim comes first, and a later one never
//! moves ahead of it.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::target::Kind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub app_id: String,
    #[serde(default)]
    pub schemes: Vec<String>,
    #[serde(default)]
    pub types: Vec<String>,
    /// When the app first claimed (seconds since the epoch); 0 for Sola's own.
    #[serde(default)]
    pub since: u64,
}

impl Claim {
    pub fn new(app_id: &str, schemes: &[&str], types: &[&str]) -> Claim {
        Claim {
            app_id: app_id.into(),
            schemes: schemes.iter().map(|s| s.to_ascii_lowercase()).collect(),
            types: types.iter().map(|t| t.to_ascii_lowercase()).collect(),
            since: 0,
        }
    }

    pub fn covers(&self, kind: &Kind) -> bool {
        match kind {
            Kind::Scheme(s) => self.schemes.iter().any(|c| c == s),
            Kind::Type(t) => self.types.iter().any(|c| c == t),
        }
    }
}

/// The source and config files Sola's editor opens by default.
const TEXT_EXTENSIONS: &[&str] = &[
    "rs", "toml", "md", "markdown", "py", "sh", "bash", "zsh", "fish", "js", "jsx", "ts", "tsx",
    "css", "scss", "c", "h", "cc", "cpp", "hpp", "go", "rb", "lua", "java", "kt", "swift", "zig",
    "yaml", "yml", "nix", "sql", "conf", "cfg", "ini", "env", "log", "lock", "thx", "diff",
    "patch",
];

/// Sola's own apps' claims.
pub fn builtin() -> Vec<Claim> {
    vec![
        Claim::new(
            "sola-browser",
            &["http", "https"],
            &["html", "htm", "xhtml", "pdf", "svg", "txt", "json", "xml"],
        ),
        Claim::new("sola-paint", &[], sola_core::open_image::IMAGE_EXTENSIONS),
        Claim::new("sola-calendar", &[], sola_core::open_calendar::CALENDAR_EXTENSIONS),
        Claim::new("sola-mail", &["mailto"], &[]),
        // After the browser, which keeps txt, json and xml.
        Claim::new("sola-editor", &[], TEXT_EXTENSIONS),
    ]
}

/// Sola's own claim for `app_id`, if it has one.
pub fn builtin_of(app_id: &str) -> Option<Claim> {
    builtin().into_iter().find(|c| c.app_id == app_id)
}

/// `~/.config/sola/handlers`.
pub fn root() -> PathBuf {
    sola_core::config::sola_config_dir().join("handlers")
}

/// Record `claim` for its app in `dir`, keeping the first `since`.
pub fn register_in(dir: &Path, claim: &Claim) -> Result<(), String> {
    let path = dir.join(format!("{}.json", claim.app_id));
    let since = sola_core::config::load_json::<Claim>(&path)
        .ok()
        .map(|old| old.since)
        .filter(|&s| s > 0)
        .unwrap_or_else(now);
    let claim = Claim { since, ..claim.clone() };
    sola_core::config::save_json_pretty(&path, &claim).map_err(|e| e.to_string())
}

/// Record `claim` in this user's claims.
pub fn register(claim: &Claim) -> Result<(), String> {
    register_in(&root().join("claims"), claim)
}

/// The claims recorded in `dir`, earliest first.
pub fn recorded_in(dir: &Path) -> Vec<Claim> {
    let mut claims: Vec<Claim> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| sola_core::config::load_json::<Claim>(&e.path()).ok())
        .collect();
    claims.sort_by(|a, b| a.since.cmp(&b.since).then_with(|| a.app_id.cmp(&b.app_id)));
    claims
}

/// This user's recorded claims, earliest first.
pub fn recorded() -> Vec<Claim> {
    recorded_in(&root().join("claims"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(1)
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_claim_keeps_its_first_time() {
        let dir = tempfile::tempdir().unwrap();
        let first = Claim::new("foo", &["mailto"], &[]);
        register_in(dir.path(), &first).unwrap();
        let since = recorded_in(dir.path())[0].since;
        assert!(since > 0);
        let again = Claim::new("foo", &["mailto", "news"], &["eml"]);
        register_in(dir.path(), &again).unwrap();
        let got = &recorded_in(dir.path())[0];
        assert_eq!(got.since, since);
        assert_eq!(got.schemes, ["mailto", "news"]);
    }

    #[test]
    fn claims_come_earliest_first() {
        let dir = tempfile::tempdir().unwrap();
        for (id, since) in [("late", 20), ("early", 10)] {
            let c = Claim { since, ..Claim::new(id, &["x"], &[]) };
            sola_core::config::save_json_pretty(&dir.path().join(format!("{id}.json")), &c).unwrap();
        }
        let ids: Vec<String> = recorded_in(dir.path()).into_iter().map(|c| c.app_id).collect();
        assert_eq!(ids, ["early", "late"]);
    }

    #[test]
    fn sola_claims_cover_their_kinds() {
        let paint = builtin_of("sola-paint").unwrap();
        assert!(paint.covers(&Kind::Type("png".into())));
        assert!(!paint.covers(&Kind::Scheme("png".into())));
        assert!(builtin_of("sola-mail").unwrap().covers(&Kind::Scheme("mailto".into())));
    }
}
