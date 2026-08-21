//! Create a sibling git worktree under `<project-root>/.worktrees/<slug>`.
//!
//! D4.2: the worktree base is always the project's `.worktrees` folder.
//! `git worktree remove` is not this module — drop is unregister + tmux.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Folder name under the project root. Locked (D4.2).
pub const WORKTREE_DIR: &str = ".worktrees";

pub fn worktree_base(root: &Path) -> PathBuf {
    root.join(WORKTREE_DIR)
}

pub fn worktree_path(root: &Path, slug: &str) -> PathBuf {
    worktree_base(root).join(slug)
}

/// Lowercase kebab. Empty if the name has no letter or digit.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

pub fn is_git_checkout(path: &Path) -> bool {
    let Some(dir) = path.to_str() else {
        return false;
    };
    let output = Command::new("git")
        .args(["-C", dir, "rev-parse", "--is-inside-work-tree"])
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim() == "true",
        _ => false,
    }
}

/// Append `.worktrees/` to `.gitignore` when git does not already ignore it.
pub fn ensure_worktrees_ignored(root: &Path) -> Result<(), String> {
    if git_ignores(root, WORKTREE_DIR) {
        return Ok(());
    }
    let gi = root.join(".gitignore");
    let mut text = fs::read_to_string(&gi).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("/.worktrees/\n");
    fs::write(&gi, text).map_err(|e| format!("write .gitignore: {e}"))
}

fn git_ignores(root: &Path, path: &str) -> bool {
    let Some(dir) = root.to_str() else {
        return false;
    };
    Command::new("git")
        .args(["-C", dir, "check-ignore", "-q", path])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `git worktree add` at `<root>/.worktrees/<slug>`.
///
/// `branch` is the git branch (default: `slug`). `base` is the start-point
/// (default: HEAD). Does not start a pane.
pub fn add_worktree(root: &Path, slug: &str) -> Result<PathBuf, String> {
    add_worktree_at(root, slug, None, None)
}

pub fn add_worktree_at(
    root: &Path,
    slug: &str,
    branch: Option<&str>,
    base: Option<&str>,
) -> Result<PathBuf, String> {
    if slug.is_empty() {
        return Err("name needs a letter or number".into());
    }
    if slug.contains('/') || slug.contains('\0') || slug == "." || slug == ".." {
        return Err("name is not a safe folder".into());
    }
    if !is_git_checkout(root) {
        return Err("project root is not a git checkout".into());
    }
    let branch = branch
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(slug);
    if !valid_branch(branch) {
        return Err(format!("branch name is not safe ({branch})"));
    }
    if let Some(start) = base.map(str::trim).filter(|s| !s.is_empty()) {
        if !rev_exists(root, start) {
            return Err(format!(
                "unknown start-point '{start}' (fetch remotes if this is origin/…)"
            ));
        }
    }
    let dest = worktree_path(root, slug);
    if dest.exists() {
        return Err(format!("{} already exists", dest.display()));
    }
    fs::create_dir_all(worktree_base(root)).map_err(|e| format!("create .worktrees: {e}"))?;
    ensure_worktrees_ignored(root)?;

    let dest_s = dest
        .to_str()
        .ok_or_else(|| "worktree path is not utf-8".to_string())?;
    let start = base.map(str::trim).filter(|s| !s.is_empty());

    let mut create: Vec<&str> = vec!["worktree", "add", "-b", branch, dest_s];
    if let Some(s) = start {
        create.push(s);
    }
    if git_ok(root, &create) {
        return Ok(dest);
    }
    if start.is_none() && git_ok(root, &["worktree", "add", dest_s, branch]) {
        return Ok(dest);
    }
    let _ = git_ok(root, &["worktree", "prune"]);
    if git_ok(root, &create) {
        return Ok(dest);
    }
    if start.is_none() && git_ok(root, &["worktree", "add", dest_s, branch]) {
        return Ok(dest);
    }
    let err = git_stderr(root, &create);
    Err(if err.is_empty() {
        "git worktree add failed".into()
    } else {
        err
    })
}

fn valid_branch(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.ends_with(".lock")
        && !name.contains('\0')
        && !name.contains("..")
        && !name.contains(" ")
}

fn rev_exists(root: &Path, rev: &str) -> bool {
    git_ok(
        root,
        &["rev-parse", "--verify", &format!("{rev}^{{commit}}")],
    )
}

fn git_ok(root: &Path, args: &[&str]) -> bool {
    git_output(root, args).0
}

fn git_stderr(root: &Path, args: &[&str]) -> String {
    git_output(root, args).1
}

fn git_output(root: &Path, args: &[&str]) -> (bool, String) {
    let Some(dir) = root.to_str() else {
        return (false, "path is not utf-8".into());
    };
    match Command::new("git").args(["-C", dir]).args(args).output() {
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
            (o.status.success(), err)
        }
        Err(e) => (false, format!("git: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn temp_git() -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ws-spawn-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        run(&dir, &["git", "init", "-q"]);
        run(&dir, &["git", "config", "user.email", "t@t"]);
        run(&dir, &["git", "config", "user.name", "t"]);
        fs::write(dir.join("README"), "x").unwrap();
        run(&dir, &["git", "add", "README"]);
        run(&dir, &["git", "commit", "-q", "-m", "init"]);
        dir
    }

    fn run(dir: &Path, argv: &[&str]) {
        let st = Command::new(argv[0])
            .args(&argv[1..])
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(st.success(), "{argv:?} failed in {}", dir.display());
    }

    #[test]
    fn slug_kebabs_and_rejects_empty() {
        assert_eq!(slug("KVM Perf"), "kvm-perf");
        assert_eq!(slug("  mail_kit  "), "mail-kit");
        assert_eq!(slug("..."), "");
        assert_eq!(slug("a"), "a");
    }

    #[test]
    fn worktree_lands_under_dot_worktrees() {
        let root = PathBuf::from("/tmp/sola");
        assert_eq!(
            worktree_path(&root, "kvm-perf"),
            PathBuf::from("/tmp/sola/.worktrees/kvm-perf")
        );
    }

    #[test]
    fn add_worktree_creates_checkout() {
        let root = temp_git();
        let dest = add_worktree(&root, "kvm-perf").expect("add");
        assert_eq!(dest, root.join(".worktrees/kvm-perf"));
        assert!(dest.join("README").exists());
        assert!(dest.join(".git").exists() || dest.join(".git").is_file());
        assert!(git_ignores(&root, WORKTREE_DIR));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn add_worktree_rejects_duplicate() {
        let root = temp_git();
        add_worktree(&root, "dup").unwrap();
        let err = add_worktree(&root, "dup").unwrap_err();
        assert!(err.contains("already exists"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn add_worktree_branch_and_base() {
        let root = temp_git();
        fs::write(root.join("README"), "v2").unwrap();
        run(&root, &["git", "add", "README"]);
        run(&root, &["git", "commit", "-q", "-m", "second"]);
        let first = String::from_utf8(
            Command::new("git")
                .args(["-C", root.to_str().unwrap(), "rev-parse", "HEAD~1"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let first = first.trim().to_string();
        let dest = add_worktree_at(
            &root,
            "sc-1234",
            Some("joshua/sc-1234/fix-login"),
            Some(&first),
        )
        .expect("add at base");
        assert_eq!(dest, root.join(".worktrees/sc-1234"));
        assert_eq!(fs::read_to_string(dest.join("README")).unwrap(), "x");
        let head = String::from_utf8(
            Command::new("git")
                .args([
                    "-C",
                    dest.to_str().unwrap(),
                    "rev-parse",
                    "--abbrev-ref",
                    "HEAD",
                ])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert_eq!(head.trim(), "joshua/sc-1234/fix-login");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn add_worktree_unknown_base_errors() {
        let root = temp_git();
        let err = add_worktree_at(&root, "x", None, Some("origin/nope")).unwrap_err();
        assert!(err.contains("unknown start-point"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn add_worktree_requires_git() {
        let dir = std::env::temp_dir().join(format!(
            "ws-nongit-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let err = add_worktree(&dir, "x").unwrap_err();
        assert!(err.contains("not a git"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }
}
