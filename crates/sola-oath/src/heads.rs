//! The heads this machine took (`/oath/heads.json`, R72): for each package
//! published on its own, the head `oath update` last moved it to. Only an
//! update writes it, so a head published since, from this machine too, is
//! not in it until the next one; a check says what the origins hold now.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::oath;

#[derive(Debug, Clone, PartialEq)]
pub struct Head {
    pub hash: String,
    /// The key that signed it (`oath@novus`).
    pub signer: String,
    pub published: String,
}

pub fn parse(v: &Value) -> BTreeMap<String, Head> {
    let Some(map) = v.as_object() else {
        return BTreeMap::new();
    };
    map.iter()
        .filter_map(|(name, h)| {
            let s = |k: &str| h.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
            let head = Head { hash: s("hash"), signer: s("signer"), published: s("published") };
            (!head.hash.is_empty()).then(|| (name.clone(), head))
        })
        .collect()
}

/// The record, or none when this machine has taken no head yet.
pub fn read() -> Result<BTreeMap<String, Head>, String> {
    let file = oath::heads_file();
    match std::fs::read_to_string(&file) {
        Ok(t) => serde_json::from_str(&t).map(|v| parse(&v)).map_err(|e| format!("{}: {e}", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(format!("{}: {e}", file.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn by_name() {
        let h = parse(&json!({
            "sola": {"hash": "sha256-90ca", "published": "2026-11-10T01:44:07Z",
                     "committed": "2026-11-10T01:40:34Z", "signer": "oath@novus"},
            "broken": {"signer": "x"}
        }));
        assert_eq!(h.len(), 1);
        assert_eq!(h["sola"].signer, "oath@novus");
        assert_eq!(h["sola"].hash, "sha256-90ca");
    }
}
