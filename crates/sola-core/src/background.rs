//! The desktop's background pictures: Sola keeps its own copy of every
//! picture set under `~/.config/sola/background/`, so a picture stays when
//! the original moves or goes, and one set before can be set again
//! (Settings → Appearance). [`adopt`] copies a file there (or
//! [`adopt_bytes`] writes one) and returns the copy's path for
//! `Topic::Background`; [`kept`] lists the copies, [`forget`] removes one.
//! sola-river draws the one set.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Where the pictures live.
pub fn dir() -> PathBuf {
    crate::config::sola_config_dir().join("background")
}

/// Keep a copy of `src` in [`dir`]. A copy already there (`src` itself, or
/// a file of its name with the same bytes) is used as is.
pub fn adopt(src: &Path) -> io::Result<PathBuf> {
    adopt_into(&dir(), src)
}

/// Keep `bytes` (an edited image, encoded by the caller) in [`dir`] as
/// `file_name`, or under a free name when another picture has it.
pub fn adopt_bytes(file_name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    adopt_bytes_into(&dir(), file_name, bytes)
}

/// The kept pictures, the newest first.
pub fn kept() -> Vec<PathBuf> {
    kept_in(&dir())
}

/// Remove the kept picture `path`.
pub fn forget(path: &Path) -> io::Result<()> {
    forget_in(&dir(), path)
}

fn adopt_into(dir: &Path, src: &Path) -> io::Result<PathBuf> {
    let src = fs::canonicalize(src)?;
    let name = src
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a file"))?
        .to_string_lossy()
        .into_owned();
    fs::create_dir_all(dir)?;
    if src.parent() == Some(fs::canonicalize(dir)?.as_path()) {
        return Ok(dir.join(&name));
    }
    keep(dir, &name, &fs::read(&src)?)
}

fn adopt_bytes_into(dir: &Path, file_name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let name = Path::new(file_name)
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?
        .to_string_lossy()
        .into_owned();
    fs::create_dir_all(dir)?;
    keep(dir, &name, bytes)
}

/// `bytes` in `dir` under `name`: the file there when it holds the same,
/// else a new file under the first free name (`name-2.ext`, `name-3.ext`, …).
fn keep(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    for candidate in names(name) {
        let dest = dir.join(&candidate);
        match fs::metadata(&dest) {
            Ok(m) if m.len() == bytes.len() as u64 && fs::read(&dest)? == bytes => return Ok(dest),
            Ok(_) => continue,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let tmp = dir.join(format!(".{candidate}.tmp"));
                fs::write(&tmp, bytes)?;
                fs::rename(&tmp, &dest)?;
                return Ok(dest);
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!("names() never ends")
}

/// `name`, then `stem-2.ext`, `stem-3.ext`, …
fn names(name: &str) -> impl Iterator<Item = String> {
    let path = Path::new(name);
    let stem = path.file_stem().map_or_else(|| name.to_string(), |s| s.to_string_lossy().into_owned());
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned());
    std::iter::once(name.to_string()).chain((2u32..).map(move |n| match &ext {
        Some(ext) => format!("{stem}-{n}.{ext}"),
        None => format!("{stem}-{n}"),
    }))
}

fn kept_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            let m = e.metadata().ok().filter(|m| m.is_file())?;
            Some((m.modified().unwrap_or(SystemTime::UNIX_EPOCH), e.path()))
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    files.into_iter().map(|(_, path)| path).collect()
}

fn forget_in(dir: &Path, path: &Path) -> io::Result<()> {
    if path.parent() != Some(dir) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a kept picture", path.display()),
        ));
    }
    fs::remove_file(path)
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
    fn adopting_keeps_every_picture() {
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
        assert!(first.exists(), "the previous picture is kept");
        assert!(a.exists(), "the original stays where it was");

        // The copy itself again, or the same file again: kept as is.
        assert_eq!(adopt_into(&store, &second).unwrap(), second);
        assert_eq!(adopt_into(&store, &a).unwrap(), first);

        // Another picture under a kept one's name gets a name of its own.
        let other = root.join("other");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("a.jpg"), b"three").unwrap();
        let third = adopt_into(&store, &other.join("a.jpg")).unwrap();
        assert_eq!(third, store.join("a-2.jpg"));
        assert_eq!(fs::read(&first).unwrap(), b"one");

        let mut all = kept_in(&store);
        all.sort();
        assert_eq!(all, [third.clone(), first.clone(), second.clone()]);

        forget_in(&store, &first).unwrap();
        assert!(!first.exists());
        assert!(forget_in(&store, &a).is_err(), "only a kept picture");
        assert!(a.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn adopting_bytes_writes_the_file() {
        let root = scratch("bytes");
        let store = root.join("background");
        let path = adopt_bytes_into(&store, "edited.png", b"png").unwrap();
        assert_eq!(path, store.join("edited.png"));
        assert_eq!(fs::read(&path).unwrap(), b"png");
        let again = adopt_bytes_into(&store, "edited.png", b"png 2").unwrap();
        assert_eq!(again, store.join("edited-2.png"));
        let _ = fs::remove_dir_all(&root);
    }
}
