//! Which compositor the apps run on. Its Wayland socket, by inode and
//! change time, is the compositor's identity: a new compositor makes a new
//! socket. An app that exits after its compositor went (it restarted under
//! the running desk, say an Oath apply that moved rivulet) did not close: it
//! lost its display, and the session keeps it and brings it back.

use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use sola_core::env;

/// The live compositor's identity; `None` when no compositor answers (gone,
/// or a dead one's socket left behind).
pub fn live() -> Option<String> {
    let name = env::child_wayland_display()?;
    let path = if name.starts_with('/') {
        PathBuf::from(&name)
    } else {
        env::runtime_dir().join(&name)
    };
    UnixStream::connect(&path).ok()?;
    let m = std::fs::metadata(&path).ok()?;
    Some(identity(m.ino(), m.ctime(), m.ctime_nsec()))
}

fn identity(ino: u64, ctime: i64, ctime_nsec: i64) -> String {
    format!("{ino}-{ctime}.{ctime_nsec:09}")
}

/// An app launched on `launched_on` just exited while `now` is the live
/// compositor: did it go with its display (not close)?
pub fn went_with_display(launched_on: Option<&str>, now: Option<&str>) -> bool {
    match launched_on {
        Some(then) => now != Some(then),
        // Launched with no compositor to tell by: take the exit as a close.
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exit_counts_as_lost_only_when_its_compositor_changed() {
        let a = identity(10, 100, 5);
        let b = identity(11, 200, 7);
        assert!(!went_with_display(Some(&a), Some(&a)), "same compositor: the app closed");
        assert!(went_with_display(Some(&a), Some(&b)), "a new compositor");
        assert!(went_with_display(Some(&a), None), "no compositor answering");
        assert!(!went_with_display(None, Some(&b)), "unknown at launch: a close");
    }
}
