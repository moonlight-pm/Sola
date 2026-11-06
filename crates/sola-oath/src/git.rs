//! Whether a build's commit is on its repository's origin. Oath records
//! the repo, commit and dirty bit of a build, not whether anyone else has
//! the commit, so the app asks a clone of that repo on this machine.
//! It reads the clone's remote branches as last fetched; it fetches
//! nothing.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq)]
pub enum Pushed {
    /// On a remote branch (the first that holds it, e.g. `origin/master`).
    Yes(String),
    /// In the clone, but on no remote branch.
    No,
    /// Built from uncommitted changes: the work is in no commit at all.
    Dirty,
    /// Can't tell, and why.
    Unknown(String),
}

/// Where clones live on this machine.
fn projects() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/home"), PathBuf::from).join("Projects")
}

/// `https://github.com/moonlight-pm/Sola` → `sola`.
fn repo_name(repo: &str) -> String {
    repo.trim_end_matches('/').trim_end_matches(".git").rsplit(['/', ':']).next().unwrap_or_default().to_lowercase()
}

fn same_repo(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let s = s.trim().trim_end_matches('/').trim_end_matches(".git").to_lowercase();
        let s = s.strip_prefix("git@").map(|r| r.replacen(':', "/", 1)).unwrap_or(s);
        s.trim_start_matches("https://").trim_start_matches("ssh://").to_string()
    };
    norm(a) == norm(b)
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A clone of `repo` under `~/Projects`, by its name and its remotes.
pub fn clone_of(repo: &str) -> Option<PathBuf> {
    let name = repo_name(repo);
    let entries = std::fs::read_dir(projects()).ok()?;
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|f| f.to_string_lossy().to_lowercase() == name))
        .find(|p| {
            git(p, &["remote", "-v"])
                .is_some_and(|r| r.split_whitespace().any(|url| same_repo(url, repo)))
        })
}

/// Is `commit` of `repo` on a remote branch?
pub fn pushed(repo: &str, commit: &str, dirty: bool) -> Pushed {
    if dirty {
        return Pushed::Dirty;
    }
    if repo.is_empty() || commit.is_empty() {
        return Pushed::Unknown("no repository recorded".into());
    }
    let Some(dir) = clone_of(repo) else {
        return Pushed::Unknown(format!("no clone of {} in ~/Projects", repo_name(repo)));
    };
    if git(&dir, &["cat-file", "-e", &format!("{commit}^{{commit}}")]).is_none() {
        return Pushed::Unknown(format!("{} does not have the commit; fetch it", dir.display()));
    }
    match git(&dir, &["branch", "-r", "--contains", commit, "--format=%(refname:short)"]) {
        Some(b) if !b.is_empty() => {
            let first = b.lines().find(|l| l.ends_with("/master") || l.ends_with("/main")).or_else(|| b.lines().next());
            Pushed::Yes(first.unwrap_or_default().to_string())
        }
        Some(_) => Pushed::No,
        None => Pushed::Unknown("git could not read the clone".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_remotes() {
        assert_eq!(repo_name("https://github.com/moonlight-pm/Sola"), "sola");
        assert_eq!(repo_name("git@github.com:moonlight-pm/oath.git"), "oath");
        assert!(same_repo("https://github.com/moonlight-pm/Sola.git", "https://github.com/moonlight-pm/sola"));
        assert!(same_repo("git@github.com:moonlight-pm/Sola.git", "https://github.com/moonlight-pm/Sola"));
        assert!(!same_repo("https://github.com/moonlight-pm/oath", "https://github.com/moonlight-pm/Sola"));
    }

    #[test]
    fn a_dirty_build_is_never_pushed() {
        assert_eq!(pushed("https://x/y", "abc", true), Pushed::Dirty);
    }
}
