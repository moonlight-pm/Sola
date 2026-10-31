//! Where a pinned package came from (the `oath-packages` skill;
//! `docs/specs/2026-10-31-oath-package-development-design.md`).
//!
//! A pin is a hash. Who made it — repo, commit, branch, dirty, machine,
//! time — goes on the `pkg` object as `source` (Oath R47) with the pin.
//! Pins made before Oath had the field are in
//! `~/.local/state/oath-dev/pins.json` (each entry with the hash it
//! describes; one whose hash is not the live one is stale), read here and
//! no longer written. Never in the tree: that would move every package on
//! every commit.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Where a build came from: the shape of Oath's `pkg.source` (all six
/// keys; Oath decision with the Oath session, 2026-10-31).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Source {
    /// `https://host/owner/name`, no `.git` ([`plain_repo`]); else the
    /// checkout's path.
    pub repo: String,
    /// The full SHA.
    pub commit: String,
    /// `""` when detached.
    pub branch: String,
    /// Anything in the tree uncommitted.
    pub dirty: bool,
    /// `user@host`.
    pub built_by: String,
    /// RFC 3339, UTC.
    pub built_at: String,
}

/// A remote URL in the plain form: `git@github.com:o/n.git` and
/// `https://github.com/o/n.git` are both `https://github.com/o/n`.
pub fn plain_repo(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    if let Some(rest) = url.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            return format!("https://{host}/{path}");
        }
    }
    if let Some(rest) = url.strip_prefix("ssh://git@") {
        return format!("https://{rest}");
    }
    url.to_string()
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
        .map(|u| plain_repo(&u))
        .or_else(|| git(&["rev-parse", "--show-toplevel"]))
        .unwrap_or_default();
    let branch = git(&["branch", "--show-current"]).unwrap_or_default();
    let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|h| h.trim().to_string())
        .unwrap_or_default();
    let user = std::env::var("USER").unwrap_or_else(|_| "home".into());
    let built_by = format!("{user}@{host}");
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

/// Where `id`'s live pin `live_hash` came from, if this machine knows:
/// Oath's `source` on the pin, else what `pins.json` recorded before it.
pub fn live(id: &str, live_hash: &str) -> Option<Source> {
    oath_source(id, live_hash).or_else(|| {
        read()
            .remove(id)
            .filter(|p| p.hash == live_hash)
            .map(|p| p.source)
    })
}

fn oath_source(id: &str, live_hash: &str) -> Option<Source> {
    let out = Command::new("oath").args(["--json", "get", id]).output().ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    if v["desired"]["hash"] != live_hash {
        return None;
    }
    serde_json::from_value(v["desired"]["source"].clone()).ok()
}

/// This checkout's history has `commit` (an unknown commit: no).
pub fn has_commit(commit: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", commit, "HEAD"])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Same repository (both in, or brought to, the plain form).
pub fn same_repo(a: &str, b: &str) -> bool {
    !a.is_empty() && plain_repo(a).eq_ignore_ascii_case(&plain_repo(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repos_in_the_plain_form() {
        assert_eq!(plain_repo("git@github.com:moonlight-pm/Sola.git"), "https://github.com/moonlight-pm/Sola");
        assert_eq!(plain_repo("https://github.com/moonlight-pm/Sola.git"), "https://github.com/moonlight-pm/Sola");
        assert_eq!(plain_repo("https://github.com/moonlight-pm/Sola/"), "https://github.com/moonlight-pm/Sola");
    }

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
