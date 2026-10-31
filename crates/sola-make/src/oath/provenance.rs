//! Where a pinned package came from (the `oath-packages` skill;
//! `docs/specs/2026-10-31-oath-package-development-design.md`).
//!
//! A pin is a hash. Beside it this machine keeps who made it — repo,
//! commit, branch, dirty, machine, time — in
//! `~/.local/state/oath-dev/pins.json` (each entry with the hash it
//! describes; one whose hash is not the live one is stale), until `oath`
//! carries it on the `pkg` object. Never in the tree: that would move every
//! package on every commit.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Where a build came from.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Source {
    /// `origin`'s URL, else the checkout's path.
    pub repo: String,
    pub commit: String,
    pub branch: String,
    pub dirty: bool,
    pub built_by: String,
    pub built_at: String,
}

/// One pin and where it came from.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Pin {
    pub hash: String,
    #[serde(flatten)]
    pub source: Source,
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// This checkout.
pub fn here() -> Result<Source, String> {
    let commit = git(&["rev-parse", "HEAD"]).ok_or("not in a git checkout")?;
    let repo = git(&["remote", "get-url", "origin"])
        .or_else(|| git(&["rev-parse", "--show-toplevel"]))
        .unwrap_or_default();
    let branch = git(&["branch", "--show-current"]).unwrap_or_default();
    let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    let built_by = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|h| h.trim().to_string())
        .unwrap_or_default();
    let built_at = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    Ok(Source {
        repo,
        commit,
        branch,
        dirty,
        built_by,
        built_at,
    })
}

impl Source {
    /// `branch@abcdef12` (`+dirty`).
    pub fn short(&self) -> String {
        let c = &self.commit[..self.commit.len().min(8)];
        let b = if self.branch.is_empty() { "detached" } else { &self.branch };
        format!("{b}@{c}{}", if self.dirty { "+dirty" } else { "" })
    }
}

fn pins_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/state/oath-dev/pins.json")
}

fn read() -> BTreeMap<String, Pin> {
    std::fs::read_to_string(pins_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Where `id`'s live pin `live_hash` came from, if this machine knows.
pub fn live(id: &str, live_hash: &str) -> Option<Source> {
    read()
        .remove(id)
        .filter(|p| p.hash == live_hash)
        .map(|p| p.source)
}

/// Remember that `id` is pinned to `hash` from `source`.
pub fn record(pins: &[(String, String)], source: &Source) -> Result<(), String> {
    let mut all = read();
    for (id, hash) in pins {
        all.insert(
            id.clone(),
            Pin {
                hash: hash.clone(),
                source: source.clone(),
            },
        );
    }
    let path = pins_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// This checkout's history has `commit` (an unknown commit: no).
pub fn has_commit(commit: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", commit, "HEAD"])
        .status()
        .is_ok_and(|s| s.success())
}

/// Same repository: equal, or the same one by another URL form.
pub fn same_repo(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.trim_end_matches('/')
            .trim_end_matches(".git")
            .rsplit(['/', ':'])
            .take(2)
            .collect::<Vec<_>>()
            .join("/")
            .to_lowercase()
    };
    !a.is_empty() && norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_repo_by_any_url() {
        assert!(same_repo(
            "https://github.com/moonlight-pm/Sola.git",
            "git@github.com:moonlight-pm/Sola"
        ));
        assert!(!same_repo("https://github.com/moonlight-pm/Sola", "https://github.com/moonlight-pm/Oath"));
        assert!(!same_repo("", ""));
    }

    #[test]
    fn a_source_says_branch_and_commit() {
        let s = Source {
            commit: "ce6408de0123".into(),
            branch: "next".into(),
            dirty: true,
            ..Source::default()
        };
        assert_eq!(s.short(), "next@ce6408de+dirty");
    }

    #[test]
    fn this_checkout_has_its_own_head() {
        let head = git(&["rev-parse", "HEAD"]).unwrap();
        assert!(has_commit(&head));
        assert!(!has_commit("0000000000000000000000000000000000000000"));
    }
}
