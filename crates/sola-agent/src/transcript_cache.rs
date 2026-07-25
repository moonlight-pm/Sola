//! In-memory transcript cache for fast session bouncing.
//!
//! Entries live for up to [`TTL`] and are capped at [`MAX_ENTRIES`]
//! (LRU by last access). Stale relative to `updates.jsonl` length are
//! treated as a miss so live TUI writes still show up.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::protocol::Turn;

/// How long a cached transcript stays valid without re-read.
pub const TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Soft cap — bouncing among many projects should not balloon RSS.
pub const MAX_ENTRIES: usize = 32;

#[derive(Debug, Clone)]
pub struct CachedTranscript {
    pub turns: Vec<Turn>,
    pub history_start_byte: u64,
    pub has_older_history: bool,
    pub session_title: Option<String>,
    /// Composer draft so bounce-back restores mid-edit text.
    pub draft: String,
    pub scroll_rel_y: Option<f32>,
    pub stick_to_bottom: bool,
    /// `updates.jsonl` length when this entry was stored.
    pub file_len: u64,
    inserted_at: Instant,
    last_access: Instant,
}

impl CachedTranscript {
    pub fn new(
        turns: Vec<Turn>,
        history_start_byte: u64,
        has_older_history: bool,
        session_title: Option<String>,
        draft: String,
        scroll_rel_y: Option<f32>,
        stick_to_bottom: bool,
        file_len: u64,
    ) -> Self {
        let now = Instant::now();
        Self {
            turns,
            history_start_byte,
            has_older_history,
            session_title,
            draft,
            scroll_rel_y,
            stick_to_bottom,
            file_len,
            inserted_at: now,
            last_access: now,
        }
    }

    fn expired(&self) -> bool {
        self.inserted_at.elapsed() > TTL
    }
}

#[derive(Debug, Default)]
pub struct TranscriptCache {
    map: HashMap<String, CachedTranscript>,
}

impl TranscriptCache {
    pub fn get_fresh(&mut self, id: &str, current_file_len: u64) -> Option<CachedTranscript> {
        let entry = self.map.get_mut(id)?;
        if entry.expired() {
            self.map.remove(id);
            return None;
        }
        // File grew (or shrank after rewrite) — force a re-read.
        if entry.file_len != current_file_len {
            self.map.remove(id);
            return None;
        }
        entry.last_access = Instant::now();
        Some(entry.clone())
    }

    pub fn insert(&mut self, id: String, entry: CachedTranscript) {
        self.map.insert(id, entry);
        self.evict();
    }

    pub fn remove(&mut self, id: &str) {
        self.map.remove(id);
    }

    /// Drop expired and, if still over cap, least-recently-accessed.
    fn evict(&mut self) {
        self.map.retain(|_, e| !e.expired());
        while self.map.len() > MAX_ENTRIES {
            let oldest = self
                .map
                .iter()
                .min_by_key(|(_, e)| e.last_access)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                self.map.remove(&k);
            } else {
                break;
            }
        }
    }
}
