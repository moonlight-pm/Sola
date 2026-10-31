//! `oath --json` client. The GUI must not write `/oath/objects`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::Value;

/// Catalog object the Packages panel shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pkg {
    pub id: String,
    pub desired_present: bool,
    pub actual_present: bool,
    pub removable: bool,
    pub hash: String,
    pub links: Vec<String>,
    pub status: String,
    pub needs: Vec<Need>,
    pub description: String,
    pub home: String,
    pub components: Vec<Component>,
    pub advisories: Option<Vec<Advisory>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    pub name: String,
    pub version: String,
    pub purl: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Advisory {
    pub id: String,
    pub url: String,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Need {
    pub id: String,
    pub hash: String,
}

/// Case-insensitive match on id, `/bin` links, and `needs`.
pub fn matches_query(pkg: &Pkg, query: &str) -> bool {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return true;
    }
    if pkg.id.to_ascii_lowercase().contains(&q) {
        return true;
    }
    if pkg
        .links
        .iter()
        .any(|l| l.to_ascii_lowercase().contains(&q))
    {
        return true;
    }
    if pkg
        .needs
        .iter()
        .any(|n| n.id.to_ascii_lowercase().contains(&q) || n.hash.to_ascii_lowercase().contains(&q))
    {
        return true;
    }
    if pkg.description.to_ascii_lowercase().contains(&q) {
        return true;
    }
    if pkg.components.iter().any(|c| {
        c.name.to_ascii_lowercase().contains(&q)
            || c.version.to_ascii_lowercase().contains(&q)
            || c.purl.to_ascii_lowercase().contains(&q)
    }) {
        return true;
    }
    pkg.advisories.as_ref().is_some_and(|a| {
        a.iter().any(|x| {
            x.id.to_ascii_lowercase().contains(&q) || x.summary.to_ascii_lowercase().contains(&q)
        })
    })
}

fn json_hash(v: &Value) -> Option<String> {
    if let Some(h) = v
        .get("hash")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(h.to_string());
    }
    v.get("realizations")
        .and_then(|r| r.as_array())
        .and_then(|a| {
            a.iter().find_map(|x| {
                x.get("hash")
                    .and_then(|h| h.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
            })
        })
}

fn parse_need(v: &Value) -> Option<Need> {
    if let Some(s) = v.as_str() {
        return Some(Need {
            id: normalize_need(s),
            hash: String::new(),
        });
    }
    let id = v.get("id").and_then(|x| x.as_str())?;
    let hash = v
        .get("hash")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    Some(Need {
        id: normalize_need(id),
        hash,
    })
}

pub fn short_name(id: &str) -> &str {
    id.strip_prefix("pkg:").unwrap_or(id)
}

pub fn normalize_need(w: &str) -> String {
    if w.contains(':') {
        w.to_string()
    } else {
        format!("pkg:{w}")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogError {
    pub message: String,
    pub hint: Option<String>,
    pub confirm: bool,
}

impl CatalogError {
    fn msg(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            hint: None,
            confirm: false,
        }
    }
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.hint {
            Some(h) => write!(f, "{}\n{h}", self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

pub fn oath_bin() -> PathBuf {
    if let Some(p) = std::env::var_os("OATH_BIN") {
        return PathBuf::from(p);
    }
    PathBuf::from("/bin/oath")
}

pub fn sudo_bin() -> PathBuf {
    PathBuf::from("/bin/sudo")
}

/// First catalog JSON value in `stdout`, ignoring `oath-tel` / btrfs chatter.
pub fn extract_json(stdout: &str) -> Result<Value, CatalogError> {
    let filtered: String = stdout
        .lines()
        .filter(|l| !l.trim_start().starts_with("oath-tel"))
        .collect::<Vec<_>>()
        .join("\n");
    let s = filtered.trim();
    let mut rest = s;
    while let Some(start) = rest.find(['{', '[']) {
        rest = &rest[start..];
        let mut de = serde_json::Deserializer::from_str(rest);
        match Value::deserialize(&mut de) {
            Ok(v) => {
                if is_catalog_json(&v) {
                    return Ok(v);
                }
                rest = &rest[1..];
            }
            Err(e) => return Err(CatalogError::msg(format!("oath --json: {e}"))),
        }
    }
    Err(CatalogError::msg("oath --json produced no JSON"))
}

fn is_catalog_json(v: &Value) -> bool {
    if v.is_array() {
        return true;
    }
    let Some(o) = v.as_object() else {
        return false;
    };
    o.contains_key("id") || o.contains_key("generation") || o.contains_key("present")
}

pub fn parse_ls_pkg(v: &Value) -> Result<Vec<String>, CatalogError> {
    let arr = v
        .as_array()
        .ok_or_else(|| CatalogError::msg("oath ls: expected a JSON array"))?;
    Ok(arr
        .iter()
        .filter_map(|x| x.as_str())
        .filter(|s| s.starts_with("pkg:"))
        .map(|s| s.to_string())
        .collect())
}

pub fn parse_pkg_get(v: &Value) -> Result<Pkg, CatalogError> {
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(error_from_json(v, err));
    }
    let id = v
        .get("id")
        .and_then(|x| x.as_str())
        .ok_or_else(|| CatalogError::msg("oath get: missing id"))?
        .to_string();
    let desired = v.get("desired").cloned().unwrap_or(Value::Null);
    let actual = v.get("actual").cloned().unwrap_or(Value::Null);
    let meta = v.get("meta").cloned().unwrap_or(Value::Null);
    let desired_present = desired
        .get("present")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let actual_present = actual
        .get("present")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let removable = actual
        .get("removable")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let hash = json_hash(&actual)
        .or_else(|| json_hash(&desired))
        .unwrap_or_default();
    let links = actual
        .get("links")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let status = meta
        .get("status")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let needs = desired
        .get("needs")
        .or_else(|| actual.get("needs"))
        .and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(parse_need).collect())
        .unwrap_or_default();
    let description = desired
        .get("description")
        .or_else(|| actual.get("description"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let home = desired
        .get("home")
        .or_else(|| actual.get("home"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let components = actual
        .get("components")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    let name = e.get("name").and_then(|x| x.as_str())?;
                    let version = e.get("version").and_then(|x| x.as_str())?;
                    let purl = e.get("purl").and_then(|x| x.as_str()).unwrap_or("");
                    Some(Component {
                        name: name.to_string(),
                        version: version.to_string(),
                        purl: purl.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let advisories = actual
        .get("advisories")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    let id = e.get("id").and_then(|x| x.as_str())?;
                    Some(Advisory {
                        id: id.to_string(),
                        url: e
                            .get("url")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        summary: e
                            .get("summary")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        });
    Ok(Pkg {
        id,
        desired_present,
        actual_present,
        removable,
        hash,
        links,
        status,
        needs,
        description,
        home,
        components,
        advisories,
    })
}

fn error_from_json(v: &Value, err: &str) -> CatalogError {
    CatalogError {
        message: err.to_string(),
        hint: v
            .get("hint")
            .and_then(|h| h.as_str())
            .map(|s| s.to_string()),
        confirm: false,
    }
}

fn run(sudo: bool, args: &[&str]) -> Result<(i32, String, String), CatalogError> {
    let bin = oath_bin();
    if !sudo && !Path::new(&bin).is_file() {
        return Err(CatalogError::msg(format!("{} not found", bin.display())));
    }
    let mut cmd = if sudo {
        let mut c = Command::new(sudo_bin());
        c.arg(&bin);
        c
    } else {
        Command::new(&bin)
    };
    cmd.arg("--json").args(args).stdin(Stdio::null());
    let out = cmd
        .output()
        .map_err(|e| CatalogError::msg(format!("oath: {e}")))?;
    let code = out.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    Ok((code, stdout, stderr))
}

fn run_json(sudo: bool, args: &[&str]) -> Result<Value, CatalogError> {
    let (code, stdout, stderr) = run(sudo, args)?;
    if !stdout.trim().is_empty() {
        if let Ok(v) = extract_json(&stdout) {
            if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
                let mut e = error_from_json(&v, err);
                e.confirm = code == 3;
                return Err(e);
            }
            if code == 0 {
                return Ok(v);
            }
            let mut e = CatalogError::msg(format!("oath exited {code}"));
            e.hint = v
                .get("hint")
                .and_then(|h| h.as_str())
                .map(|s| s.to_string());
            e.confirm = code == 3;
            return Err(e);
        }
    }
    if code == 3 {
        return Err(CatalogError {
            message: "apply needs --confirm".into(),
            hint: Some("pass --confirm only if the owner asked. See /oath/INDEX.md safety.".into()),
            confirm: true,
        });
    }
    if code != 0 {
        let tail = stderr
            .lines()
            .rev()
            .find(|l| !l.starts_with("oath-tel"))
            .unwrap_or("oath failed");
        return Err(CatalogError::msg(format!("oath exited {code}: {tail}")));
    }
    extract_json(&stdout)
}

pub fn list_packages() -> Result<Vec<Pkg>, CatalogError> {
    let v = run_json(false, &["ls", "--kind", "pkg"])?;
    let ids = parse_ls_pkg(&v)?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let got = run_json(false, &["get", &id])?;
        out.push(parse_pkg_get(&got)?);
    }
    Ok(out)
}

pub fn apply_present(changes: &[(String, bool)]) -> Result<u64, CatalogError> {
    if changes.is_empty() {
        return Err(CatalogError::msg("nothing to apply"));
    }
    let mut ids: Vec<&str> = Vec::new();
    for (id, present) in changes {
        let val = if *present {
            "present=true"
        } else {
            "present=false"
        };
        run_json(true, &["set", id, val])?;
        ids.push(id.as_str());
    }
    let mut args: Vec<&str> = vec!["apply"];
    args.extend(ids.iter().copied());
    let v = run_json(true, &args)?;
    Ok(v.get("generation").and_then(|g| g.as_u64()).unwrap_or(0))
}

pub fn undo() -> Result<u64, CatalogError> {
    let v = run_json(true, &["undo"])?;
    Ok(v.get("generation").and_then(|g| g.as_u64()).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_skips_chatter() {
        let stdout = "Create readonly snapshot of '/oath/run/fs/@' in '/oath/run/fs/@gen-59'\n{\n  \"generation\": 59,\n  \"ids\": [\"pkg:hello\"]\n}\n";
        let v = extract_json(stdout).unwrap();
        assert_eq!(v["generation"], 59);
        assert_eq!(v["ids"][0], "pkg:hello");
    }

    #[test]
    fn extract_skips_tel_object() {
        let stdout = "oath-tel {\"event\":\"cmd\",\"verb\":\"get\"}\n{\"id\":\"pkg:git\",\"desired\":{\"present\":true},\"actual\":{\"present\":true,\"hash\":\"sha256-abcd\"}}\n";
        let v = extract_json(stdout).unwrap();
        assert_eq!(v["id"], "pkg:git");
        assert_eq!(v["actual"]["hash"], "sha256-abcd");
    }

    #[test]
    fn parse_ls_filters_pkg() {
        let v = json!(["pkg:hello", "svc:sshd", "pkg:busybox", "host:local"]);
        assert_eq!(parse_ls_pkg(&v).unwrap(), ["pkg:hello", "pkg:busybox"]);
    }

    #[test]
    fn parse_get_pkg() {
        let v = json!({
            "id": "pkg:hello",
            "meta": { "safety": "mutate", "status": "in-sync" },
            "desired": { "present": true, "hash": "sha256-abc" },
            "actual": {
                "present": true,
                "hash": "sha256-abc",
                "links": ["hello"],
                "removable": true
            }
        });
        let p = parse_pkg_get(&v).unwrap();
        assert_eq!(p.id, "pkg:hello");
        assert!(p.desired_present && p.actual_present && p.removable);
        assert_eq!(p.links, ["hello"]);
        assert!(p.needs.is_empty());
        assert_eq!(p.hash, "sha256-abc");
    }

    #[test]
    fn parse_needs() {
        let v = json!({
            "id": "pkg:steam",
            "meta": { "status": "in-sync" },
            "desired": { "present": true, "needs": [
                {"id": "pkg:mesa", "hash": "sha256-aa"},
                {"id": "glibc", "hash": "sha256-bb"}
            ] },
            "actual": { "present": true, "removable": true, "links": ["steam"] }
        });
        let p = parse_pkg_get(&v).unwrap();
        assert_eq!(p.needs[0].id, "pkg:mesa");
        assert_eq!(p.needs[1].id, "pkg:glibc");
        assert_eq!(p.needs[0].hash, "sha256-aa");
    }

    #[test]
    fn query_matches_id_links_needs() {
        let p = Pkg {
            id: "pkg:steam".into(),
            desired_present: true,
            actual_present: true,
            removable: true,
            hash: String::new(),
            links: vec!["steam".into()],
            status: String::new(),
            needs: vec![Need {
                id: "pkg:mesa".into(),
                hash: String::new(),
            }],
            description: "Valve Steam launcher.".into(),
            home: "https://store.steampowered.com/".into(),
            components: Vec::new(),
            advisories: None,
        };
        assert!(matches_query(&p, ""));
        assert!(matches_query(&p, "STEAM"));
        assert!(matches_query(&p, "mesa"));
        assert!(matches_query(&p, "launcher"));
        assert!(!matches_query(&p, "hyprland"));
    }

    #[test]
    fn parse_components_and_advisories() {
        let v = json!({
            "id": "pkg:curl",
            "meta": { "status": "in-sync" },
            "desired": { "present": true },
            "actual": {
                "present": true,
                "removable": true,
                "components": [
                    {"name": "curl", "version": "8.11.1", "purl": "pkg:generic/curl@8.11.1"}
                ],
                "advisories": [
                    {"id": "CVE-TEST-1", "url": "https://example.invalid/CVE-TEST-1"}
                ]
            }
        });
        let p = parse_pkg_get(&v).unwrap();
        assert_eq!(p.components[0].name, "curl");
        assert_eq!(p.components[0].version, "8.11.1");
        assert_eq!(p.advisories.as_ref().unwrap()[0].id, "CVE-TEST-1");
        assert!(matches_query(&p, "8.11"));
        assert!(matches_query(&p, "CVE-TEST"));
    }

    #[test]
    fn locked_pack_not_removable() {
        let v = json!({
            "id": "pkg:busybox",
            "meta": { "status": "in-sync" },
            "desired": { "present": true },
            "actual": { "present": true, "removable": false, "links": ["sh"] }
        });
        let p = parse_pkg_get(&v).unwrap();
        assert!(!p.removable);
    }

    #[test]
    fn json_error_carries_hint() {
        let v = json!({ "error": "svc:river and svc:hyprland cannot both be enabled (one compositor)", "hint": "oath set host:local session=sola|omarchy" });
        let e = parse_pkg_get(&v).unwrap_err();
        assert!(e.message.contains("one compositor"));
        assert_eq!(
            e.hint.as_deref(),
            Some("oath set host:local session=sola|omarchy")
        );
    }
}
