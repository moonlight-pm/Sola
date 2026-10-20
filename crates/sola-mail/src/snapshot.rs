//! Last-session list snapshot so Mail opens on the chrome instead of a
//! blocking Connecting screen. Not an offline store.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::protocol::{Folder, FolderRef, MessageSummary};

/// Bump when the row shape changes; older files are ignored.
pub const VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub folder: FolderRef,
    pub folders: Vec<Folder>,
    pub rows: Vec<MessageSummary>,
    pub total: u32,
}

fn path() -> PathBuf {
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    root.join("sola/mail/snapshot.json")
}

pub fn load() -> Option<Snapshot> {
    let bytes = fs::read(path()).ok()?;
    serde_json::from_slice::<Snapshot>(&bytes)
        .ok()
        .filter(|s| s.version == VERSION)
}

pub fn save(snap: &Snapshot) {
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let result = serde_json::to_vec(snap)
        .map_err(std::io::Error::other)
        .and_then(|bytes| {
            // Write-then-rename so a crash never leaves half a file.
            let tmp = path.with_extension("json.tmp");
            fs::write(&tmp, bytes)?;
            fs::rename(&tmp, &path)
        });
    if let Err(e) = result {
        tracing::debug!(path = %path.display(), "mail snapshot not saved: {e}");
    }
}
