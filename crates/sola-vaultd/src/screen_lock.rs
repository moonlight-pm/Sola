//! Is the screen locked? Sola has no bus signal for it. `sola-lock`, while
//! it holds the session lock, holds an exclusive `flock` on
//! `$XDG_RUNTIME_DIR/sola-lock.lock` (its one-instance guard); the kernel
//! lists flocks in `/proc/locks` with the holder's pid and the file's
//! inode. Reading that list never touches the lock itself, so it cannot
//! get in sola-lock's way (taking even a shared lock could make a starting
//! sola-lock think another one holds the screen, and leave it unlocked).
//!
//! The device column is the filesystem's own, which differs from what
//! `stat` says on btrfs; so a lock counts when it is on the file's inode
//! and its holder is `sola-lock`.

use std::os::unix::fs::MetadataExt;

/// `sola-lock` holds the screen now.
pub fn locked() -> bool {
    let path = sola_core::env::runtime_dir().join("sola-lock.lock");
    let Ok(meta) = std::fs::metadata(&path) else {
        return false;
    };
    let Ok(locks) = std::fs::read_to_string("/proc/locks") else {
        return false;
    };
    held(&locks, meta.ino(), |pid| {
        std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == "sola-lock")
    })
}

/// A FLOCK in `locks` (the `/proc/locks` text) on inode `ino`, held by a
/// process `is_sola_lock` accepts. Waiters (`->`) do not hold it.
fn held(locks: &str, ino: u64, is_sola_lock: impl Fn(u32) -> bool) -> bool {
    let suffix = format!(":{ino}");
    locks.lines().any(|line| {
        let f: Vec<&str> = line.split_whitespace().collect();
        f.get(1) == Some(&"FLOCK")
            && f.get(5).is_some_and(|dev_ino| dev_ino.ends_with(&suffix))
            && f.get(4).and_then(|p| p.parse().ok()).is_some_and(&is_sola_lock)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCKS: &str = "\
1: POSIX  ADVISORY  WRITE 812 00:1a:4242 0 EOF
2: FLOCK  ADVISORY  WRITE 2153 00:2a:777 0 EOF
2: -> FLOCK  ADVISORY  WRITE 2160 00:2a:778 0 EOF
3: FLOCK  ADVISORY  WRITE 4000 00:23:1777 0 EOF
";

    #[test]
    fn a_flock_held_by_sola_lock_on_the_file_reads_locked() {
        let sola_lock = |pid| pid == 2153 || pid == 2160;
        assert!(held(LOCKS, 777, sola_lock));
        // A waiter, a POSIX lock, another inode, another holder: not held.
        assert!(!held(LOCKS, 778, sola_lock));
        assert!(!held(LOCKS, 4242, |_| true));
        assert!(!held(LOCKS, 77, sola_lock));
        assert!(!held(LOCKS, 1777, sola_lock));
        assert!(!held(LOCKS, 777, |_| false));
        assert!(!held("", 777, sola_lock));
    }
}
