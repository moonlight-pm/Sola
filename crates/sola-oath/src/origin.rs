//! What the origin publishes: its public listing (`index.json`) of every
//! pack and recipe, so the app can show what this machine has not taken.
//! The listing is not a release; which hashes a release pins comes only
//! from `oath update`.

use std::time::Duration;

use serde_json::Value;

/// One listed pack or recipe.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    pub name: String,
    pub version: String,
    pub category: Option<String>,
    pub license: String,
    pub size: Option<u64>,
    /// A recipe: a plan that fetches the vendor's build here, not a pack.
    pub recipe: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Listing {
    pub origin: String,
    pub at: String,
    pub items: Vec<Listed>,
}

pub fn parse(origin: &str, at: &str, v: &Value) -> Listing {
    let one = |x: &Value, recipe: bool| {
        let s = |k: &str| x.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        Listed {
            name: s("name"),
            version: s("version"),
            category: x.get("category").and_then(Value::as_str).map(str::to_string),
            license: s("license"),
            size: x.get("size").and_then(Value::as_u64),
            recipe,
        }
    };
    let list = |k: &str, recipe: bool| {
        v.get(k)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(move |x| one(x, recipe))
            .filter(|l| !l.name.is_empty())
            .collect::<Vec<_>>()
    };
    let mut items = list("packages", false);
    items.extend(list("recipes", true));
    Listing { origin: origin.to_string(), at: at.to_string(), items }
}

/// Fetch `origin/index.json`.
pub fn fetch(origin: &str) -> Result<Value, String> {
    let url = format!("{}/index.json", origin.trim_end_matches('/'));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into();
    let mut response = agent
        .get(&url)
        .header("User-Agent", concat!("sola-oath/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| format!("{url}: {e}"))?;
    let text = response.body_mut().read_to_string().map_err(|e| format!("{url}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{url}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn packs_and_recipes() {
        let l = parse(
            "https://o",
            "t",
            &json!({
                "packages": [{"name": "zlib", "hash": "h", "version": "1.3.2", "category": "base",
                              "license": "Zlib", "plan": "p", "source": "s", "size": 300}],
                "recipes": [{"name": "steam", "builds": "x", "version": "1", "category": "games",
                             "license": "Proprietary", "plan": "p"}]
            }),
        );
        assert_eq!(l.items.len(), 2);
        assert_eq!(l.items[0].size, Some(300));
        assert!(l.items[1].recipe);
        assert_eq!(l.items[1].category.as_deref(), Some("games"));
    }
}
