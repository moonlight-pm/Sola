//! Non-secret vault chrome prefs (remembered email, etc.).
//!
//! D8: `~/.config/sola/browser/vault.json` via [`JsonConfigIn`] (shared
//! across profiles). Never put master passwords, tokens, or OTP here.

use sola_core::config::JsonConfigIn;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VaultPrefs {
    /// Last successfully used Bitwarden account email.
    #[serde(default)]
    pub last_email: Option<String>,
    /// Cipher id → unix seconds of last fill / passkey use in this browser.
    #[serde(default)]
    pub last_used: std::collections::HashMap<String, i64>,
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
}
