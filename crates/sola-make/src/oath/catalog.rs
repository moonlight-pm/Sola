//! The `oath` CLI calls `cargo make oath` makes. Reads run as the user;
//! writes (`store`, `set`, `apply`) run through `sudo`, which Oath's
//! desk user has without a password.

use std::path::Path;
use std::process::{Command, Output};

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

/// Pin `id` to `hash` with `needs` (`(id, hash)` pairs).
pub fn set(id: &str, hash: &str, needs: &[(String, String)]) -> Result<(), String> {
    run(Command::new("sudo").args([
        "oath",
        "set",
        id,
        &format!("hash={hash}"),
        &format!("needs={}", needs_json(needs)),
    ]))
    .map(drop)
}

/// `oath apply` these ids together.
pub fn apply(ids: &[String]) -> Result<(), String> {
    run(Command::new("sudo").args(["oath", "apply"]).args(ids)).map(drop)
}

/// `[{"id":…,"hash":…}]`, sorted by id as Oath stores needs.
fn needs_json(needs: &[(String, String)]) -> String {
    let mut sorted = needs.to_vec();
    sorted.sort();
    let list: Vec<serde_json::Value> = sorted
        .into_iter()
        .map(|(id, hash)| serde_json::json!({ "id": id, "hash": hash }))
        .collect();
    serde_json::Value::Array(list).to_string()
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
        let v: serde_json::Value = serde_json::from_str(&needs_json(&needs)).unwrap();
        assert_eq!(v[0]["id"], "pkg:cef");
        assert_eq!(v[0]["hash"], "sha256-a");
        assert_eq!(v[1]["id"], "pkg:wayland");
    }
}
