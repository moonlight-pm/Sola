//! The signed-in, locked account on disk (V1): only what Bitwarden's own
//! clients keep for a locked account — the email, server, tokens, KDF and
//! salt, the user id, the user key *as the master key wraps it* and the
//! encrypted private key. Age-encrypted with the desk key besides
//! ([`sola_core::Encrypted`]). The decrypted user key is never written:
//! after a restart the vault is signed in and locked, and the master
//! password unlocks it with no round trip.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bitwarden_core::auth::JwtToken;
use bitwarden_crypto::{EncString, Kdf, MasterKey};
use serde::{Deserialize, Serialize};
use sola_core::Encrypted;
use sola_vault_proto::Server;
use tracing::{info, warn};
use zeroize::Zeroize;

use super::errors::{Result, internal};

const FILE_NAME: &str = "session.json";
/// Refresh this many seconds before the access token's `exp`.
const REFRESH_SKEW_SECS: u64 = 120;

/// A signed-in account, locked. Tokens are zeroized on drop.
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub email: String,
    #[serde(default)]
    pub server: Server,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub kdf: Kdf,
    /// The KDF salt (the account's email as Bitwarden has it).
    pub salt: String,
    pub user_id: String,
    /// The user key as the master key wraps it (an `EncString`).
    pub wrapped_user_key: String,
    /// The account's private key, encrypted with the user key.
    pub private_key: String,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.access_token.zeroize();
        if let Some(t) = self.refresh_token.as_mut() {
            t.zeroize();
        }
        self.private_key.zeroize();
    }
}

impl Session {
    /// The KDF salt, falling back to the email.
    pub fn salt(&self) -> &str {
        if self.salt.is_empty() {
            &self.email
        } else {
            &self.salt
        }
    }

    pub fn wrapped_user_key(&self) -> Result<EncString> {
        self.wrapped_user_key
            .parse()
            .map_err(|e| internal(format!("The stored key could not be read ({e}).")))
    }

    /// Does `password` unwrap this account's user key? Runs the KDF;
    /// nothing is kept or sent.
    pub fn check_password(&self, password: &str) -> Result<bool> {
        let wrapped = self.wrapped_user_key()?;
        let key = MasterKey::derive(password, self.salt(), &self.kdf)
            .map_err(|e| internal(format!("master key: {e}")))?;
        Ok(key.decrypt_user_key(wrapped).is_ok())
    }
}

#[derive(Serialize, Deserialize, Default)]
struct OnDisk {
    #[serde(default)]
    session: Option<Encrypted<Session>>,
}

pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

pub fn load(dir: &Path) -> Option<Session> {
    let path = path(dir);
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

pub fn save(dir: &Path, session: &Session) {
    let path = path(dir);
    let disk = OnDisk {
        session: Some(Encrypted(session.clone())),
    };
    let mut body = match serde_json::to_string(&disk) {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "vault: session serialize failed");
            return;
        }
    };
    match crate::paths::write_private(&path, body.as_bytes()) {
        Ok(()) => info!(path = %path.display(), "vault: session saved"),
        Err(e) => warn!(path = %path.display(), error = %e, "vault: session write failed"),
    }
    body.zeroize();
}

pub fn clear(dir: &Path) {
    let path = path(dir);
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
    fn the_disk_form_has_no_user_key() {
        let s = Session {
            email: "a@b.c".into(),
            server: Server::Eu,
            access_token: "t".into(),
            refresh_token: None,
            kdf: Kdf::default_pbkdf2(),
            salt: String::new(),
            user_id: "u".into(),
            wrapped_user_key: "w".into(),
            private_key: "p".into(),
        };
        let v = serde_json::to_value(&s).unwrap();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert!(!keys.iter().any(|k| k.contains("user_key_b64")), "{keys:?}");
        assert_eq!(s.salt(), "a@b.c");
    }
}
