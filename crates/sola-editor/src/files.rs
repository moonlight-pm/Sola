//! Files on disk: reading, the atomic write, what's changed under us.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Past this it is not a text file anyone means to edit.
const LARGEST_FILE: u64 = 256 * 1024 * 1024;

/// What a file looked like when we last read or wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

pub fn stamp(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    Some(Stamp { modified: meta.modified().ok(), len: meta.len() })
}

#[derive(Debug, Clone)]
pub struct Loaded {
    pub path: PathBuf,
    pub text: String,
    pub stamp: Option<Stamp>,
    /// Not UTF-8: shown as best we can, and saving would damage it.
    pub read_only: bool,
}

/// The file at `path`, or an empty new one if there is none yet.
pub fn load(path: &Path) -> Result<Loaded, String> {
    let name = path.display();
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(Loaded {
                path: path.to_path_buf(),
                text: String::new(),
                stamp: None,
                read_only: false,
            });
        }
        Err(e) => return Err(format!("{name}: {e}")),
    };
    if meta.is_dir() {
        return Err(format!("{name} is a folder"));
    }
    if meta.len() > LARGEST_FILE {
        return Err(format!("{name} is too large ({} MB)", meta.len() / (1024 * 1024)));
    }
    let bytes = fs::read(path).map_err(|e| format!("{name}: {e}"))?;
    let (text, read_only) = match String::from_utf8(bytes) {
        Ok(text) => (text, false),
        Err(e) => (String::from_utf8_lossy(e.as_bytes()).into_owned(), true),
    };
    Ok(Loaded {
        path: path.to_path_buf(),
        text,
        stamp: stamp(path),
        read_only: read_only || meta.permissions().readonly(),
    })
}

/// Write `text` to `path` so that a crash leaves the old file or the new
/// one, never half: a temporary beside it, renamed over. The file's mode
/// stays. Through a symlink, the link's target is what changes.
pub fn save(path: &Path, text: &str) -> io::Result<Stamp> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let old = fs::metadata(&target).ok();
    let temporary = temporary_beside(&target);
    let written = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        if let Some(meta) = &old {
            fs::set_permissions(&temporary, meta.permissions())?;
        }
        fs::rename(&temporary, &target)
    })();
    if let Err(e) = written {
        let _ = fs::remove_file(&temporary);
        // A folder we can't write in, a file we can: write it in place.
        if e.kind() == io::ErrorKind::PermissionDenied && old.is_some() {
            fs::write(&target, text)?;
        } else {
            return Err(e);
        }
    }
    stamp(&target).ok_or_else(|| io::Error::other("saved file has gone"))
}

fn temporary_beside(target: &Path) -> PathBuf {
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    target.with_file_name(format!(".{name}.sola-editor-{}", std::process::id()))
}

/// What a path on a command line names: absolute, and through any symlink
/// when it exists, so one file has one window.
pub fn absolute(path: &Path) -> PathBuf {
    fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sola-editor-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn a_missing_file_is_a_new_empty_one() {
        let d = dir("missing");
        let got = load(&d.join("nope.rs")).expect("loads");
        assert_eq!(got.text, "");
        assert!(got.stamp.is_none());
        assert!(!got.read_only);
    }

    #[test]
    fn a_folder_is_refused() {
        let d = dir("folder");
        assert!(load(&d).unwrap_err().contains("is a folder"));
    }

    #[test]
    fn not_utf8_opens_read_only() {
        let d = dir("bytes");
        let path = d.join("a.bin");
        fs::write(&path, [b'a', 0xFF, b'b']).expect("write");
        let got = load(&path).expect("loads");
        assert!(got.read_only);
        assert!(got.text.starts_with('a'));
    }

    #[test]
    fn save_replaces_the_file_and_keeps_its_mode() {
        let d = dir("save");
        let path = d.join("a.sh");
        fs::write(&path, "old").expect("write");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
        let saved = save(&path, "new").expect("saves");
        assert_eq!(fs::read_to_string(&path).expect("read"), "new");
        assert_eq!(fs::metadata(&path).expect("meta").permissions().mode() & 0o777, 0o755);
        assert_eq!(stamp(&path), Some(saved));
        // Nothing left behind.
        assert_eq!(fs::read_dir(&d).expect("list").count(), 1);
    }

    #[test]
    fn save_creates_a_new_file() {
        let d = dir("create");
        let path = d.join("new.txt");
        save(&path, "hi").expect("saves");
        assert_eq!(fs::read_to_string(&path).expect("read"), "hi");
    }

    #[test]
    fn save_through_a_symlink_changes_the_target() {
        let d = dir("link");
        let real = d.join("real.txt");
        let link = d.join("link.txt");
        fs::write(&real, "old").expect("write");
        symlink(&real, &link).expect("link");
        save(&link, "new").expect("saves");
        assert_eq!(fs::read_to_string(&real).expect("read"), "new");
        assert!(fs::symlink_metadata(&link).expect("meta").file_type().is_symlink());
    }

    #[test]
    fn a_failed_save_says_so_and_leaves_no_temporary() {
        let d = dir("fail");
        let path = d.join("no-such-folder").join("a.txt");
        assert!(save(&path, "x").is_err());
    }

    #[test]
    fn a_stamp_notices_a_change() {
        let d = dir("stamp");
        let path = d.join("a.txt");
        fs::write(&path, "one").expect("write");
        let first = stamp(&path);
        fs::write(&path, "three").expect("write");
        assert_ne!(first, stamp(&path));
    }

    #[test]
    fn absolute_resolves_relative_paths_and_new_files() {
        let d = dir("abs");
        assert!(absolute(Path::new("some-new-file.txt")).is_absolute());
        let real = d.join("real.txt");
        let link = d.join("link.txt");
        fs::write(&real, "x").expect("write");
        symlink(&real, &link).expect("link");
        assert_eq!(absolute(&link), fs::canonicalize(&real).expect("real"));
    }
}
