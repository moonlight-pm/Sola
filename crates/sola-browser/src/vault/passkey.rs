//! Bitwarden passkey (FIDO2) assertion for WebAuthn intercept.
//!
//! Uses `bitwarden-fido::Fido2Client` (WebAuthn client path) with vault
//! credentials. The page polyfill rebuilds a `PublicKeyCredential` from the
//! returned base64url fields.

use std::sync::Arc;

use bitwarden_fido::{
    CheckUserOptions, CheckUserResult, ClientData, ClientFido2Ext, Fido2CallbackError,
    Fido2CredentialStore, Fido2UserInterface, Origin,
};
use bitwarden_vault::{
    CipherListView, CipherType, CipherView, EncryptionContext, Fido2CredentialNewView,
};
use serde::Serialize;

use super::client::{VaultError, VaultService};

/// One passkey the user can pick for a WebAuthn get() (no secrets).
#[derive(Debug, Clone)]
pub struct PasskeyCandidate {
    pub cipher_id: String,
    pub name: String,
    pub username: Option<String>,
    pub rp_id: String,
    pub user_display_name: Option<String>,
}

struct AutoUserInterface {
    preferred_cipher_id: Option<String>,
}

#[async_trait::async_trait]
impl Fido2UserInterface for AutoUserInterface {
    async fn check_user<'a>(
        &self,
        _options: CheckUserOptions,
        _hint: bitwarden_fido::UiHint<'a, CipherView>,
    ) -> Result<CheckUserResult, Fido2CallbackError> {
        Ok(CheckUserResult {
            user_present: true,
            user_verified: true,
        })
    }

    async fn pick_credential_for_authentication(
        &self,
        available_credentials: Vec<CipherView>,
    ) -> Result<CipherView, Fido2CallbackError> {
        if available_credentials.is_empty() {
            return Err(Fido2CallbackError::OperationCancelled);
        }
        if let Some(ref want) = self.preferred_cipher_id {
            if let Some(c) = available_credentials.iter().find(|c| {
                c.id.map(|id| id.to_string().as_str() == want.as_str())
                    .unwrap_or(false)
            }) {
                return Ok(c.clone());
            }
        }
        Ok(available_credentials[0].clone())
    }

    async fn check_user_and_pick_credential_for_creation(
        &self,
        _options: CheckUserOptions,
        _new_credential: Fido2CredentialNewView,
    ) -> Result<(CipherView, CheckUserResult), Fido2CallbackError> {
        Err(Fido2CallbackError::Unknown(
            "passkey registration is not supported yet".into(),
        ))
    }

    fn is_verification_enabled(&self) -> bool {
        true
    }
}

struct VaultCredentialStore {
    ciphers: Arc<Vec<CipherView>>,
}

#[async_trait::async_trait]
impl Fido2CredentialStore for VaultCredentialStore {
    async fn find_credentials(
        &self,
        _ids: Option<Vec<Vec<u8>>>,
        _rip_id: String,
        _user_handle: Option<Vec<u8>>,
    ) -> Result<Vec<CipherView>, Fido2CallbackError> {
        let mut out = Vec::new();
        for cipher in self.ciphers.iter() {
            if cipher.r#type != CipherType::Login || cipher.deleted_date.is_some() {
                continue;
            }
            let Some(login) = cipher.login.as_ref() else {
                continue;
            };
            if login
                .fido2_credentials
                .as_ref()
                .map(|c| c.is_empty())
                .unwrap_or(true)
            {
                continue;
            }
            out.push(cipher.clone());
        }
        Ok(out)
    }

    async fn all_credentials(&self) -> Result<Vec<CipherListView>, Fido2CallbackError> {
        Ok(Vec::new())
    }

    async fn save_credential(&self, _cred: EncryptionContext) -> Result<(), Fido2CallbackError> {
        Err(Fido2CallbackError::Unknown(
            "passkey registration is not supported yet".into(),
        ))
    }
}

/// Wire form returned to the page polyfill (base64url fields).
///
/// Field names must match WebAuthn / our polyfill (`clientDataJSON` with
/// capital JSON — plain camelCase would emit `clientDataJson` and the page
/// would assemble an empty clientData buffer).
#[derive(Debug, Serialize)]
pub struct PasskeyAssertionJson {
    pub id: String,
    #[serde(rename = "rawId")]
    pub raw_id: String,
    #[serde(rename = "clientDataJSON")]
    pub client_data_json: String,
    #[serde(rename = "authenticatorData")]
    pub authenticator_data: String,
    pub signature: String,
    /// Omitted when empty — Google rejects empty userHandle ArrayBuffers.
    #[serde(rename = "userHandle", skip_serializing_if = "Option::is_none")]
    pub user_handle: Option<String>,
}

fn b64url(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | (data[i + 2] as u32);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
        out.push(T[(n & 63) as usize] as char);
        i += 3;
    }
    if i < data.len() {
        let rem = data.len() - i;
        let n = if rem == 1 {
            (data[i] as u32) << 16
        } else {
            ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8)
        };
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if rem == 2 {
            out.push(T[((n >> 6) & 63) as usize] as char);
        }
    }
    out
}

/// List passkeys that can answer a WebAuthn get() for `rp_id`.
pub async fn list_candidates(
    svc: &VaultService,
    rp_id: &str,
) -> Result<Vec<PasskeyCandidate>, VaultError> {
    if !svc.is_ready_for_passkey() {
        return Err(VaultError::Locked);
    }

    let listed = svc
        .client
        .vault()
        .ciphers()
        .get_all()
        .await
        .map_err(|e| VaultError::Other(e.to_string()))?;

    let fido = svc.client.0.fido2();
    let mut out = Vec::new();
    let rp_lower = rp_id.to_ascii_lowercase();

    for cipher in listed.successes {
        if cipher.r#type != CipherType::Login || cipher.deleted_date.is_some() {
            continue;
        }
        let Some(login) = cipher.login.as_ref() else {
            continue;
        };
        if login
            .fido2_credentials
            .as_ref()
            .map(|c| c.is_empty())
            .unwrap_or(true)
        {
            continue;
        }
        let cipher_id = match cipher.id {
            Some(id) => id.to_string(),
            None => continue,
        };
        let views = match fido.decrypt_fido2_autofill_credentials(cipher.clone()) {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!(error = %e, cipher_id = %cipher_id, "passkey: decrypt autofill skip");
                continue;
            }
        };
        for v in views {
            let cand_rp = v.rp_id.to_ascii_lowercase();
            // Exact or suffix match (rp_id "google.com" vs "accounts.google.com").
            let rp_ok = cand_rp == rp_lower
                || rp_lower.ends_with(&format!(".{cand_rp}"))
                || cand_rp.ends_with(&format!(".{rp_lower}"));
            if !rp_ok && !rp_id.is_empty() {
                continue;
            }
            out.push(PasskeyCandidate {
                cipher_id: cipher_id.clone(),
                name: cipher.name.clone(),
                username: login.username.clone().or(v.user_name_for_ui.clone()),
                rp_id: v.rp_id.clone(),
                user_display_name: v.user_name_for_ui.clone(),
            });
            // One row per cipher is enough for the picker.
            break;
        }
    }

    // Stable sort by name.
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

/// Authenticate with a vault passkey.
///
/// `public_key_json` is the serialized `publicKey` options from the page
/// (challenge / allowCredentials already base64url strings).
pub async fn authenticate(
    svc: &VaultService,
    origin: &str,
    public_key_json: &str,
    preferred_cipher_id: Option<String>,
) -> Result<PasskeyAssertionJson, VaultError> {
    if !svc.is_ready_for_passkey() {
        return Err(VaultError::Locked);
    }

    let listed = svc
        .client
        .vault()
        .ciphers()
        .get_all()
        .await
        .map_err(|e| VaultError::Other(e.to_string()))?;

    let ciphers: Arc<Vec<CipherView>> = Arc::new(listed.successes);
    let store = VaultCredentialStore {
        ciphers: ciphers.clone(),
    };
    let ui = AutoUserInterface {
        preferred_cipher_id,
    };

    let fido = svc.client.0.fido2();
    let mut client = fido.create_client(&ui, &store);

    // CredentialRequestOptions shape expected by passkey-rs.
    let request = format!(r#"{{"publicKey":{public_key_json}}}"#);

    // Web RPs (Google) reject clientDataJSON that includes
    // `androidPackageName`. `DefaultWithExtraData` always flattens that
    // field even when empty. Use CustomHash with a hash of the *same*
    // clean CollectedClientData JSON that Fido2Client will emit when
    // extra=None (CustomHash path), so signature and returned JSON match.
    let client_data = web_client_data_for_request(origin, public_key_json)?;

    let result = client
        .authenticate(Origin::Web(origin.to_string()), request, client_data)
        .await
        .map_err(|e| VaultError::Other(format!("passkey authenticate failed: {e}")))?;

    let client_data_str = String::from_utf8_lossy(&result.response.client_data_json);
    // Must return this exact clientDataJSON — signature covers its SHA-256.
    if client_data_str.contains("androidPackageName") {
        tracing::error!(
            %client_data_str,
            "passkey clientDataJSON still has androidPackageName — Google will reject"
        );
    }
    tracing::info!(
        %client_data_str,
        raw_id_len = result.raw_id.len(),
        sig_len = result.response.signature.len(),
        auth_data_len = result.response.authenticator_data.len(),
        user_handle_len = result.response.user_handle.len(),
        cred_id = %result.id,
        "vault: passkey assertion detail"
    );

    let user_handle = if result.response.user_handle.is_empty() {
        None
    } else {
        Some(b64url(&result.response.user_handle))
    };

    Ok(PasskeyAssertionJson {
        id: result.id,
        raw_id: b64url(&result.raw_id),
        client_data_json: b64url(&result.response.client_data_json),
        authenticator_data: b64url(&result.response.authenticator_data),
        signature: b64url(&result.response.signature),
        user_handle,
    })
}

/// Build `ClientData::DefaultWithCustomHash` matching clean web clientDataJSON.
///
/// Mirrors passkey-client's CollectedClientData serialization:
/// `{"type":"webauthn.get","challenge":…,"origin":…,"crossOrigin":false}`
fn web_client_data_for_request(
    origin: &str,
    public_key_json: &str,
) -> Result<ClientData, VaultError> {
    let pk: serde_json::Value = serde_json::from_str(public_key_json)
        .map_err(|e| VaultError::Other(format!("publicKey JSON: {e}")))?;
    let challenge_b64 = pk
        .get("challenge")
        .and_then(|c| c.as_str())
        .ok_or_else(|| VaultError::Other("publicKey.challenge missing".into()))?;

    // Re-decode/re-encode so we match passkey-rs `encoding::base64url` (nopad).
    let challenge_bytes = b64url_decode(challenge_b64)
        .ok_or_else(|| VaultError::Other("publicKey.challenge not base64url".into()))?;
    let challenge_canon = b64url(&challenge_bytes);

    // Origin::Web trims trailing `/` for Display.
    let origin_clean = origin.trim_end_matches('/');

    // Field order must match CollectedClientData Serialize (type, challenge,
    // origin, crossOrigin). truthiness() always emits crossOrigin as bool.
    let client_data_json = format!(
        r#"{{"type":"webauthn.get","challenge":"{challenge_canon}","origin":"{origin_clean}","crossOrigin":false}}"#
    );
    let hash = {
        use sha2::{Digest, Sha256};
        Sha256::digest(client_data_json.as_bytes()).to_vec()
    };
    tracing::debug!(%client_data_json, "vault: passkey expected clientDataJSON");
    Ok(ClientData::DefaultWithCustomHash { hash })
}

fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    let mut s = s.replace('-', "+").replace('_', "/");
    while s.len() % 4 != 0 {
        s.push('=');
    }
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            b'=' => Some(0),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut i = 0;
    while i + 4 <= bytes.len() {
        let a = val(bytes[i])?;
        let b = val(bytes[i + 1])?;
        let c = val(bytes[i + 2])?;
        let d = val(bytes[i + 3])?;
        let n = ((a as u32) << 18) | ((b as u32) << 12) | ((c as u32) << 6) | (d as u32);
        out.push(((n >> 16) & 0xff) as u8);
        if bytes[i + 2] != b'=' {
            out.push(((n >> 8) & 0xff) as u8);
        }
        if bytes[i + 3] != b'=' {
            out.push((n & 0xff) as u8);
        }
        i += 4;
    }
    Some(out)
}
