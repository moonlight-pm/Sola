//! Single-owner metadata store.
//!
//! Every read/write of a `SessionMeta` on disk goes through here. Holding
//! the lock across load→mutate→save closes the races that used to let a
//! concurrent `cmd_rename` get clobbered by an in-flight turn's metrics
//! save (both paths did load_meta → mutate → save_meta_full in parallel).
//!
//! Each method mutates only the fields it owns — `rename` is the sole
//! writer of `name`, `update_metrics` only touches `metrics`/`updated_at`,
//! etc. Merges happen inside the lock against the live in-memory copy, so
//! no caller can reintroduce a stale field.

use crate::storage::{self, SessionMeta};
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct MetaStore {
    inner: Mutex<HashMap<String, SessionMeta>>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl MetaStore {
    /// Load every saved session into memory.
    pub fn new() -> Self {
        let mut map = HashMap::new();
        for meta in storage::list_all() {
            map.insert(meta.session_id.clone(), meta);
        }
        Self { inner: Mutex::new(map) }
    }

    /// Snapshot of all sessions, sorted by most recently updated.
    pub fn list_all(&self) -> Vec<SessionMeta> {
        let mut vec: Vec<SessionMeta> = self.inner.lock().unwrap().values().cloned().collect();
        vec.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        vec
    }

    pub fn get(&self, id: &str) -> Option<SessionMeta> {
        self.inner.lock().unwrap().get(id).cloned()
    }

    /// Insert a fresh session. No-op if one already exists with this id.
    pub fn create(&self, id: &str, working_dir: &str, name: Option<&str>) -> Result<()> {
        let mut map = self.inner.lock().unwrap();
        if map.contains_key(id) { return Ok(()); }
        let now = now_ms();
        let meta = SessionMeta {
            session_id: id.to_string(),
            name: name.map(String::from),
            working_dir: working_dir.to_string(),
            created_at: now,
            updated_at: now,
            metrics: None,
            model: "opus".into(),
            effort: "high".into(),
            cli_synced_at: 0,
            metrics_schema: 0,
        };
        storage::save_meta_full(&meta)?;
        map.insert(id.to_string(), meta);
        Ok(())
    }

    /// Set the display name. The only path that writes `name` — every
    /// other mutator reads it back from the locked in-memory copy, so a
    /// turn completing in the middle of a rename can't overwrite it.
    pub fn rename(&self, id: &str, name: String) -> Result<()> {
        let mut map = self.inner.lock().unwrap();
        if let Some(meta) = map.get_mut(id) {
            meta.name = Some(name);
            meta.updated_at = now_ms();
            storage::save_meta_full(meta)?;
        }
        Ok(())
    }

    pub fn update_config(&self, id: &str, model: Option<&str>, effort: Option<&str>) -> Result<()> {
        let mut map = self.inner.lock().unwrap();
        if let Some(meta) = map.get_mut(id) {
            if let Some(m) = model { meta.model = m.to_string(); }
            if let Some(e) = effort { meta.effort = e.to_string(); }
            storage::save_meta_full(meta)?;
        }
        Ok(())
    }

    /// Atomically replace metrics + updated_at. Never touches `name`.
    pub fn update_metrics(&self, id: &str, metrics: Value, updated_at: u64) -> Result<()> {
        let mut map = self.inner.lock().unwrap();
        if let Some(meta) = map.get_mut(id) {
            meta.metrics = Some(metrics);
            meta.updated_at = updated_at;
            storage::save_meta_full(meta)?;
        }
        Ok(())
    }

    /// Bump updated_at only — used when user sends a message so the row
    /// sorts to the top before the turn's metrics arrive.
    pub fn touch(&self, id: &str) -> Result<()> {
        let mut map = self.inner.lock().unwrap();
        if let Some(meta) = map.get_mut(id) {
            meta.updated_at = now_ms();
            storage::save_meta_full(meta)?;
        }
        Ok(())
    }

    /// Apply a CLI sync rebuild. Preserves user-editable fields (`name`,
    /// `model`, `effort`) from the in-memory copy; overwrites
    /// CLI-derivable fields (`working_dir`, `updated_at`, `metrics`,
    /// `cli_synced_at`, `metrics_schema`).
    pub fn apply_cli_rebuild(
        &self,
        id: &str,
        working_dir: String,
        first_prompt: Option<String>,
        cli_synced_at: u64,
        metrics_schema: u8,
        metrics: Option<Value>,
    ) -> Result<SessionMeta> {
        let mut map = self.inner.lock().unwrap();
        let existing = map.get(id).cloned();
        let now = now_ms();
        let meta = SessionMeta {
            session_id: id.to_string(),
            // Seed name from first_prompt only for brand-new entries;
            // keep any user-set name otherwise.
            name: existing.as_ref().and_then(|e| e.name.clone()).or(first_prompt),
            working_dir,
            created_at: existing.as_ref().map(|e| e.created_at).unwrap_or(now),
            // updated_at reflects when the session was last active in the
            // CLI, not when we rebuilt our view model.
            updated_at: if cli_synced_at > 0 {
                cli_synced_at
            } else {
                existing.as_ref().map(|e| e.updated_at).unwrap_or(now)
            },
            metrics,
            model: existing.as_ref().map(|e| e.model.clone()).unwrap_or_else(|| "opus".into()),
            effort: existing.as_ref().map(|e| e.effort.clone()).unwrap_or_else(|| "high".into()),
            cli_synced_at,
            metrics_schema,
        };
        storage::save_meta_full(&meta)?;
        map.insert(id.to_string(), meta.clone());
        Ok(meta)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let mut map = self.inner.lock().unwrap();
        map.remove(id);
        storage::delete_session(id)?;
        Ok(())
    }
}
