//! Take over the browser's vault (P8): its device id, so Bitwarden does
//! not see a new device, and its session without the decrypted user key
//! it held (V1). The browser's session file is deleted after.

use std::path::Path;

use bitwarden_crypto::Kdf;
use serde::Deserialize;
use sola_core::Encrypted;
use sola_vault_proto::Server;
use tracing::{info, warn};
use zeroize::Zeroize;

use crate::prefs::Prefs;
use crate::vault::{Session, session};

/// The browser's session as it wrote it, less `user_key_b64` (never read
/// into a field here).
#[derive(Deserialize)]
struct BrowserSession {
    email: String,
    access_token: String,
    refresh_token: Option<String>,
    private_key: String,
    kdf: Kdf,
    salt: String,
    user_id: String,
    #[serde(default)]
    wrapped_user_key: Option<String>,
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        self.access_token.zeroize();
        if let Some(t) = self.refresh_token.as_mut() {
            t.zeroize();
        }
    }
}

#[derive(Deserialize, Default)]
struct BrowserSessionFile {
    #[serde(default)]
    session: Option<Encrypted<BrowserSession>>,
}

#[derive(Deserialize, Default)]
struct BrowserPrefs {
    #[serde(default)]
    last_email: Option<String>,
    #[serde(default)]
    last_used: std::collections::HashMap<String, i64>,
    #[serde(default)]
    device_id: Option<String>,
}

/// What a browser session comes to here.
enum Taken {
    /// Signed in, locked.
    Session(Session),
    /// No wrapped key: it cannot be unlocked offline. Signed out; the
    /// email is kept for the sign-in form.
    EmailOnly(String),
}

pub fn run(dir: &Path, prefs: &mut Prefs) {
    take_prefs(&crate::paths::browser_prefs(), dir, prefs);
    take_session(&crate::paths::browser_session(), dir, prefs);
}

fn take_prefs(path: &Path, dir: &Path, prefs: &mut Prefs) {
    if prefs.device_id.is_some() {
        return;
    }
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let theirs: BrowserPrefs = match serde_json::from_str(&raw) {
        Ok(p) => p,
        Err(e) => return warn!(path = %path.display(), error = %e, "vault: browser prefs unreadable"),
    };
    let Some(device_id) = theirs.device_id.filter(|s| !s.is_empty()) else {
        return;
    };
    prefs.device_id = Some(device_id);
    if prefs.last_email.is_none() {
        prefs.last_email = theirs.last_email;
    }
    for (id, at) in theirs.last_used {
        prefs.last_used.entry(id).or_insert(at);
    }
    prefs.save(dir);
    info!(path = %path.display(), "vault: took the browser's device id and last-used clock");
}

fn take_session(path: &Path, dir: &Path, prefs: &mut Prefs) {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => return warn!(path = %path.display(), error = %e, "vault: browser session unreadable"),
    };
    if session::load(dir).is_none() {
        match serde_json::from_str::<BrowserSessionFile>(&raw) {
            Ok(file) => match file.session.map(|e| from_browser(e.0)) {
                Some(Taken::Session(s)) => {
                    session::save(dir, &s);
                    info!("vault: took the browser's session (signed in, locked)");
                }
                Some(Taken::EmailOnly(email)) => {
                    prefs.remember_email(dir, &email);
                    info!("vault: the browser's session has no wrapped key; signed out");
                }
                None => {}
            },
            Err(e) => warn!(error = %e, "vault: browser session could not be opened; dropping it"),
        }
    }
    let mut raw = raw;
    raw.zeroize();
    match std::fs::remove_file(path) {
        Ok(()) => info!(path = %path.display(), "vault: deleted the browser's session file"),
        Err(e) => warn!(path = %path.display(), error = %e, "vault: could not delete the browser's session file"),
    }
}

fn from_browser(b: BrowserSession) -> Taken {
    let Some(wrapped) = b.wrapped_user_key.clone().filter(|w| !w.is_empty()) else {
        return Taken::EmailOnly(b.email.clone());
    };
    Taken::Session(Session {
        email: b.email.clone(),
        // The browser only ever signed in to bitwarden.com.
        server: Server::Us,
        access_token: b.access_token.clone(),
        refresh_token: b.refresh_token.clone(),
        kdf: b.kdf.clone(),
        salt: b.salt.clone(),
        user_id: b.user_id.clone(),
        wrapped_user_key: wrapped,
        private_key: b.private_key.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BROWSER: &str = r#"{
        "email": "ana@example.com",
        "access_token": "a",
        "refresh_token": "r",
        "user_key_b64": "THE-DECRYPTED-KEY",
        "private_key": "2.p",
        "kdf": {"pBKDF2": {"iterations": 600000}},
        "salt": "ana@example.com",
        "user_id": "u",
        "wrapped_user_key": "2.w"
    }"#;

    fn browser(json: &str) -> BrowserSession {
        let mut v: serde_json::Value = serde_json::from_str(json).unwrap();
        v["kdf"] = serde_json::to_value(Kdf::default_pbkdf2()).unwrap();
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_browser_session_comes_over_locked_without_its_key() {
        let Taken::Session(s) = from_browser(browser(BROWSER)) else {
            panic!("expected a session");
        };
        assert_eq!(s.wrapped_user_key, "2.w");
        assert_eq!(s.server, Server::Us);
        let saved = serde_json::to_string(&s).unwrap();
        assert!(!saved.contains("THE-DECRYPTED-KEY"));
    }

    #[test]
    fn without_a_wrapped_key_only_the_email_comes_over() {
        let json = BROWSER.replace(r#""wrapped_user_key": "2.w""#, r#""wrapped_user_key": null"#);
        assert!(matches!(from_browser(browser(&json)), Taken::EmailOnly(e) if e == "ana@example.com"));
    }

    #[test]
    fn the_browser_device_id_is_taken_once() {
        let dir = tempfile::tempdir().unwrap();
        let theirs = dir.path().join("vault.json");
        std::fs::write(&theirs, r#"{"device_id":"dev-1","last_email":"ana@example.com","last_used":{"x":5}}"#).unwrap();
        let mut prefs = Prefs::default();
        take_prefs(&theirs, dir.path(), &mut prefs);
        assert_eq!(prefs.device_id.as_deref(), Some("dev-1"));
        assert_eq!(prefs.last_used.get("x"), Some(&5));
        // Ours stands once made.
        std::fs::write(&theirs, r#"{"device_id":"dev-2"}"#).unwrap();
        take_prefs(&theirs, dir.path(), &mut prefs);
        assert_eq!(prefs.device_id.as_deref(), Some("dev-1"));
    }
}
