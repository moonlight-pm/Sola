//! The daemon's prefs (`prefs.json` in the state dir): this install's
//! Bitwarden device id, the last email, and when each item was last used.
//! Nothing secret.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::warn;

const FILE_NAME: &str = "prefs.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Prefs {
    /// The account last signed in (for the sign-in form).
    #[serde(default)]
    pub last_email: Option<String>,
    /// Item id → unix seconds of its last fill, delivery or passkey use.
    #[serde(default)]
    pub last_used: HashMap<String, i64>,
    /// This install's Bitwarden device identifier (random, made once).
    /// Bitwarden remembers verified devices by it.
    #[serde(default)]
    pub device_id: Option<String>,
}

impl Prefs {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(FILE_NAME);
        match std::fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                warn!(path = %path.display(), error = %e, "vault: prefs unreadable; starting fresh");
                Self::default()
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                warn!(path = %path.display(), error = %e, "vault: prefs read failed");
                Self::default()
            }
        }
    }

    pub fn save(&self, dir: &Path) {
        let path = dir.join(FILE_NAME);
        let body = match serde_json::to_vec_pretty(self) {
            Ok(b) => b,
            Err(e) => return warn!(error = %e, "vault: prefs serialize failed"),
        };
        if let Err(e) = crate::paths::write_private(&path, &body) {
            warn!(path = %path.display(), error = %e, "vault: prefs write failed");
        }
    }

    /// This install's device id, made (and saved) the first time.
    pub fn device_id(&mut self, dir: &Path) -> String {
        if let Some(id) = self.device_id.clone().filter(|s| !s.is_empty()) {
            return id;
        }
        let id = new_uuid_v4();
        self.device_id = Some(id.clone());
        self.save(dir);
        id
    }

    /// Note that item `id` was used now.
    pub fn touch(&mut self, dir: &Path, id: &str) {
        if id.is_empty() {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.last_used.insert(id.to_string(), now);
        self.save(dir);
    }

    pub fn remember_email(&mut self, dir: &Path, email: &str) {
        let email = email.trim();
        if email.is_empty() || self.last_email.as_deref() == Some(email) {
            return;
        }
        self.last_email = Some(email.to_string());
        self.save(dir);
    }
}

/// A random (version 4) UUID.
fn new_uuid_v4() -> String {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).expect("os random");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_ids_are_random_v4_uuids() {
        let a = new_uuid_v4();
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "4");
        assert!(matches!(&a[19..20], "8" | "9" | "a" | "b"));
        assert_ne!(a, new_uuid_v4());
    }

    #[test]
    fn the_device_id_is_made_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = Prefs::load(dir.path());
        let id = p.device_id(dir.path());
        assert_eq!(Prefs::load(dir.path()).device_id(dir.path()), id);
    }
}
