//! Browser downloads — shared list, persist, dest paths.
//!
//! Freeze: `docs/specs/2026-08-14-sola-browser-downloads-design.md`.
//! Files land in `~/Downloads`. The index is browser-wide under
//! `~/.local/share/sola/browser/shared/downloads.json`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cef::ipc::{DownloadEvent, DownloadPhase};

const STORE_VERSION: u32 = 1;

/// One row in the chrome list (in-progress or persisted).
#[derive(Debug, Clone)]
pub struct DownloadEntry {
    pub id: String,
    pub profile_id: String,
    pub cef_id: u32,
    pub filename: String,
    pub path: PathBuf,
    pub url: String,
    pub received: u64,
    pub total: Option<u64>,
    pub percent: Option<f32>,
    pub status: DownloadStatus,
    pub ended_unix: Option<u64>,
    /// Completed/failed since the panel was last opened.
    pub unseen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadStatus {
    InProgress,
    Complete,
    Failed,
}

/// Chrome-owned list. In-progress is memory-only; terminals persist.
#[derive(Debug, Clone)]
pub struct DownloadList {
    items: Vec<DownloadEntry>,
    /// `(profile_id, cef_id)` → persist uuid for the live helper download.
    live: HashMap<(String, u32), String>,
    persist: bool,
}

impl Default for DownloadList {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            live: HashMap::new(),
            persist: false,
        }
    }
}

impl DownloadList {
    pub fn load() -> Self {
        let store = DownloadStore::load();
        let items = store
            .items
            .into_iter()
            .filter_map(DownloadEntry::from_stored)
            .collect();
        Self {
            items,
            live: HashMap::new(),
            persist: true,
        }
    }

    pub fn items(&self) -> &[DownloadEntry] {
        &self.items
    }

    pub fn in_progress(&self) -> impl Iterator<Item = &DownloadEntry> {
        self.items
            .iter()
            .filter(|e| e.status == DownloadStatus::InProgress)
    }

    pub fn has_in_progress(&self) -> bool {
        self.items
            .iter()
            .any(|e| e.status == DownloadStatus::InProgress)
    }

    pub fn has_unseen(&self) -> bool {
        self.items.iter().any(|e| e.unseen)
    }

    /// Combined 0..1 for the icon hairline (average of known percents).
    pub fn progress_frac(&self) -> Option<f32> {
        let mut n = 0u32;
        let mut sum = 0.0f32;
        for e in self.in_progress() {
            if let Some(p) = e.percent {
                sum += p.clamp(0.0, 1.0);
                n += 1;
            }
        }
        if n == 0 {
            if self.has_in_progress() {
                Some(0.08)
            } else {
                None
            }
        } else {
            Some((sum / n as f32).max(0.08))
        }
    }

    pub fn mark_seen(&mut self) {
        for e in &mut self.items {
            e.unseen = false;
        }
    }

    pub fn apply(&mut self, profile_id: &str, ev: DownloadEvent, panel_open: bool) {
        match ev.state {
            DownloadPhase::Canceled => {
                self.drop_live(profile_id, ev.id);
            }
            DownloadPhase::Progress => {
                self.upsert_progress(profile_id, &ev);
            }
            DownloadPhase::Complete | DownloadPhase::Failed => {
                self.finish(profile_id, &ev, panel_open);
            }
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.items.retain(|e| e.id != id);
        self.live.retain(|_, v| v != id);
        self.persist();
    }

    fn upsert_progress(&mut self, profile_id: &str, ev: &DownloadEvent) {
        let key = (profile_id.to_string(), ev.id);
        if let Some(id) = self.live.get(&key).cloned() {
            if let Some(e) = self.items.iter_mut().find(|e| e.id == id) {
                fill_from_event(e, ev);
                return;
            }
        }
        let id = new_id();
        self.live.insert(key, id.clone());
        let mut entry = DownloadEntry {
            id,
            profile_id: profile_id.to_string(),
            cef_id: ev.id,
            filename: ev.filename.clone(),
            path: PathBuf::from(&ev.path),
            url: ev.url.clone(),
            received: 0,
            total: None,
            percent: None,
            status: DownloadStatus::InProgress,
            ended_unix: None,
            unseen: false,
        };
        fill_from_event(&mut entry, ev);
        // Newest in-progress first.
        self.items.insert(0, entry);
    }

    fn finish(&mut self, profile_id: &str, ev: &DownloadEvent, panel_open: bool) {
        let key = (profile_id.to_string(), ev.id);
        let id = self.live.remove(&key).unwrap_or_else(new_id);
        let status = if ev.state == DownloadPhase::Complete {
            DownloadStatus::Complete
        } else {
            DownloadStatus::Failed
        };
        if let Some(e) = self.items.iter_mut().find(|e| e.id == id) {
            fill_from_event(e, ev);
            e.status = status;
            e.ended_unix = Some(unix_now());
            e.unseen = !panel_open;
        } else {
            let mut entry = DownloadEntry {
                id,
                profile_id: profile_id.to_string(),
                cef_id: ev.id,
                filename: ev.filename.clone(),
                path: PathBuf::from(&ev.path),
                url: ev.url.clone(),
                received: 0,
                total: None,
                percent: None,
                status,
                ended_unix: Some(unix_now()),
                unseen: !panel_open,
            };
            fill_from_event(&mut entry, ev);
            self.items.insert(0, entry);
        }
        self.persist();
    }

    fn drop_live(&mut self, profile_id: &str, cef_id: u32) {
        if let Some(id) = self.live.remove(&(profile_id.to_string(), cef_id)) {
            self.items.retain(|e| e.id != id);
        }
    }

    fn persist(&self) {
        if !self.persist {
            return;
        }
        let store = DownloadStore {
            version: STORE_VERSION,
            items: self
                .items
                .iter()
                .filter(|e| e.status != DownloadStatus::InProgress)
                .map(StoredItem::from_entry)
                .collect(),
        };
        store.save();
    }
}

fn fill_from_event(e: &mut DownloadEntry, ev: &DownloadEvent) {
    if !ev.filename.is_empty() {
        e.filename = ev.filename.clone();
    }
    if !ev.path.is_empty() {
        e.path = PathBuf::from(&ev.path);
    }
    if !ev.url.is_empty() {
        e.url = ev.url.clone();
    }
    if ev.received >= 0 {
        e.received = ev.received as u64;
    }
    e.total = (ev.total > 0).then_some(ev.total as u64);
    e.percent = (ev.percent >= 0).then_some((ev.percent as f32 / 100.0).clamp(0.0, 1.0));
}

/// Open a completed file with the host default app. Returns `false` if the
/// path is missing or `xdg-open` could not spawn.
pub fn open_file(path: &Path) -> bool {
    if !path.is_file() {
        tracing::warn!(path = %path.display(), "download: file missing");
        return false;
    }
    match std::process::Command::new("xdg-open")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "download: xdg-open failed");
            false
        }
    }
}

/// `~/Downloads`, created on demand.
pub fn downloads_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = home.join("Downloads");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Collision-safe dest under `~/Downloads`.
pub fn unique_dest(suggested: &str) -> PathBuf {
    unique_path(&downloads_dir(), suggested)
}

pub fn unique_path(dir: &Path, suggested: &str) -> PathBuf {
    let name = sanitize_filename(suggested);
    let candidate = dir.join(&name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = split_stem_ext(&name);
    for n in 1..10_000 {
        let next = if ext.is_empty() {
            format!("{stem} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        let p = dir.join(next);
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}-{}.part", unix_now()))
}

pub fn sanitize_filename(raw: &str) -> String {
    let base = raw
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or(raw)
        .trim()
        .to_string();
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('.').trim();
    if cleaned.is_empty() || cleaned == ".." {
        "download".into()
    } else if cleaned.len() > 200 {
        let (stem, ext) = split_stem_ext(cleaned);
        let keep = 200usize.saturating_sub(ext.len().saturating_add(1));
        let stem: String = stem.chars().take(keep.max(1)).collect();
        if ext.is_empty() {
            stem
        } else {
            format!("{stem}.{ext}")
        }
    } else {
        cleaned.to_string()
    }
}

fn split_stem_ext(name: &str) -> (String, String) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() && !ext.contains(' ') => {
            (stem.to_string(), ext.to_string())
        }
        _ => (name.to_string(), String::new()),
    }
}

pub fn format_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let x = n as f64;
    if x >= GB {
        format!("{:.1} GB", x / GB)
    } else if x >= MB {
        format!("{:.1} MB", x / MB)
    } else if x >= KB {
        format!("{:.0} KB", x / KB)
    } else {
        format!("{n} B")
    }
}

pub fn format_progress(e: &DownloadEntry) -> String {
    match (e.percent, e.total) {
        (Some(p), Some(total)) => {
            format!(
                "{}% · {} / {}",
                (p * 100.0).round() as u32,
                format_bytes(e.received),
                format_bytes(total)
            )
        }
        (_, Some(total)) => format!("{} / {}", format_bytes(e.received), format_bytes(total)),
        (Some(p), None) => format!("{}%", (p * 100.0).round() as u32),
        _ => {
            if e.received > 0 {
                format_bytes(e.received)
            } else {
                "Starting…".into()
            }
        }
    }
}

fn index_path() -> PathBuf {
    crate::profiles::shared_dir().join("downloads.json")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DownloadStore {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    items: Vec<StoredItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredItem {
    id: String,
    filename: String,
    path: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    received: u64,
    total: Option<u64>,
    #[serde(default)]
    ended_unix: Option<u64>,
    #[serde(default)]
    failed: bool,
}

impl DownloadStore {
    fn load() -> Self {
        let path = index_path();
        match sola_core::config::load_json_or_default::<Self>(&path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "failed to load downloads");
                Self::default()
            }
        }
    }

    fn save(&self) {
        let path = index_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = sola_core::config::save_json_pretty(&path, self) {
            tracing::warn!(path = %path.display(), error = %e, "failed to write downloads");
        }
    }
}

impl StoredItem {
    fn from_entry(e: &DownloadEntry) -> Self {
        Self {
            id: e.id.clone(),
            filename: e.filename.clone(),
            path: e.path.to_string_lossy().into_owned(),
            url: e.url.clone(),
            received: e.received,
            total: e.total,
            ended_unix: e.ended_unix,
            failed: e.status == DownloadStatus::Failed,
        }
    }
}

impl DownloadEntry {
    fn from_stored(s: StoredItem) -> Option<Self> {
        if s.id.is_empty() || s.filename.is_empty() {
            return None;
        }
        Some(Self {
            id: s.id,
            profile_id: String::new(),
            cef_id: 0,
            filename: s.filename,
            path: PathBuf::from(s.path),
            url: s.url,
            received: s.received,
            total: s.total,
            percent: if s.failed { None } else { Some(1.0) },
            status: if s.failed {
                DownloadStatus::Failed
            } else {
                DownloadStatus::Complete
            },
            ended_unix: s.ended_unix,
            unseen: false,
        })
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn new_id() -> String {
    let mut b = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut b);
    } else {
        let t = unix_now().to_le_bytes();
        b[..8].copy_from_slice(&t);
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_paths_and_controls() {
        assert_eq!(sanitize_filename("/tmp/../evil.pdf"), "evil.pdf");
        assert_eq!(sanitize_filename("a\\b\\c.txt"), "c.txt");
        assert_eq!(sanitize_filename("  "), "download");
        assert_eq!(sanitize_filename("foo:bar?.bin"), "foo_bar_.bin");
    }

    #[test]
    fn unique_path_adds_numeric_suffix() {
        let dir = std::env::temp_dir().join(format!("sola-dl-{}", unix_now()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = unique_path(&dir, "report.pdf");
        std::fs::write(&a, b"x").unwrap();
        let b = unique_path(&dir, "report.pdf");
        assert_eq!(b.file_name().unwrap(), "report (1).pdf");
        std::fs::write(&b, b"y").unwrap();
        let c = unique_path(&dir, "report.pdf");
        assert_eq!(c.file_name().unwrap(), "report (2).pdf");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_progress_then_complete_persists_shape() {
        let mut list = DownloadList::default();
        let ev = DownloadEvent {
            id: 7,
            filename: "a.pdf".into(),
            path: "/tmp/a.pdf".into(),
            url: "https://ex/a.pdf".into(),
            received: 10,
            total: 100,
            percent: 10,
            state: DownloadPhase::Progress,
        };
        list.apply("p1", ev.clone(), false);
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0].status, DownloadStatus::InProgress);
        assert!(list.has_in_progress());
        let mut done = ev;
        done.received = 100;
        done.percent = 100;
        done.state = DownloadPhase::Complete;
        list.apply("p1", done, false);
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0].status, DownloadStatus::Complete);
        assert!(list.items[0].unseen);
        assert!(!list.has_in_progress());
    }

    #[test]
    fn cancel_drops_row() {
        let mut list = DownloadList::default();
        list.apply(
            "p1",
            DownloadEvent {
                id: 1,
                filename: "x.bin".into(),
                path: "/tmp/x.bin".into(),
                url: String::new(),
                received: 1,
                total: 10,
                percent: 10,
                state: DownloadPhase::Progress,
            },
            false,
        );
        list.apply(
            "p1",
            DownloadEvent {
                id: 1,
                filename: "x.bin".into(),
                path: "/tmp/x.bin".into(),
                url: String::new(),
                received: 1,
                total: 10,
                percent: 10,
                state: DownloadPhase::Canceled,
            },
            false,
        );
        assert!(list.items.is_empty());
    }

    #[test]
    fn format_bytes_buckets() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(2048), "2 KB");
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.0 MB");
    }
}
