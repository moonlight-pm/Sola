use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sola_bus::topics::Zone;

/// On-disk entry (persisted fields only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedEntry {
    pub app_id: String,
    pub zone: Zone,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistedSession {
    pub entries: Vec<PersistedEntry>,
}

/// In-memory entry. `window_id` is runtime-only and not persisted.
#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub app_id: String,
    pub zone: Zone,
    pub window_id: Option<u32>,
}

impl SessionEntry {
    pub fn persisted(&self) -> PersistedEntry {
        PersistedEntry {
            app_id: self.app_id.clone(),
            zone: self.zone,
        }
    }
}

/// `$XDG_STATE_HOME/sola/session.json` (default `~/.local/state/sola/session.json`).
pub fn state_file() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let base = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(format!("{home}/.local/state")));
    base.join("sola").join("session.json")
}

/// Load session entries. Missing → empty. Unparseable → back up and
/// start empty.
pub fn load() -> Vec<SessionEntry> {
    let path = state_file();
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            tracing::warn!(%e, ?path, "session.json read failed");
            return Vec::new();
        }
    };
    match serde_json::from_slice::<PersistedSession>(&bytes) {
        Ok(s) => s
            .entries
            .into_iter()
            .map(|e| SessionEntry {
                app_id: e.app_id,
                zone: e.zone,
                window_id: None,
            })
            .collect(),
        Err(e) => {
            tracing::warn!(%e, ?path, "session.json parse failed; backing up");
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let backup = path.with_extension(format!("json.bak-{ts}"));
            let _ = std::fs::rename(&path, &backup);
            Vec::new()
        }
    }
}

/// Atomic write: write to a temp file, then rename over the final path.
pub fn save(entries: &[SessionEntry]) {
    let path = state_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let persisted = PersistedSession {
        entries: entries.iter().map(|e| e.persisted()).collect(),
    };
    let bytes = match serde_json::to_vec_pretty(&persisted) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(%e, "session.json serialize failed");
            return;
        }
    };
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, &bytes) {
        tracing::warn!(%e, ?tmp, "session.json temp write failed");
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        tracing::warn!(%e, ?path, "session.json rename failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_roundtrip() {
        let entries = vec![
            SessionEntry {
                app_id: "a".into(),
                zone: Zone::Top,
                window_id: None,
            },
            SessionEntry {
                app_id: "b".into(),
                zone: Zone::Bottom,
                window_id: Some(7),
            },
        ];
        let persisted = PersistedSession {
            entries: entries.iter().map(|e| e.persisted()).collect(),
        };
        let json = serde_json::to_string(&persisted).unwrap();
        let back: PersistedSession = serde_json::from_str(&json).unwrap();
        assert_eq!(back.entries.len(), 2);
        assert_eq!(back.entries[0].app_id, "a");
        assert_eq!(back.entries[0].zone, Zone::Top);
        assert_eq!(back.entries[1].app_id, "b");
        assert_eq!(back.entries[1].zone, Zone::Bottom);
    }

    #[test]
    fn window_id_not_persisted() {
        let entries = vec![SessionEntry {
            app_id: "x".into(),
            zone: Zone::TopMiddle,
            window_id: Some(42),
        }];
        let persisted = PersistedSession {
            entries: entries.iter().map(|e| e.persisted()).collect(),
        };
        let json = serde_json::to_string(&persisted).unwrap();
        assert!(!json.contains("window_id"), "window_id leaked into persisted JSON: {json}");
        assert!(!json.contains("42"));
    }
}
