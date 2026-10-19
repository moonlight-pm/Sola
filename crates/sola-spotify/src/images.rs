//! Album art: memory + on-disk cache, fetched on the worker runtime.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use sha1::{Digest, Sha1};
use tokio::sync::Notify;

const HELD_BYTES: usize = 64 * 1024 * 1024;
const MAX_ART_BYTES: usize = 8 * 1024 * 1024;
/// A cover that failed to load is asked for again after this long.
const FAILED_RETRY: Duration = Duration::from_secs(60);
/// The on-disk cache is pruned back to `DISK_TARGET` once it passes
/// `DISK_LIMIT`, oldest files first.
const DISK_LIMIT: u64 = 300 * 1024 * 1024;
const DISK_TARGET: u64 = 240 * 1024 * 1024;

enum Entry {
    /// A download is under way; later callers wait on it rather than
    /// starting another.
    Pending(Arc<Notify>),
    Ready {
        bytes: Arc<[u8]>,
        last_used: Instant,
    },
    Failed {
        error: String,
        at: Instant,
    },
}

struct Inner {
    entries: Mutex<HashMap<String, Entry>>,
    http: reqwest::Client,
    cache_dir: PathBuf,
}

#[derive(Clone)]
pub struct ArtLoader {
    inner: Arc<Inner>,
}

impl ArtLoader {
    pub fn new(http: reqwest::Client, cache_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&cache_dir);
        Self {
            inner: Arc::new(Inner {
                entries: Mutex::new(HashMap::new()),
                http,
                cache_dir,
            }),
        }
    }

    pub async fn fetch(&self, url: &str) -> Result<Arc<[u8]>, String> {
        self.inner.fetch(url).await
    }

    /// Trims the on-disk cache to its budget. Blocking; run off the runtime.
    pub fn prune_disk(&self) {
        prune_dir(&self.inner.cache_dir, DISK_LIMIT, DISK_TARGET);
    }
}

impl Inner {
    fn cache_path(&self, url: &str) -> PathBuf {
        let digest = Sha1::digest(url.as_bytes());
        let mut name = String::with_capacity(40);
        for byte in digest {
            use std::fmt::Write;
            let _ = write!(name, "{byte:02x}");
        }
        self.cache_dir.join(name)
    }

    async fn fetch(self: &Arc<Self>, url: &str) -> Result<Arc<[u8]>, String> {
        if url.is_empty() {
            return Err("empty art url".into());
        }
        loop {
            let waiter = {
                let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
                match entries.get_mut(url) {
                    Some(Entry::Ready { bytes, last_used }) => {
                        *last_used = Instant::now();
                        return Ok(Arc::clone(bytes));
                    }
                    Some(Entry::Pending(notify)) => Some(Arc::clone(notify)),
                    Some(Entry::Failed { error, at }) if at.elapsed() < FAILED_RETRY => {
                        return Err(error.clone());
                    }
                    _ => {
                        entries.insert(url.to_string(), Entry::Pending(Arc::new(Notify::new())));
                        None
                    }
                }
            };
            let Some(notify) = waiter else {
                break;
            };
            // A `Notified` hears `notify_waiters` from the moment it exists,
            // so check again after creating it, then wait.
            let notified = notify.notified();
            if self.pending(url, &notify) {
                notified.await;
            }
        }

        let result = self.download(url).await;
        match &result {
            Ok(bytes) => self.finish(
                url,
                Entry::Ready {
                    bytes: Arc::clone(bytes),
                    last_used: Instant::now(),
                },
            ),
            Err(error) => self.finish(
                url,
                Entry::Failed {
                    error: error.clone(),
                    at: Instant::now(),
                },
            ),
        }
        result
    }

    /// Whether the download `notify` belongs to is still under way.
    fn pending(&self, url: &str, notify: &Arc<Notify>) -> bool {
        matches!(
            self.entries
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(url),
            Some(Entry::Pending(current)) if Arc::ptr_eq(current, notify)
        )
    }

    async fn download(&self, url: &str) -> Result<Arc<[u8]>, String> {
        let path = self.cache_path(url);
        if let Ok(bytes) = tokio::fs::read(&path).await
            && bytes.len() <= MAX_ART_BYTES
            && !bytes.is_empty()
        {
            return Ok(bytes.into());
        }
        let response = self.http.get(url).send().await.map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("art HTTP {}", response.status()));
        }
        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        if bytes.len() > MAX_ART_BYTES {
            return Err("artwork too large".into());
        }
        // A kill mid-write must never leave a truncated cover to be served.
        let tmp = path.with_extension("part");
        if tokio::fs::write(&tmp, &bytes).await.is_ok() {
            let _ = tokio::fs::rename(&tmp, &path).await;
        }
        Ok(Arc::from(bytes.as_ref()))
    }

    /// Records how a download ended and wakes anyone waiting on it.
    fn finish(&self, url: &str, entry: Entry) {
        let previous = {
            let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
            if let Entry::Ready { bytes, .. } = &entry {
                self.evict_locked(&mut entries, bytes.len());
            }
            entries.insert(url.to_string(), entry)
        };
        if let Some(Entry::Pending(notify)) = previous {
            notify.notify_waiters();
        }
    }

    fn evict_locked(&self, entries: &mut HashMap<String, Entry>, incoming: usize) {
        let mut held: Vec<(String, Instant, usize)> = Vec::new();
        let mut total = incoming;
        let mut expired = Vec::new();
        for (url, entry) in entries.iter() {
            match entry {
                Entry::Failed { at, .. } if at.elapsed() >= FAILED_RETRY => {
                    expired.push(url.clone())
                }
                Entry::Ready { bytes, last_used } => {
                    total += bytes.len();
                    held.push((url.clone(), *last_used, bytes.len()));
                }
                Entry::Failed { .. } | Entry::Pending(_) => {}
            }
        }
        for url in expired {
            entries.remove(&url);
        }
        if total <= HELD_BYTES {
            return;
        }
        held.sort_by_key(|(_, last_used, _)| *last_used);
        for (url, _, bytes) in held {
            if total <= HELD_BYTES {
                break;
            }
            entries.remove(&url);
            total = total.saturating_sub(bytes);
        }
    }
}

/// Deletes the oldest files in `dir` until it holds at most `target` bytes,
/// once it holds more than `limit`. Leftover partial writes always go.
pub fn prune_dir(dir: &Path, limit: u64, target: u64) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
    let mut total = 0u64;
    for entry in read.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        if path.extension().is_some_and(|ext| ext == "part") {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        let used = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        total += meta.len();
        files.push((path, meta.len(), used));
    }
    if total <= limit {
        return;
    }
    files.sort_by_key(|(_, _, used)| *used);
    for (path, size, _) in files {
        if total <= target {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
    tracing::info!(dir = %dir.display(), bytes = total, "pruned artwork cache");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pruning_removes_the_oldest_files_first() {
        let dir = std::env::temp_dir().join(format!("sola-spotify-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, age) in [("old", 30), ("mid", 20), ("new", 10)] {
            let path = dir.join(name);
            std::fs::write(&path, vec![0u8; 100]).unwrap();
            let when = SystemTime::now() - Duration::from_secs(age);
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(when)
                .unwrap();
        }
        std::fs::write(dir.join("half.part"), b"x").unwrap();
        prune_dir(&dir, 250, 150);
        assert!(!dir.join("old").exists());
        assert!(!dir.join("mid").exists());
        assert!(dir.join("new").exists());
        assert!(!dir.join("half.part").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
