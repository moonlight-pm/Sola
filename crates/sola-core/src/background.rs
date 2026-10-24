//! The desktop's background image: Sola keeps its own copy under
//! `~/.config/sola/background/`, so the picture stays when the original
//! moves or goes. [`adopt`] copies a file there (or [`adopt_bytes`] writes
//! one), drops the previous picture, and returns the copy's path for
//! `Topic::Background`. sola-river draws it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Where the picture lives.
pub fn dir() -> PathBuf {
    crate::config::sola_config_dir().join("background")
}

/// Copy `src` into [`dir`] and make it the only picture there. A file
/// already there is kept as is.
pub fn adopt(src: &Path) -> io::Result<PathBuf> {
    adopt_into(&dir(), src)
}

/// Write `bytes` into [`dir`] as `file_name` (an edited image, encoded by
/// the caller) and make it the only picture there.
pub fn adopt_bytes(file_name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    adopt_bytes_into(&dir(), file_name, bytes)
}

fn adopt_into(dir: &Path, src: &Path) -> io::Result<PathBuf> {
    let src = fs::canonicalize(src)?;
    let name = src
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a file"))?
        .to_string_lossy()
        .into_owned();
    fs::create_dir_all(dir)?;
    let dest = dir.join(&name);
    if src.parent() != Some(fs::canonicalize(dir)?.as_path()) {
        let tmp = dir.join(format!(".{name}.tmp"));
        fs::copy(&src, &tmp)?;
        fs::rename(&tmp, &dest)?;
    }
    prune(dir, &dest);
    Ok(dest)
}

fn adopt_bytes_into(dir: &Path, file_name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let name = Path::new(file_name)
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?
        .to_string_lossy()
        .into_owned();
    fs::create_dir_all(dir)?;
    let dest = dir.join(&name);
    let tmp = dir.join(format!(".{name}.tmp"));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, &dest)?;
    prune(dir, &dest);
    Ok(dest)
}

/// Remove every other file in `dir` (the previous pictures).
fn prune(dir: &Path, keep: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path != keep && path.is_file() {
            if let Err(e) = fs::remove_file(&path) {
                tracing::warn!(path = %path.display(), %e, "remove old background");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let marker = 0u8;
        let dir = std::env::temp_dir().join(format!(
            "sola-background-test-{}-{tag}-{:p}",
            std::process::id(),
            &marker
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn adopting_copies_and_drops_the_previous_picture() {
        let root = scratch("adopt");
        let store = root.join("background");
        let a = root.join("a.jpg");
        let b = root.join("b.png");
        fs::write(&a, b"one").unwrap();
        fs::write(&b, b"two").unwrap();

        let first = adopt_into(&store, &a).unwrap();
        assert_eq!(first, store.join("a.jpg"));
        assert_eq!(fs::read(&first).unwrap(), b"one");

        let second = adopt_into(&store, &b).unwrap();
        assert_eq!(fs::read(&second).unwrap(), b"two");
        assert!(!first.exists(), "the previous picture goes");
        assert!(a.exists(), "the original stays where it was");

        // The copy itself again: kept, nothing else touched.
        let again = adopt_into(&store, &second).unwrap();
        assert_eq!(again, second);
        assert_eq!(fs::read(&again).unwrap(), b"two");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn adopting_bytes_writes_the_file() {
        let root = scratch("bytes");
        let store = root.join("background");
        let path = adopt_bytes_into(&store, "edited.png", b"png").unwrap();
        assert_eq!(path, store.join("edited.png"));
        assert_eq!(fs::read(&path).unwrap(), b"png");
        let _ = fs::remove_dir_all(&root);
    }
}
