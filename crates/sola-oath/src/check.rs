//! Is anything newer out? `oath --json update` without `--confirm` says
//! what the newest output for this machine's profile, and each package's
//! newest head (R72), would change, and changes nothing. The app asks only
//! when its Check button is pressed, and keeps the last answer with its
//! time in its own state (Oath keeps none), so it shows on the next open.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::{oath, when};

/// One package's move in the newest release, or to its newest head.
#[derive(Debug, Clone, PartialEq)]
pub struct Move {
    /// `pin`, `keep`, `stay`, `new`, `rebuild`, `drop`, `head`, `hold` or
    /// `repoint`.
    pub kind: String,
    pub to_hash: String,
    pub to_version: String,
    /// Oath's own words for it.
    pub text: String,
    /// A pin whose hash is the same: only its record moves.
    pub record_only: bool,
}

impl Move {
    /// Nothing newer waits for it: only its record or its needs move, or
    /// no release carries it. (A `hold` keeps its pin, but a head waits.)
    pub fn nothing_newer(&self) -> bool {
        self.record_only || matches!(self.kind.as_str(), "keep" | "stay" | "repoint")
    }

    /// Its newest head, not a release, moves it.
    pub fn is_head(&self) -> bool {
        self.kind == "head"
    }
}

#[derive(Debug, Clone, Default)]
pub struct Check {
    /// When it was asked.
    pub at: String,
    /// The newest output for the profile, when Oath named it.
    pub newest: Option<String>,
    /// This machine already runs it.
    pub current: bool,
    pub moves: BTreeMap<String, Move>,
    /// The move needs a reboot (kernel, initrd or PID 1).
    pub reboot: bool,
    /// Packages the release would take away / make.
    pub gone: Vec<String>,
    pub made: Vec<String>,
    /// It could not say (unapplied changes, probation, no network).
    pub error: Option<String>,
}

impl Check {
    /// Packages whose tree would move, not just their record.
    pub fn moving(&self) -> impl Iterator<Item = (&String, &Move)> {
        self.moves.iter().filter(|(_, m)| !m.nothing_newer() && m.kind != "hold")
    }
}

fn ids(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().or_else(|| x.get("id").and_then(Value::as_str)))
                .map(|s| s.strip_prefix("pkg:").unwrap_or(s).to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// A move's `from` or `to`: a hash, or `{hash, version}`.
fn pin(v: Option<&Value>) -> (String, String) {
    let s = |k: &str| v.and_then(|x| x.get(k)).and_then(Value::as_str).unwrap_or_default().to_string();
    match v.and_then(Value::as_str) {
        Some(h) => (h.to_string(), String::new()),
        None => (s("hash"), s("version")),
    }
}

pub fn parse(at: &str, v: &Value) -> Check {
    let s = |x: Option<&Value>, k: &str| x.and_then(|x| x.get(k)).and_then(Value::as_str).unwrap_or_default().to_string();
    let newest = v
        .get("release")
        .and_then(|r| r.as_str().map(str::to_string).or_else(|| r.get("output").and_then(Value::as_str).map(str::to_string)))
        .or_else(|| v.get("output").and_then(Value::as_str).map(str::to_string))
        .filter(|s| !s.is_empty());
    let mut moves = BTreeMap::new();
    for m in v.get("moves").and_then(Value::as_array).into_iter().flatten() {
        let id = s(Some(m), "id");
        let name = id.strip_prefix("pkg:").unwrap_or(&id).to_string();
        if name.is_empty() || !id.starts_with("pkg:") {
            continue;
        }
        let ((from, _), (to_hash, to_version)) = (pin(m.get("from")), pin(m.get("to")));
        let kind = s(Some(m), "move");
        moves.insert(
            name,
            Move {
                record_only: matches!(kind.as_str(), "pin" | "head") && !to_hash.is_empty() && to_hash == from,
                kind,
                to_hash,
                to_version,
                text: s(Some(m), "text"),
            },
        );
    }
    Check {
        at: at.to_string(),
        newest,
        current: v.get("runs").and_then(Value::as_bool).unwrap_or(false),
        moves,
        reboot: v.get("reboot").and_then(Value::as_bool).unwrap_or(false),
        gone: ids(v.get("gone")),
        made: ids(v.get("made")),
        error: None,
    }
}

fn state_file() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| {
        std::env::var_os("HOME").map_or_else(|| PathBuf::from("/home"), PathBuf::from).join(".local/state")
    });
    base.join("sola-oath/check.json")
}

/// Ask Oath now, and keep the answer.
pub fn run() -> Check {
    let at = when::format(when::now());
    let kept = match oath::json(&["update"]) {
        Ok(v) => json!({ "at": at, "result": v }),
        Err(e) => json!({ "at": at, "error": e }),
    };
    let file = state_file();
    if let Some(dir) = file.parent()
        && let Err(e) = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&file, kept.to_string()))
    {
        tracing::warn!(file = %file.display(), error = %e, "can't keep the update check");
    }
    from_kept(&kept)
}

/// The last answer kept, if any.
pub fn last() -> Option<Check> {
    let text = std::fs::read_to_string(state_file()).ok()?;
    serde_json::from_str::<Value>(&text).ok().map(|v| from_kept(&v))
}

fn from_kept(v: &Value) -> Check {
    let at = v.get("at").and_then(Value::as_str).unwrap_or_default();
    match (v.get("result"), v.get("error").and_then(Value::as_str)) {
        (Some(r), _) => parse(at, r),
        (None, e) => Check { at: at.to_string(), error: Some(e.unwrap_or("no answer").to_string()), ..Check::default() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_by_name_and_record_only_pins() {
        let v = json!({
            "release": "05e5ccdcc736", "reboot": true, "probation": false,
            "gone": [], "left": [], "made": ["pkg:sola-oath"],
            "moves": [
                {"id": "pkg:oath", "move": "pin", "from": {"hash": "a", "version": "0.0.0"},
                 "to": {"hash": "b", "version": "0.0.0"}, "overlay": false, "services": [], "text": "moves"},
                {"id": "pkg:zlib", "move": "pin", "from": {"hash": "z"}, "to": {"hash": "z"},
                 "text": "(its category moves)"},
                {"id": "svc:x", "move": "pin", "from": {}, "to": {}}
            ]
        });
        let c = parse("t", &v);
        assert_eq!(c.newest.as_deref(), Some("05e5ccdcc736"));
        assert!(c.reboot);
        assert_eq!(c.made, ["sola-oath"]);
        assert_eq!(c.moves.len(), 2);
        assert!(c.moves["zlib"].record_only);
        assert_eq!(c.moving().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["oath"]);
    }

    #[test]
    fn heads_as_oath_prints_them() {
        // `move_list` in Oath's update.rs: `from` and `to` are hashes.
        let v = json!({
            "output": "97ae5e337869", "runs": true, "heads": 165, "refused": [],
            "moves": [
                {"id": "pkg:sola", "move": "head", "from": "sha256-a", "to": "sha256-b",
                 "overlay": true, "services": ["svc:sola-river"], "text": "moves to its head"},
                {"id": "pkg:mesa", "move": "hold", "from": "sha256-m", "to": "sha256-m", "text": "waits"},
                {"id": "pkg:cef", "move": "repoint", "from": "sha256-c", "to": null, "text": "needs move"}
            ]
        });
        let c = parse("t", &v);
        assert!(c.current);
        assert_eq!(c.moves["sola"].to_hash, "sha256-b");
        assert!(c.moves["sola"].is_head());
        assert!(c.moves["cef"].nothing_newer());
        assert!(!c.moves["mesa"].nothing_newer());
        assert_eq!(c.moving().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["sola"]);
    }

    #[test]
    fn a_current_machine() {
        let c = parse("t", &json!({"output": "f6011e8a6d9a", "runs": true}));
        assert!(c.current);
        assert_eq!(c.newest.as_deref(), Some("f6011e8a6d9a"));
        assert_eq!(c.moving().count(), 0);
    }

    #[test]
    fn a_refusal_is_kept_as_words() {
        let c = from_kept(&json!({"at": "t", "error": "unapplied changes"}));
        assert_eq!(c.error.as_deref(), Some("unapplied changes"));
    }
}
