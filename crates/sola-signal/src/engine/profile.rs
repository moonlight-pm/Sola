//! Profiles: the name someone gave themselves, kept on the server encrypted
//! under their profile key.

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use libsignal_core::Aci;
use serde::Deserialize;
use signal_crypto::Aes256GcmDecryption;
use zkgroup::profiles::ProfileKey;

use super::Wire;
use crate::net;

fn key(profile_key: &[u8]) -> Option<ProfileKey> {
    Some(ProfileKey::create(profile_key.try_into().ok()?))
}

/// What lets us send to someone without the server learning it is us: a
/// key anyone holding their profile key can derive.
pub fn access_key(profile_key: &[u8]) -> Option<[u8; 16]> {
    Some(key(profile_key)?.derive_access_key())
}

#[derive(Deserialize)]
struct Profile {
    name: Option<String>,
}

/// Someone's profile name. `None`: they have no profile under this key.
pub async fn name(wire: &Wire, aci: Aci, profile_key: &[u8]) -> Result<Option<String>> {
    let key = key(profile_key).context("a profile key that is not 32 bytes")?;
    let version: String = key.get_profile_key_version(aci).bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let path = format!("/v1/profile/{}/{version}", aci.service_id_string());
    let access = (
        http::HeaderName::from_static("unidentified-access-key"),
        http::HeaderValue::from_str(&BASE64.encode(key.derive_access_key()))?,
    );
    // Asked without saying who asks; signed in only if they do not allow that.
    let response = match net::request(&wire.unauth, http::Method::GET, &path, Some(access), None).await {
        Ok(response) => response,
        Err(e) if e.status() == Some(http::StatusCode::UNAUTHORIZED) => {
            match net::request(&wire.chat, http::Method::GET, &path, None, None).await {
                Ok(response) => response,
                Err(e) if e.status() == Some(http::StatusCode::NOT_FOUND) => return Ok(None),
                Err(e) => return Err(e.into()),
            }
        }
        Err(e) if e.status() == Some(http::StatusCode::NOT_FOUND) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let profile: Profile = serde_json::from_slice(response.body.as_deref().unwrap_or_default()).context("profile")?;
    let Some(name) = profile.name.filter(|name| !name.is_empty()) else { return Ok(None) };
    let padded = decrypt(profile_key, &BASE64.decode(name)?)?;
    let end = padded.iter().rposition(|byte| *byte != 0).map_or(0, |last| last + 1);
    let name = String::from_utf8_lossy(&padded[..end]);
    // Given name, then family name after a zero byte.
    let name = name.split('\0').filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ");
    Ok((!name.is_empty()).then_some(name))
}

/// AES-256-GCM as profiles use it: a 12-byte nonce, the ciphertext, a
/// 16-byte tag.
pub fn decrypt(profile_key: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
    if sealed.len() < 12 + 16 {
        return Err(anyhow!("a profile field too short to be encrypted"));
    }
    let (nonce, rest) = sealed.split_at(12);
    let (ciphertext, tag) = rest.split_at(rest.len() - 16);
    let mut cipher = Aes256GcmDecryption::new(profile_key, nonce, &[]).map_err(|e| anyhow!("profile: {e}"))?;
    let mut plaintext = ciphertext.to_vec();
    cipher.decrypt(&mut plaintext);
    cipher.verify_tag(tag).map_err(|e| anyhow!("profile: {e}"))?;
    Ok(plaintext)
}
