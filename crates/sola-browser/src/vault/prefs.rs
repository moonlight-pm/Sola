//! Non-secret vault chrome prefs (remembered email, etc.).
//!
//! D8: `~/.config/sola/browser/vault.json` via [`JsonConfigIn`] (shared
//! across profiles). Never put master passwords, tokens, or OTP here.

use serde::{Deserialize, Serialize};
use sola_core::config::JsonConfigIn;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VaultPrefs {
    /// Last successfully used Bitwarden account email.
    #[serde(default)]
    pub last_email: Option<String>,
    /// Cipher id → unix seconds of last fill / passkey use in this browser.
    #[serde(default)]
    pub last_used: std::collections::HashMap<String, i64>,
    /// Last username filled or created — prefill for New login.
    #[serde(default)]
    pub last_username: Option<String>,
    /// This install's Bitwarden device identifier (random, made once).
    #[serde(default)]
    pub device_id: Option<String>,
}

impl JsonConfigIn for VaultPrefs {
    const APP_DIR: &'static str = "browser";
    const FILE_NAME: &'static str = "vault.json";
}

impl VaultPrefs {
    pub fn load_email() -> Option<String> {
        Self::load()
            .last_email
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn save_email(email: &str) {
        let email = email.trim();
        if email.is_empty() {
            return;
        }
        let mut prefs = Self::load();
        if prefs.last_email.as_deref() == Some(email) {
            return;
        }
        prefs.last_email = Some(email.to_string());
        prefs.save();
    }

    /// Record that this cipher was used (fill or passkey).
    pub fn touch_cipher(id: &str) {
        if id.is_empty() {
            return;
        }
        let mut prefs = Self::load();
        prefs.last_used.insert(
            id.to_string(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        );
        prefs.save();
    }

    pub fn last_used_map() -> std::collections::HashMap<String, i64> {
        Self::load().last_used
    }

    /// This install's device identifier for Bitwarden logins. Bitwarden
    /// remembers verified devices by it, so it must not be shared: the
    /// SDK's fixed id would let anyone with the master password skip
    /// new-device email verification once any one install was verified.
    pub fn device_id() -> String {
        let mut prefs = Self::load();
        if let Some(id) = prefs.device_id.clone().filter(|s| !s.is_empty()) {
            return id;
        }
        let id = new_uuid_v4();
        prefs.device_id = Some(id.clone());
        prefs.save();
        id
    }

    pub fn load_last_username() -> Option<String> {
        Self::load()
            .last_username
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn save_last_username(username: &str) {
        let username = username.trim();
        if username.is_empty() {
            return;
        }
        let mut prefs = Self::load();
        if prefs.last_username.as_deref() == Some(username) {
            return;
        }
        prefs.last_username = Some(username.to_string());
        prefs.save();
    }
}

/// A random (version 4) UUID.
fn new_uuid_v4() -> String {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).expect("os random");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
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
}
