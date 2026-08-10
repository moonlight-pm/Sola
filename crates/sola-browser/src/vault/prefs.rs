//! Non-secret vault chrome prefs (remembered email, etc.).
//!
//! Stored as `~/.config/sola/browser-vault.json` via [`JsonConfig`].
//! Never put master passwords, tokens, or OTP here.

use sola_core::config::JsonConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VaultPrefs {
    /// Last successfully used Bitwarden account email.
    #[serde(default)]
    pub last_email: Option<String>,
}

impl JsonConfig for VaultPrefs {
    const FILE_NAME: &'static str = "browser-vault.json";
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
}
