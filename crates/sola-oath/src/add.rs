//! Adding a package that is on the origin only: `sudo oath add <name>`
//! (R81) takes it at its newest signed head, with each package it needs
//! that this machine lacks, and applies them at once. Nothing here moves.
//! The app never passes `--confirm`: a change that needs it is refused,
//! and the refusal is shown in Oath's words. As home, `oath add` only
//! previews what it would make (Oath 0995609); an older Oath refuses, and
//! the sheet says less.

use serde_json::Value;

use crate::oath;

/// One package the add made.
#[derive(Debug, Clone, PartialEq)]
pub struct Made {
    pub name: String,
    pub hash: String,
    pub version: String,
    pub signer: String,
    /// Its needs pointed at this machine's builds instead.
    pub repointed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Added {
    pub made: Vec<Made>,
    pub generation: Option<u64>,
}

/// What `sudo oath add` would make, by `oath add` as home.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub made: Vec<Made>,
    /// The packages it needs that this machine lacks, made with it.
    pub needs: Vec<String>,
    /// Bytes to fetch, when every pack's size is known.
    pub size: Option<u64>,
}

/// What came of asking for `name`.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub name: String,
    pub result: Result<Added, String>,
}

fn name_of(id: &str) -> String {
    id.strip_prefix("pkg:").unwrap_or(id).to_string()
}

pub fn parse(v: &Value) -> Added {
    Added { made: made_in(v, "added"), generation: v.get("generation").and_then(Value::as_u64) }
}

/// None when Oath printed no preview.
pub fn parse_preview(v: &Value) -> Option<Preview> {
    let needs = v
        .get("would_add")?
        .as_array()?
        .iter()
        .filter(|m| m.get("asked").and_then(Value::as_bool) == Some(false))
        .filter_map(|m| m.get("id").and_then(Value::as_str))
        .map(name_of)
        .collect();
    let known = v.get("size_known").and_then(Value::as_bool).unwrap_or(false);
    Some(Preview {
        made: made_in(v, "would_add"),
        needs,
        size: v.get("size").and_then(Value::as_u64).filter(|_| known),
    })
}

fn made_in(v: &Value, key: &str) -> Vec<Made> {
    let s = |x: &Value, k: &str| x.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    v.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|m| Made {
            name: name_of(&s(m, "id")),
            hash: s(m, "hash"),
            version: s(m, "version"),
            signer: s(m, "signer"),
            repointed: m
                .get("repointed")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(name_of)
                .collect(),
        })
        .collect()
}

/// Ask what adding `name` would make; nothing changes.
pub fn preview(name: &str) -> Result<Preview, String> {
    oath::json(&["add", name]).and_then(|v| parse_preview(&v).ok_or_else(|| "no preview".into()))
}

pub fn run(name: String) -> Outcome {
    let result = oath::json_as_root(&["add", &name]).map(|v| parse(&v));
    match &result {
        Ok(a) => tracing::info!(name, made = a.made.len(), generation = ?a.generation, "added"),
        Err(e) => tracing::warn!(name, error = %e, "oath add refused"),
    }
    Outcome { name, result }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn what_oath_add_prints() {
        // `oath add --json` (Oath's add.rs, `Made::json`).
        let a = parse(&json!({
            "added": [
                {"id": "pkg:python-mako", "hash": "sha256-a", "version": "1.3", "signer": "oath@buccina",
                 "repointed": ["pkg:python"]},
                {"id": "pkg:python-markupsafe", "hash": "sha256-b", "version": "3.0", "signer": "oath@buccina",
                 "repointed": []}
            ],
            "here": [], "generation": 129
        }));
        assert_eq!(a.generation, Some(129));
        assert_eq!(a.made.len(), 2);
        assert_eq!(a.made[0].name, "python-mako");
        assert_eq!(a.made[0].repointed, ["python"]);
    }

    #[test]
    fn what_oath_add_previews_as_home() {
        // Oath 0995609, add.rs `preview`.
        let p = parse_preview(&json!({
            "would_add": [
                {"id": "pkg:python-mako", "hash": "sha256-a", "version": "1.4.3", "signer": "oath@buccina",
                 "repointed": [], "asked": true, "size": 273030, "in_store": false},
                {"id": "pkg:python-markupsafe", "hash": "sha256-b", "version": "3.0.3", "signer": "oath@buccina",
                 "repointed": [], "asked": false, "size": 13673, "in_store": false}
            ],
            "size": 286703, "size_known": true, "added": [], "here": [], "generation": null,
            "run": "sudo oath add python-mako"
        }))
        .unwrap();
        assert_eq!(p.needs, ["python-markupsafe"]);
        assert_eq!(p.size, Some(286703));
        assert_eq!(p.made[0].version, "1.4.3");
        // An older Oath refuses instead: no preview.
        assert!(parse_preview(&json!({"error": "add changes this machine"})).is_none());
    }

    #[test]
    fn already_here_makes_nothing() {
        let a = parse(&json!({"added": [], "here": [{"id": "pkg:bc", "hash": "h"}], "generation": null}));
        assert!(a.made.is_empty());
        assert_eq!(a.generation, None);
    }
}
