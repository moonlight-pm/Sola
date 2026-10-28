//! The bots: every directory in `~/Bots` is one, as every directory in
//! `~/Projects` is a project. Its `bot.json` holds its name, its AI and
//! its session; a directory without one (made by hand) is a bot whose AI
//! is not chosen yet.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::ai::Ai;
use crate::paths;

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub bots: Vec<BotRecord>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BotRecord {
    pub id: String,
    pub name: String,
    /// The directory's name in `~/Bots`.
    pub slug: String,
    /// `None` until one is chosen; fixed after.
    pub ai: Option<Ai>,
    /// The AI's model; empty for the AI's own default.
    pub model: String,
    /// The AI's session: Grok's ACP session, Claude Code's session id.
    pub session: Option<String>,
    pub created: String,
    pub last_used: String,
}

/// `bot.json`. The slug is the directory, so it is not written.
#[derive(Debug, Default, Serialize, Deserialize)]
struct BotFile {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    ai: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<String>,
    #[serde(default)]
    created: String,
    #[serde(default)]
    last_used: String,
}

/// What a [`Catalog::rescan`] found: bots whose directories appeared, and
/// the ids of those whose directories went.
#[derive(Debug, Default)]
pub struct Rescan {
    pub added: Vec<BotRecord>,
    pub removed: Vec<String>,
}

impl Catalog {
    pub fn load() -> Self {
        migrate_legacy();
        let mut catalog = Self::default();
        catalog.rescan();
        catalog
    }

    /// Bring the list in line with `~/Bots`.
    pub fn rescan(&mut self) -> Rescan {
        let slugs = bot_dirs();
        let mut found = Rescan::default();
        self.bots.retain(|b| {
            let keep = slugs.contains(&b.slug);
            if !keep {
                found.removed.push(b.id.clone());
            }
            keep
        });
        for slug in slugs {
            if self.bots.iter().all(|b| b.slug != slug) {
                let rec = read_record(&slug);
                self.insert(rec.clone());
                found.added.push(rec);
            }
        }
        found
    }

    /// Add `rec`, keeping the list in name order.
    pub fn insert(&mut self, rec: BotRecord) {
        self.bots.push(rec);
        self.bots
            .sort_by_key(|b| (b.name.to_lowercase(), b.slug.clone()));
    }

    pub fn find(&self, key: &str) -> Option<&BotRecord> {
        self.bots
            .iter()
            .find(|b| b.id == key || b.slug == key || b.name.eq_ignore_ascii_case(key))
    }

    pub fn find_mut(&mut self, key: &str) -> Option<&mut BotRecord> {
        self.bots
            .iter_mut()
            .find(|b| b.id == key || b.slug == key || b.name.eq_ignore_ascii_case(key))
    }
}

/// Write `rec`'s `bot.json`.
pub fn save(rec: &BotRecord) -> anyhow::Result<()> {
    let home = paths::bot_home(&rec.slug);
    fs::create_dir_all(&home)?;
    let file = BotFile {
        id: rec.id.clone(),
        name: rec.name.clone(),
        ai: rec.ai.map(|a| a.id().to_string()).unwrap_or_default(),
        model: rec.model.clone(),
        session: rec.session.clone(),
        created: rec.created.clone(),
        last_used: rec.last_used.clone(),
    };
    let tmp = home.join("bot.json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(&file)?)?;
    fs::rename(tmp, paths::bot_config(&rec.slug))?;
    Ok(())
}

/// The directories in `~/Bots`, hidden ones aside.
fn bot_dirs() -> Vec<String> {
    let Ok(entries) = fs::read_dir(paths::bots_root()) else {
        return Vec::new();
    };
    let mut slugs: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| !name.starts_with('.'))
        .collect();
    slugs.sort();
    slugs
}

fn read_record(slug: &str) -> BotRecord {
    let path = paths::bot_config(slug);
    let file: BotFile = match fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
            warn!(path = %path.display(), "bot.json unreadable ({e}); AI not chosen");
            BotFile::default()
        }),
        Err(_) => BotFile::default(),
    };
    let ai = Ai::parse(&file.ai);
    if ai.is_none() && !file.ai.is_empty() {
        warn!(%slug, ai = %file.ai, "unknown AI in bot.json");
    }
    BotRecord {
        id: non_empty(file.id, slug),
        name: non_empty(file.name, slug),
        slug: slug.to_string(),
        ai,
        model: file.model,
        session: file.session,
        created: file.created,
        last_used: file.last_used,
    }
}

fn non_empty(s: String, fallback: &str) -> String {
    if s.trim().is_empty() {
        fallback.to_string()
    } else {
        s
    }
}

/// The catalog file bots had before they were their directories.
#[derive(Deserialize)]
struct LegacyCatalog {
    #[serde(default)]
    bots: Vec<LegacyRecord>,
}

#[derive(Deserialize)]
struct LegacyRecord {
    id: String,
    name: String,
    slug: String,
    #[serde(default)]
    vendor: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    grok_session_id: Option<String>,
    #[serde(default)]
    created: String,
    #[serde(default)]
    last_used: String,
}

/// Move `catalog.json` into each bot's `bot.json` (all were Grok), once.
fn migrate_legacy() {
    let path = paths::legacy_catalog_path();
    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };
    let legacy: LegacyCatalog = match serde_json::from_str(&text) {
        Ok(c) => c,
        Err(e) => {
            warn!(path = %path.display(), "old bot catalog unreadable ({e}); left in place");
            return;
        }
    };
    for old in legacy.bots {
        if paths::bot_config(&old.slug).exists() {
            continue;
        }
        let rec = BotRecord {
            id: old.id,
            name: old.name,
            slug: old.slug,
            ai: Some(Ai::parse(&old.vendor).unwrap_or(Ai::Grok)),
            // `grok-4.6` was a placeholder; Grok ran its own default.
            model: if old.model == "grok-4.6" { String::new() } else { old.model },
            session: old.grok_session_id,
            created: old.created,
            last_used: old.last_used,
        };
        if let Err(e) = save(&rec) {
            warn!(slug = %rec.slug, "moving bot to bot.json failed: {e}");
            return;
        }
        info!(slug = %rec.slug, "bot moved to bot.json");
    }
    let moved = PathBuf::from(format!("{}.moved", path.display()));
    if let Err(e) = fs::rename(&path, &moved) {
        warn!("old bot catalog not renamed: {e}");
    }
}

pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "bot".into() } else { out }
}

/// `base`, or `base-2`… when a bot or a directory already has it.
pub fn unique_slug(catalog: &Catalog, base: &str) -> String {
    let free = |s: &str| catalog.bots.iter().all(|b| b.slug != s) && !paths::bot_home(s).exists();
    if free(base) {
        return base.to_string();
    }
    for i in 2..1000 {
        let s = format!("{base}-{i}");
        if free(&s) {
            return s;
        }
    }
    format!("{base}-{}", uuid::Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_from_name() {
        assert_eq!(slugify("Suno Music"), "suno-music");
        assert_eq!(slugify("  "), "bot");
        assert_eq!(slugify("--Foo--Bar--"), "foo-bar");
    }

    #[test]
    fn unique_slug_suffixes() {
        let cat = Catalog {
            bots: vec![BotRecord {
                id: "1".into(),
                name: "Suno".into(),
                slug: "sola-bots-test-suno".into(),
                ai: Some(Ai::Grok),
                model: String::new(),
                session: None,
                created: String::new(),
                last_used: String::new(),
            }],
        };
        assert_eq!(unique_slug(&cat, "sola-bots-test-suno"), "sola-bots-test-suno-2");
        assert_eq!(unique_slug(&cat, "sola-bots-test-mail"), "sola-bots-test-mail");
    }

    #[test]
    fn a_bot_file_without_an_ai_reads_as_not_chosen() {
        let file: BotFile = serde_json::from_str(r#"{"id":"x","name":"X"}"#).unwrap();
        assert_eq!(Ai::parse(&file.ai), None);
        let file: BotFile = serde_json::from_str(r#"{"ai":"claude"}"#).unwrap();
        assert_eq!(Ai::parse(&file.ai), Some(Ai::Claude));
    }
}
