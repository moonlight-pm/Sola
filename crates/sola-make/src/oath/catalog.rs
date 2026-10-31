//! The `oath` CLI calls `cargo make oath` makes. Reads run as the user;
//! writes (`store`, `set`, `apply`) run through `sudo`, which Oath's
//! desk user has without a password.

use std::path::Path;
use std::process::{Command, Output};

use super::provenance::Source;

/// True on an Oath machine (its catalog's INDEX is at `/oath/INDEX.md`).
pub fn on_oath() -> bool {
    Path::new("/oath/INDEX.md").is_file()
}

/// The hash `pkg:<name>` has linked on this machine.
pub fn live_hash(id: &str) -> Result<String, String> {
    let out = run(Command::new("oath").args(["--json", "get", id]))?;
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("oath get {id}: {e}"))?;
    v["actual"]["hash"]
        .as_str()
        .filter(|h| !h.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("{id} has no live hash (is it present?)"))
}

/// The packs `pkg:<name>` needs at run time, as linked here: `(id, hash)`.
pub fn live_needs(id: &str) -> Result<Vec<(String, String)>, String> {
    let out = run(Command::new("oath").args(["--json", "get", id]))?;
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("oath get {id}: {e}"))?;
    let needs = v["actual"]["needs"].as_array().cloned().unwrap_or_default();
    Ok(needs
        .iter()
        .filter_map(|n| Some((n["id"].as_str()?.to_string(), n["hash"].as_str()?.to_string())))
        .collect())
}

/// `oath fmt -w`: the plan file in its only legal form.
pub fn fmt(plan: &Path) -> Result<(), String> {
    run(Command::new("oath").args(["fmt", "-w"]).arg(plan)).map(drop)
}

/// File and pin `plan:<name>` from `plan`; returns the plan's hash.
pub fn set_plan(name: &str, plan: &Path) -> Result<String, String> {
    let out = run(Command::new("sudo")
        .args(["oath", "--json", "set", &format!("plan:{name}"), "--from-file"])
        .arg(plan))?;
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("oath set plan:{name}: {e}"))?;
    v["hash"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("oath set plan:{name}: no hash in output"))
}

/// `oath build plan:<name>` in its sandbox; returns the product's hash.
pub fn build(name: &str) -> Result<String, String> {
    let out = run(Command::new("sudo").args(["oath", "--json", "build", &format!("plan:{name}")]))?;
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("oath build plan:{name}: {e}"))?;
    v["product"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("oath build plan:{name}: no product in output"))
}

/// Put `dir` into the store as a realization of `id`; returns its hash.
pub fn store(id: &str, dir: &Path) -> Result<String, String> {
    let out = run(Command::new("sudo")
        .args(["oath", "--json", "store", id])
        .arg(dir))?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("oath store {id}: {e}"))?;
    v["hash"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("oath store {id}: no hash in output"))
}

/// Pin `id` to `hash` with `needs` (`(id, hash)` pairs) and where the tree
/// came from (`pkg.source`, Oath R47).
pub fn set(id: &str, hash: &str, needs: &[(String, String)], source: &Source) -> Result<(), String> {
    let pin = pin_json(hash, needs, source);
    run(Command::new("sudo").args(["oath", "set", id, "--from-json", &pin])).map(drop)
}

/// The catalog has `id` (a `pkg` this machine knows, present or not).
pub fn exists(id: &str) -> bool {
    Command::new("oath")
        .args(["--json", "get", id])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Make `pkg:<name>` pinned as [`set`] does (it runs at apply).
pub fn create(id: &str, hash: &str, needs: &[(String, String)], source: &Source) -> Result<(), String> {
    let pin = pin_json(hash, needs, source);
    run(Command::new("sudo").args(["oath", "new", id, "--from-json", &pin])).map(drop)
}

/// `oath apply` these ids together.
pub fn apply(ids: &[String]) -> Result<(), String> {
    run(Command::new("sudo").args(["oath", "apply"]).args(ids)).map(drop)
}

/// `{"hash", "needs", "source"}` for `--from-json`.
fn pin_json(hash: &str, needs: &[(String, String)], source: &Source) -> String {
    serde_json::json!({ "hash": hash, "needs": needs_json(needs), "source": source }).to_string()
}

/// `[{"id":…,"hash":…}]`, sorted by id as Oath stores needs.
fn needs_json(needs: &[(String, String)]) -> serde_json::Value {
    let mut sorted = needs.to_vec();
    sorted.sort();
    let list: Vec<serde_json::Value> = sorted
        .into_iter()
        .map(|(id, hash)| serde_json::json!({ "id": id, "hash": hash }))
        .collect();
    serde_json::Value::Array(list)
}

fn run(cmd: &mut Command) -> Result<Output, String> {
    let shown = format!("{cmd:?}");
    let out = cmd.output().map_err(|e| format!("{shown}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{shown} exited {}: {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_are_sorted_by_id() {
        let needs = [
            ("pkg:wayland".to_string(), "sha256-b".to_string()),
            ("pkg:cef".to_string(), "sha256-a".to_string()),
        ];
        let v = needs_json(&needs);
        assert_eq!(v[0]["id"], "pkg:cef");
        assert_eq!(v[0]["hash"], "sha256-a");
        assert_eq!(v[1]["id"], "pkg:wayland");
    }

    #[test]
    fn a_pin_carries_its_source() {
        let source = Source {
            repo: "https://github.com/moonlight-pm/Sola".into(),
            commit: "92815a651e523b28162f60b4daadd57ed00cb441".into(),
            branch: "master".into(),
            ..Source::default()
        };
        let v: serde_json::Value = serde_json::from_str(&pin_json("sha256-h", &[], &source)).unwrap();
        assert_eq!(v["hash"], "sha256-h");
        assert_eq!(v["needs"], serde_json::json!([]));
        assert_eq!(v["source"]["commit"], "92815a651e523b28162f60b4daadd57ed00cb441");
        assert_eq!(v["source"]["dirty"], false);
    }
}
