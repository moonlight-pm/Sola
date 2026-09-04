//! Last-session folder + list snapshot so Mail opens on the chrome
//! instead of a blocking Connecting screen. Not a full offline store.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::protocol::{Folder, MessageSummary};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub accounts: Vec<String>,
    pub folder: String,
    pub folders: Vec<Folder>,
    pub messages: Vec<MessageSummary>,
    pub total: u32,
    #[serde(default)]
    pub from_addresses: Vec<String>,
}

fn path() -> PathBuf {
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                let mut p = PathBuf::from(h);
                p.push(".local/state");
                p
            })
        })
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    root.join("sola/mail/snapshot.json")
}

pub fn load() -> Option<Snapshot> {
    let bytes = fs::read(path()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn save(snap: &Snapshot) {
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec(snap) {
        let _ = fs::write(path, bytes);
    }
}

pub fn matches_accounts(snap: &Snapshot, ids: &[String]) -> bool {
    if snap.accounts.is_empty() || ids.is_empty() {
        return false;
    }
    let mut a = snap.accounts.clone();
    let mut b = ids.to_vec();
    a.sort();
    b.sort();
    a == b
}
