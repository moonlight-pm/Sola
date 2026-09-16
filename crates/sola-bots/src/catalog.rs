//! Persistent bot catalog.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub bots: Vec<BotRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotRecord {
    pub id: String,
    pub name: String,
    pub slug: String,
    #[serde(default = "default_vendor")]
    pub vendor: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub grok_session_id: Option<String>,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub last_used: String,
}

fn default_vendor() -> String {
    "grok".into()
}

fn default_model() -> String {
    "grok-4.6".into()
}

impl Default for Catalog {
    fn default() -> Self {
        Self { bots: Vec::new() }
    }
}

impl Catalog {
    pub fn load() -> Self {
        let path = paths::catalog_path();
        match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let dir = paths::config_dir();
        fs::create_dir_all(&dir)?;
        let tmp = dir.join("catalog.json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(tmp, paths::catalog_path())?;
        Ok(())
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

pub fn unique_slug(catalog: &Catalog, base: &str) -> String {
    if catalog.bots.iter().all(|b| b.slug != base) {
        return base.to_string();
    }
    for i in 2..1000 {
        let s = format!("{base}-{i}");
        if catalog.bots.iter().all(|b| b.slug != s) {
            return s;
        }
    }
    format!("{base}-{}", uuid::Uuid::new_v4().simple())
}

pub fn home_of(record: &BotRecord) -> PathBuf {
    paths::bot_home(&record.slug)
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
                slug: "suno".into(),
                vendor: "grok".into(),
                model: "grok-4.6".into(),
                grok_session_id: None,
                created: String::new(),
                last_used: String::new(),
            }],
        };
        assert_eq!(unique_slug(&cat, "suno"), "suno-2");
        assert_eq!(unique_slug(&cat, "mail"), "mail");
    }
}
