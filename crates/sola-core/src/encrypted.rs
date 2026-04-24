//! `Encrypted<T>` — a serde newtype that encrypts its payload on
//! human-readable serializers (TOML, JSON) and passes it through
//! untouched on binary serializers (postcard, bincode).
//!
//! This is how sensitive fields travel the bus wire in clear (other
//! processes running as the same user can already read them) but land
//! encrypted on disk in `~/.config/sola/state.toml`.
//!
//! # Key file
//!
//! A single age x25519 identity is generated on first use at
//! `~/.config/sola/key` (mode 0600) and reused for all encrypted
//! fields. Losing the file loses the data; deserialize fails with
//! `DecryptError`, the app sees the field as unset and re-prompts.
//!
//! # Wire format (human-readable)
//!
//! The inner value is serialized to JSON, age-encrypted, base64-
//! encoded, and prefixed with `age1enc:`.

use std::fmt;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::str::FromStr;

use age::secrecy::ExposeSecret;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use serde::de::{self, Deserialize, DeserializeOwned, Deserializer};
use serde::ser::{Serialize, Serializer};

const AGE_PREFIX: &str = "age1enc:";

/// A serde newtype whose payload is encrypted only on human-readable
/// serializers. Binary serializers see the inner value as-is.
pub struct Encrypted<T>(pub T);

impl<T: Clone> Clone for Encrypted<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> fmt::Debug for Encrypted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Encrypted(<redacted>)")
    }
}

impl<T: Serialize> Serialize for Encrypted<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if !s.is_human_readable() {
            return self.0.serialize(s);
        }
        let clear = serde_json::to_vec(&self.0).map_err(serde::ser::Error::custom)?;
        let recipient = load_or_create_identity()
            .map_err(serde::ser::Error::custom)?
            .to_public();
        let cipher = encrypt_bytes(&recipient, &clear).map_err(serde::ser::Error::custom)?;
        let encoded = BASE64_STANDARD.encode(&cipher);
        s.serialize_str(&format!("{AGE_PREFIX}{encoded}"))
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for Encrypted<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if !d.is_human_readable() {
            return T::deserialize(d).map(Encrypted);
        }
        let raw = String::deserialize(d)?;
        let encoded = raw
            .strip_prefix(AGE_PREFIX)
            .ok_or_else(|| de::Error::custom(format!("missing {AGE_PREFIX} prefix")))?;
        let cipher = BASE64_STANDARD
            .decode(encoded)
            .map_err(|e| de::Error::custom(format!("base64 decode failed: {e}")))?;
        let identity = load_or_create_identity().map_err(de::Error::custom)?;
        let clear = decrypt_bytes(&identity, &cipher).map_err(de::Error::custom)?;
        let value: T = serde_json::from_slice(&clear).map_err(de::Error::custom)?;
        Ok(Encrypted(value))
    }
}

#[derive(Debug)]
pub enum EncryptedError {
    Io(std::io::Error),
    KeyParse(String),
    Encrypt(age::EncryptError),
    Decrypt(age::DecryptError),
}

impl fmt::Display for EncryptedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "key file I/O: {e}"),
            Self::KeyParse(e) => write!(f, "key file parse: {e}"),
            Self::Encrypt(e) => write!(f, "encrypt: {e}"),
            Self::Decrypt(e) => write!(f, "decrypt: {e}"),
        }
    }
}

impl std::error::Error for EncryptedError {}

impl From<std::io::Error> for EncryptedError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<age::EncryptError> for EncryptedError {
    fn from(e: age::EncryptError) -> Self {
        Self::Encrypt(e)
    }
}
impl From<age::DecryptError> for EncryptedError {
    fn from(e: age::DecryptError) -> Self {
        Self::Decrypt(e)
    }
}

fn key_path() -> PathBuf {
    crate::config::sola_config_dir().join("key")
}

/// Read the identity from disk, or generate+persist one on first use.
///
/// Not cached: bus-side encrypt/decrypt is rare enough that a disk read
/// per call is cheap, and avoiding a process-wide cache keeps tests
/// simple.
fn load_or_create_identity() -> Result<age::x25519::Identity, EncryptedError> {
    let path = key_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => age::x25519::Identity::from_str(s.trim())
            .map_err(|e| EncryptedError::KeyParse(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let fresh = age::x25519::Identity::generate();
            let secret = fresh.to_string();
            write_key_file(&path, secret.expose_secret())?;
            Ok(fresh)
        }
        Err(e) => Err(EncryptedError::Io(e)),
    }
}

fn write_key_file(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(contents.as_bytes())?;
    f.write_all(b"\n")?;
    Ok(())
}

fn encrypt_bytes(
    recipient: &age::x25519::Recipient,
    clear: &[u8],
) -> Result<Vec<u8>, EncryptedError> {
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(recipient as &dyn age::Recipient))?;
    let mut out = Vec::new();
    let mut writer = encryptor.wrap_output(&mut out)?;
    writer.write_all(clear)?;
    writer.finish()?;
    Ok(out)
}

fn decrypt_bytes(
    identity: &age::x25519::Identity,
    cipher: &[u8],
) -> Result<Vec<u8>, EncryptedError> {
    let decryptor = age::Decryptor::new(cipher)?;
    let mut reader = decryptor.decrypt(std::iter::once(identity as &dyn age::Identity))?;
    let mut clear = Vec::new();
    reader.read_to_end(&mut clear)?;
    Ok(clear)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Creds {
        user: String,
        pass: Encrypted<String>,
    }

    #[test]
    fn age_round_trips_clear() {
        let id = age::x25519::Identity::generate();
        let ct = encrypt_bytes(&id.to_public(), b"the secret").unwrap();
        assert_ne!(&ct[..], b"the secret");
        let back = decrypt_bytes(&id, &ct).unwrap();
        assert_eq!(back, b"the secret");
    }

    #[test]
    fn age_wrong_key_fails() {
        let id1 = age::x25519::Identity::generate();
        let id2 = age::x25519::Identity::generate();
        let ct = encrypt_bytes(&id1.to_public(), b"x").unwrap();
        assert!(decrypt_bytes(&id2, &ct).is_err());
    }

    #[test]
    fn postcard_passes_through_clear() {
        // postcard reports is_human_readable() == false, so the inner
        // value travels as-is. Round-trip must succeed without a key
        // file anywhere on disk (proves the encryption path is skipped).
        let creds = Creds {
            user: "alice".into(),
            pass: Encrypted("hunter2".to_string()),
        };
        let bytes = postcard::to_allocvec(&creds).unwrap();
        // The cleartext must appear verbatim in the wire bytes.
        assert!(
            bytes.windows(7).any(|w| w == b"hunter2"),
            "expected cleartext in postcard bytes"
        );
        let back: Creds = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(back, creds);
    }

    #[test]
    fn missing_prefix_errors() {
        let toml_input = r#"user = "alice"
pass = "not-age-prefixed""#;
        let err = toml::from_str::<Creds>(toml_input).unwrap_err();
        assert!(
            err.to_string().contains(AGE_PREFIX),
            "expected prefix mention in error, got: {err}"
        );
    }

    impl PartialEq for Encrypted<String> {
        fn eq(&self, other: &Self) -> bool {
            self.0 == other.0
        }
    }
}
