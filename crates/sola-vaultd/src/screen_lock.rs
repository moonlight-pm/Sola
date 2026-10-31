//! Is the screen locked? Sola has no bus signal for it. `sola-lock`, while
//! it holds the session lock, holds an exclusive `flock` on
//! `$XDG_RUNTIME_DIR/sola-lock.lock` (its one-instance guard); the kernel
//! lists flocks in `/proc/locks` by device and inode. Reading that list
//! never touches the lock itself, so it cannot get in sola-lock's way.

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
    held(&locks, meta.dev(), meta.ino())
}

/// A FLOCK in `locks` (the `/proc/locks` text) on the file at `dev`/`ino`.
/// Waiters (`->`) do not hold it.
fn held(locks: &str, dev: u64, ino: u64) -> bool {
    let want = format!("{:02x}:{:02x}:{ino}", libc::major(dev), libc::minor(dev));
    locks.lines().any(|line| {
        let f: Vec<&str> = line.split_whitespace().collect();
        f.get(1) == Some(&"FLOCK") && f.get(5) == Some(&want.as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCKS: &str = "\
1: POSIX  ADVISORY  WRITE 812 00:1a:4242 0 EOF
2: FLOCK  ADVISORY  WRITE 2153 00:2a:777 0 EOF
2: -> FLOCK  ADVISORY  WRITE 2160 00:2a:778 0 EOF
";

    #[test]
    fn a_held_flock_on_the_file_reads_locked() {
        let dev = libc::makedev(0, 0x2a);
        assert!(held(LOCKS, dev, 777));
        // A waiter, a POSIX lock, another file: not held.
        assert!(!held(LOCKS, dev, 778));
        assert!(!held(LOCKS, libc::makedev(0, 0x1a), 4242));
        assert!(!held(LOCKS, dev, 1));
        assert!(!held("", dev, 777));
    }
}
