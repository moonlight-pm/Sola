//! A new session's name, and what its folder is.
//!
//! Projects makes no worktrees: a session starts in its project's folder
//! and makes its own when its work needs one (sessions anywhere,
//! 2026-10-30).

use std::path::Path;

use crate::machine::Machine;

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

pub fn is_git_checkout(m: &Machine, path: &Path) -> bool {
    let Some(dir) = path.to_str() else {
        return false;
    };
    match m.run(None, &[], &["git", "-C", dir, "rev-parse", "--is-inside-work-tree"], None) {
        Ok(o) => o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true",
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_kebabs_and_rejects_empty() {
        assert_eq!(slug("KVM Perf"), "kvm-perf");
        assert_eq!(slug("  mail_kit  "), "mail-kit");
        assert_eq!(slug("..."), "");
        assert_eq!(slug("a"), "a");
    }

    #[test]
    fn a_plain_folder_is_not_a_checkout() {
        let dir = std::env::temp_dir().join(format!("sola-spawn-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_git_checkout(&Machine::Local, &dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
