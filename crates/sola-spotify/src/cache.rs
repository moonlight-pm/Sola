//! On-disk JSON helpers and the skipped-track list.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, mpsc};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::paths::AppDirs;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Skipped {
    #[serde(default)]
    pub uris: HashSet<String>,
}

impl Skipped {
    pub fn load(dirs: &AppDirs) -> Self {
        read_json(&dirs.skipped_file()).unwrap_or_default()
    }

    pub fn save(&self, dirs: &AppDirs) {
        write_json_later(dirs.skipped_file(), self.clone());
    }

    pub fn contains(&self, uri: &str) -> bool {
        self.uris.contains(uri)
    }

    pub fn toggle(&mut self, uri: String) -> bool {
        if !self.uris.remove(&uri) {
            self.uris.insert(uri);
            true
        } else {
            false
        }
    }
}

/// Liked Songs URIs, so likes survive a 429 on the shared Web API app.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Liked {
    #[serde(default)]
    pub uris: HashSet<String>,
}

impl Liked {
    pub fn load(dirs: &AppDirs) -> Self {
        read_json(&dirs.liked_file()).unwrap_or_default()
    }

    pub fn save(&self, dirs: &AppDirs) {
        write_json_later(dirs.liked_file(), self.clone());
    }

    pub fn set(&mut self, uri: String, saved: bool) {
        if saved {
            self.uris.insert(uri);
        } else {
            self.uris.remove(&uri);
        }
    }
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) {
    let Ok(text) = serde_json::to_string(value) else {
        return;
    };
    write_text(path, &text);
}

/// Writes through a temporary file so a kill mid-write never leaves a torn
/// file behind.
pub fn write_text(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(tmp, path);
    }
}

type Job = Box<dyn FnOnce() + Send>;

/// One background thread runs disk writes in the order they were asked
/// for, so the UI thread never waits on the filesystem.
fn writer() -> &'static mpsc::Sender<Job> {
    static WRITER: OnceLock<mpsc::Sender<Job>> = OnceLock::new();
    WRITER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        let spawned = std::thread::Builder::new()
            .name("sola-spotify-disk".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    job();
                }
            });
        if let Err(error) = spawned {
            tracing::warn!("unable to start the disk writer: {error}");
        }
        tx
    })
}

/// Serializes and writes `value` on the disk thread.
pub fn write_json_later<T: Serialize + Send + 'static>(path: PathBuf, value: T) {
    let job: Job = Box::new(move || write_json(&path, &value));
    if let Err(mpsc::SendError(job)) = writer().send(job) {
        job();
    }
}

/// Writes `text` on the disk thread.
pub fn write_text_later(path: PathBuf, text: String) {
    let job: Job = Box::new(move || write_text(&path, &text));
    if let Err(mpsc::SendError(job)) = writer().send(job) {
        job();
    }
}
