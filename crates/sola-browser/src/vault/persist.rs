//! Age-encrypted Bitwarden session so the vault stays unlocked across
//! chrome restarts. Log out deletes this file.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use bitwarden_core::auth::JwtToken;
use bitwarden_crypto::Kdf;
use serde::{Deserialize, Serialize};
use sola_core::Encrypted;
use tracing::{info, warn};
use zeroize::Zeroize;

use crate::profiles::browser_data_root;

const FILE_NAME: &str = "vault-session.json";
/// Refresh this many seconds before JWT `exp`.
const REFRESH_SKEW_SECS: u64 = 120;

/// Secrets needed to restore an unlocked vault (no master password).
#[derive(Clone, Serialize, Deserialize)]
pub struct PersistedSession {
    pub email: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub user_key_b64: String,
    pub private_key: String,
    pub kdf: Kdf,
    pub salt: String,
    pub user_id: String,
}

impl Drop for PersistedSession {
    fn drop(&mut self) {
        self.access_token.zeroize();
        if let Some(ref mut t) = self.refresh_token {
            t.zeroize();
        }
        self.user_key_b64.zeroize();
        self.private_key.zeroize();
    }
}

#[derive(Serialize, Deserialize, Default)]
struct OnDisk {
    #[serde(default)]
    session: Option<Encrypted<PersistedSession>>,
}

pub fn path() -> PathBuf {
    browser_data_root().join(FILE_NAME)
}

pub fn load() -> Option<PersistedSession> {
    let path = path();
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "vault: session read failed");
            return None;
        }
    };
    match serde_json::from_str::<OnDisk>(&raw) {
        Ok(disk) => disk.session.map(|e| e.0),
        Err(e) => {
            warn!(path = %path.display(), error = %e, "vault: session decrypt/parse failed");
            None
        }
    }
}

pub fn save(session: &PersistedSession) {
    let path = path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            warn!(path = %parent.display(), error = %e, "vault: session dir");
            return;
        }
    }
    let disk = OnDisk {
        session: Some(Encrypted(session.clone())),
    };
    let body = match serde_json::to_string(&disk) {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "vault: session serialize failed");
            return;
        }
    };
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, body.as_bytes()) {
        warn!(path = %tmp.display(), error = %e, "vault: session write failed");
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        warn!(path = %path.display(), error = %e, "vault: session rename failed");
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    if let Err(e) = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)) {
        warn!(path = %path.display(), error = %e, "vault: session chmod 0600 failed");
    }
    info!(path = %path.display(), "vault: session saved");
}

pub fn clear() {
    let path = path();
    match std::fs::remove_file(&path) {
        Ok(()) => info!(path = %path.display(), "vault: session cleared"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => warn!(path = %path.display(), error = %e, "vault: session clear failed"),
    }
}

/// True when the access JWT is missing, unparsable, expired, or within
/// [`REFRESH_SKEW_SECS`] of expiry.
pub fn access_needs_refresh(access_token: &str) -> bool {
    let Ok(jwt) = access_token.parse::<JwtToken>() else {
        return true;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    jwt.exp <= now.saturating_add(REFRESH_SKEW_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_garbage_jwt_needs_refresh() {
        assert!(access_needs_refresh(""));
        assert!(access_needs_refresh("not-a-jwt"));
    }

    #[test]
    fn session_path_is_under_browser_data() {
        let p = path();
        assert!(p.ends_with(FILE_NAME));
        assert!(p.to_string_lossy().contains("sola/browser"));
    }
}
