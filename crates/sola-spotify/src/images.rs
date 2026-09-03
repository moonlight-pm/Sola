//! Album art: memory + on-disk cache, fetched on the worker runtime.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use sha1::{Digest, Sha1};

const HELD_BYTES: usize = 64 * 1024 * 1024;
const MAX_ART_BYTES: usize = 8 * 1024 * 1024;

enum Entry {
    Pending,
    Ready { bytes: Arc<[u8]>, last_used: Instant },
    Failed(String),
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
        {
            let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
            match entries.get_mut(url) {
                Some(Entry::Ready { bytes, last_used }) => {
                    *last_used = Instant::now();
                    return Ok(Arc::clone(bytes));
                }
                Some(Entry::Pending) => {}
                Some(Entry::Failed(err)) => return Err(err.clone()),
                None => {
                    entries.insert(url.to_string(), Entry::Pending);
                }
            }
        }

        let path = self.cache_path(url);
        if let Ok(bytes) = tokio::fs::read(&path).await
            && bytes.len() <= MAX_ART_BYTES
            && !bytes.is_empty()
        {
            return self.store(url, bytes.into());
        }

        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            let err = format!("art HTTP {}", response.status());
            self.fail(url, err.clone());
            return Err(err);
        }
        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        if bytes.len() > MAX_ART_BYTES {
            let err = "artwork too large".to_string();
            self.fail(url, err.clone());
            return Err(err);
        }
        let _ = tokio::fs::write(&path, &bytes).await;
        self.store(url, Arc::from(bytes.as_ref()))
    }

    fn store(&self, url: &str, bytes: Arc<[u8]>) -> Result<Arc<[u8]>, String> {
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        self.evict_locked(&mut entries, bytes.len());
        entries.insert(
            url.to_string(),
            Entry::Ready {
                bytes: Arc::clone(&bytes),
                last_used: Instant::now(),
            },
        );
        Ok(bytes)
    }

    fn fail(&self, url: &str, err: String) {
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        entries.insert(url.to_string(), Entry::Failed(err));
    }

    fn evict_locked(&self, entries: &mut HashMap<String, Entry>, incoming: usize) {
        let mut held: Vec<(String, Instant, usize)> = Vec::new();
        let mut total = incoming;
        let mut failed = Vec::new();
        for (url, entry) in entries.iter() {
            match entry {
                Entry::Failed(_) => failed.push(url.clone()),
                Entry::Ready { bytes, last_used } => {
                    total += bytes.len();
                    held.push((url.clone(), *last_used, bytes.len()));
                }
                Entry::Pending => {}
            }
        }
        for url in failed {
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
