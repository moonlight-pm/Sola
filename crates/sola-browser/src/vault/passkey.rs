//! Bitwarden passkey (FIDO2) assertion and registration for WebAuthn intercept.
//!
//! Uses `bitwarden-fido::Fido2Client` (WebAuthn client path) with vault
//! credentials. The page polyfill rebuilds a `PublicKeyCredential` from the
//! returned base64url fields.

use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bitwarden_core::key_management::KeySlotIds;
use bitwarden_crypto::KeyStore;
use bitwarden_fido::{
    CheckUserOptions, CheckUserResult, ClientData, ClientFido2Ext, Fido2CallbackError,
    Fido2CredentialStore, Fido2UserInterface, Origin, UiHint, string_to_guid_bytes,
};
use bitwarden_vault::{
    CipherListView, CipherType, CipherView, EncryptionContext, Fido2CredentialNewView,
    Fido2CredentialView,
};
use serde::Serialize;

use super::client::{VaultError, VaultService, new_login_view};
use super::match_uri::apex_domain;

/// One passkey the user can pick for a WebAuthn get() (no secrets).
#[derive(Debug, Clone)]
pub struct PasskeyCandidate {
    pub cipher_id: String,
    pub name: String,
    pub username: Option<String>,
    pub rp_id: String,
    pub user_display_name: Option<String>,
    /// Bitwarden "master password re-prompt" on this login.
    pub reprompt: bool,
}

struct AutoUserInterface {
    preferred_cipher_id: Option<String>,
    /// New login to attach a created passkey to (no id yet).
    new_cipher: Option<CipherView>,
    ciphers: Arc<Vec<CipherView>>,
}

fn cipher_id_eq(cipher: &CipherView, want: &str) -> bool {
    cipher.id.map(|id| id.to_string() == want).unwrap_or(false)
}

#[async_trait::async_trait]
impl Fido2UserInterface for AutoUserInterface {
    async fn check_user<'a>(
        &self,
        _options: CheckUserOptions,
        hint: UiHint<'a, CipherView>,
    ) -> Result<CheckUserResult, Fido2CallbackError> {
        if matches!(hint, UiHint::InformExcludedCredentialFound(_)) {
            return Err(Fido2CallbackError::Unknown(
                "A passkey for this account is already saved.".into(),
            ));
        }
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
            if let Some(c) = available_credentials.iter().find(|c| cipher_id_eq(c, want)) {
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
        let approved = CheckUserResult {
            user_present: true,
            user_verified: true,
        };
        if let Some(ref want) = self.preferred_cipher_id {
            if let Some(c) = self.ciphers.iter().find(|c| cipher_id_eq(c, want)) {
                return Ok((c.clone(), approved));
            }
            return Err(Fido2CallbackError::Unknown(
                "Selected login was not found in the vault.".into(),
            ));
        }
        if let Some(ref new_cipher) = self.new_cipher {
            return Ok((new_cipher.clone(), approved));
        }
        Err(Fido2CallbackError::Unknown(
            "No login selected for the new passkey.".into(),
        ))
    }

    fn is_verification_enabled(&self) -> bool {
        true
    }
}

struct VaultCredentialStore {
    ciphers: Arc<Vec<CipherView>>,
    keys: KeyStore<KeySlotIds>,
    saved: Mutex<Option<EncryptionContext>>,
}

#[async_trait::async_trait]
impl Fido2CredentialStore for VaultCredentialStore {
    /// passkey-rs leaves filtering to the store: get() passes its
    /// allowCredentials, create() its excludeCredentials (any match there
    /// refuses the create).
    async fn find_credentials(
        &self,
        ids: Option<Vec<Vec<u8>>>,
        rp_id: String,
        _user_handle: Option<Vec<u8>>,
    ) -> Result<Vec<CipherView>, Fido2CallbackError> {
        Ok(self
            .ciphers
            .iter()
            .filter(|c| matching_passkey(&self.keys, c, &rp_id, ids.as_deref()).is_some())
            .cloned()
            .collect())
    }

    async fn all_credentials(&self) -> Result<Vec<CipherListView>, Fido2CallbackError> {
        Ok(Vec::new())
    }

    async fn save_credential(&self, cred: EncryptionContext) -> Result<(), Fido2CallbackError> {
        *self.saved.lock().unwrap() = Some(cred);
        Ok(())
    }
}

/// A login in use (not trashed or archived) that carries a passkey.
fn holds_passkey(cipher: &CipherView) -> bool {
    cipher.r#type == CipherType::Login
        && cipher.deleted_date.is_none()
        && cipher.archived_date.is_none()
        && cipher
            .login
            .as_ref()
            .and_then(|l| l.fido2_credentials.as_ref())
            .is_some_and(|c| !c.is_empty())
}

/// The passkey `cipher` signs with (the SDK uses its first) when it is
/// bound to exactly `rp_id` and, given `ids`, is one of them.
fn matching_passkey(
    keys: &KeyStore<KeySlotIds>,
    cipher: &CipherView,
    rp_id: &str,
    ids: Option<&[Vec<u8>]>,
) -> Option<Fido2CredentialView> {
    if !holds_passkey(cipher) {
        return None;
    }
    let cred = match cipher.decrypt_fido2_credentials(&mut keys.context()) {
        Ok(creds) => creds.into_iter().next()?,
        Err(e) => {
            tracing::debug!(error = %e, cipher_id = ?cipher.id, "passkey: decrypt skip");
            return None;
        }
    };
    if cred.rp_id != rp_id {
        return None;
    }
    if let Some(ids) = ids {
        let id = string_to_guid_bytes(&cred.credential_id).ok()?;
        if !ids.contains(&id) {
            return None;
        }
    }
    Some(cred)
}

/// Credential ids in a get()'s `allowCredentials`; `None` when absent or
/// empty (any credential for the RP may answer).
fn allow_credential_ids(pk: &serde_json::Value) -> Option<Vec<Vec<u8>>> {
    let list = pk.get("allowCredentials")?.as_array()?;
    if list.is_empty() {
        return None;
    }
    Some(
        list.iter()
            .filter_map(|c| c.get("id")?.as_str())
            .filter_map(b64url_decode)
            .collect(),
    )
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

/// Wire form returned to the page polyfill after `credentials.create`.
#[derive(Debug, Serialize)]
pub struct PasskeyAttestationJson {
    pub id: String,
    #[serde(rename = "rawId")]
    pub raw_id: String,
    #[serde(rename = "clientDataJSON")]
    pub client_data_json: String,
    #[serde(rename = "authenticatorData")]
    pub authenticator_data: String,
    #[serde(rename = "attestationObject")]
    pub attestation_object: String,
    #[serde(rename = "publicKey", skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    #[serde(rename = "publicKeyAlgorithm")]
    pub public_key_algorithm: i64,
    pub transports: Vec<String>,
}

/// Display name / username from a create() publicKey JSON (for the confirm card).
pub fn create_account_hint(public_key_json: &str) -> Option<String> {
    let pk: serde_json::Value = serde_json::from_str(public_key_json).ok()?;
    let user = pk.get("user")?;
    let display = user
        .get("displayName")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let name = user
        .get("name")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    display.or(name).map(|s| s.to_string())
}

fn create_user_name(public_key_json: &str) -> Option<String> {
    let pk: serde_json::Value = serde_json::from_str(public_key_json).ok()?;
    pk.get("user")?
        .get("name")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn create_rp_name(public_key_json: &str) -> Option<String> {
    let pk: serde_json::Value = serde_json::from_str(public_key_json).ok()?;
    pk.get("rp")?
        .get("name")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn b64url(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

/// Decode a byte string from WebAuthn JSON: base64url, tolerating `=`
/// padding and standard base64 (`+/`), both of which passkey-rs accepts.
fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=').replace('+', "-").replace('/', "_");
    URL_SAFE_NO_PAD.decode(s).ok()
}

/// List passkeys that can answer a WebAuthn get() for `rp_id`.
///
/// Same filter as the credential store `authenticate` signs from: exact
/// `rp_id`, and the options' `allowCredentials` when it is non-empty.
pub async fn list_candidates(
    svc: &VaultService,
    rp_id: &str,
    public_key_json: &str,
) -> Result<Vec<PasskeyCandidate>, VaultError> {
    if !svc.is_ready_for_passkey() {
        return Err(VaultError::Locked);
    }

    let pk: serde_json::Value = serde_json::from_str(public_key_json)
        .map_err(|e| VaultError::Other(format!("publicKey JSON: {e}")))?;
    let allow = allow_credential_ids(&pk);

    let listed = svc
        .client
        .vault()
        .ciphers()
        .get_all()
        .await
        .map_err(|e| VaultError::Other(e.to_string()))?;

    let keys = svc.client.0.internal.get_key_store();
    let mut out = Vec::new();

    for cipher in listed.successes {
        let Some(cred) = matching_passkey(keys, &cipher, rp_id, allow.as_deref()) else {
            continue;
        };
        let Some(cipher_id) = cipher.id.map(|id| id.to_string()) else {
            continue;
        };
        let user = [cred.user_name, cred.user_display_name]
            .into_iter()
            .flatten()
            .find(|s| !s.trim().is_empty());
        out.push(PasskeyCandidate {
            cipher_id,
            username: cipher
                .login
                .as_ref()
                .and_then(|l| l.username.clone())
                .or(user.clone()),
            user_display_name: user,
            rp_id: cred.rp_id,
            reprompt: cipher.reprompt == bitwarden_vault::CipherRepromptType::Password,
            name: cipher.name,
        });
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
        keys: svc.client.0.internal.get_key_store().clone(),
        saved: Mutex::new(None),
    };
    let ui = AutoUserInterface {
        preferred_cipher_id,
        new_cipher: None,
        ciphers: ciphers.clone(),
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
    let client_data = web_client_data(origin, public_key_json, "webauthn.get")?;

    // `get_all` above must stay at security-state 1. `authenticate`
    // encrypts the updated cipher internally.
    let result = {
        let _blob = super::client::BlobCipherWrites::enter(&svc.client.0);
        client
            .authenticate(Origin::Web(origin.to_string()), request, client_data)
            .await
    }
    .map_err(|e| VaultError::Other(format!("passkey authenticate failed: {e}")))?;

    if let Some(ctx) = store.saved.lock().unwrap().take() {
        if let Err(e) = svc.persist_encryption_context(ctx).await {
            tracing::warn!(error = %e, "vault: passkey counter persist failed");
        }
    }

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

/// Register a new vault passkey (`navigator.credentials.create`).
///
/// `preferred_cipher_id` attaches the credential to an existing login.
/// `None` creates a new personal login named after the site.
///
/// Returns the attestation for the page and the encrypted cipher to persist.
pub async fn register(
    svc: &VaultService,
    origin: &str,
    public_key_json: &str,
    preferred_cipher_id: Option<String>,
) -> Result<(PasskeyAttestationJson, EncryptionContext), VaultError> {
    if !svc.is_ready_for_passkey() {
        return Err(VaultError::Locked);
    }

    if let Err(e) = svc.sync().await {
        tracing::warn!(error = %e, "vault: sync before passkey register failed");
    }

    let listed = svc
        .client
        .vault()
        .ciphers()
        .get_all()
        .await
        .map_err(|e| VaultError::Other(e.to_string()))?;

    let ciphers: Arc<Vec<CipherView>> = Arc::new(listed.successes);
    let new_cipher = if preferred_cipher_id.is_none() {
        Some(new_login_for_passkey(origin, public_key_json))
    } else {
        None
    };
    let store = VaultCredentialStore {
        ciphers: ciphers.clone(),
        keys: svc.client.0.internal.get_key_store().clone(),
        saved: Mutex::new(None),
    };
    let ui = AutoUserInterface {
        preferred_cipher_id,
        new_cipher,
        ciphers,
    };

    let fido = svc.client.0.fido2();
    let mut client = fido.create_client(&ui, &store);

    let request = format!(r#"{{"publicKey":{public_key_json}}}"#);
    let client_data = web_client_data(origin, public_key_json, "webauthn.create")?;

    // `get_all` above must stay at security-state 1. `register` encrypts
    // the new or updated cipher internally.
    let result = {
        let _blob = super::client::BlobCipherWrites::enter(&svc.client.0);
        client
            .register(Origin::Web(origin.to_string()), request, client_data)
            .await
    }
    .map_err(|e| VaultError::Other(format!("passkey register failed: {e}")))?;

    let saved = store
        .saved
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| VaultError::Other("passkey register did not produce a cipher".into()))?;

    let client_data_str = String::from_utf8_lossy(&result.response.client_data_json);
    if client_data_str.contains("androidPackageName") {
        tracing::error!(
            %client_data_str,
            "passkey create clientDataJSON still has androidPackageName"
        );
    }
    tracing::info!(
        %client_data_str,
        raw_id_len = result.raw_id.len(),
        att_len = result.response.attestation_object.len(),
        auth_data_len = result.response.authenticator_data.len(),
        cred_id = %result.id,
        "vault: passkey attestation detail"
    );

    let attestation = PasskeyAttestationJson {
        id: result.id,
        raw_id: b64url(&result.raw_id),
        client_data_json: b64url(&result.response.client_data_json),
        authenticator_data: b64url(&result.response.authenticator_data),
        attestation_object: b64url(&result.response.attestation_object),
        public_key: result.response.public_key.as_ref().map(|k| b64url(k)),
        public_key_algorithm: result.response.public_key_algorithm,
        transports: result
            .response
            .transports
            .unwrap_or_else(|| vec!["internal".into()]),
    };
    Ok((attestation, saved))
}

fn new_login_for_passkey(origin: &str, public_key_json: &str) -> CipherView {
    let apex = apex_domain(origin);
    let host = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
        .and_then(|s| s.split('/').next())
        .unwrap_or("")
        .to_string();
    let name = if !apex.is_empty() {
        apex.clone()
    } else if !host.is_empty() {
        host
    } else {
        create_rp_name(public_key_json).unwrap_or_else(|| "Passkey".into())
    };
    let uri = if apex.is_empty() { None } else { Some(apex) };
    new_login_view(name, create_user_name(public_key_json), None, uri)
}

/// Build `ClientData::DefaultWithCustomHash` matching clean web clientDataJSON.
fn web_client_data(
    origin: &str,
    public_key_json: &str,
    ty: &str,
) -> Result<ClientData, VaultError> {
    let client_data_json = web_client_data_json(origin, public_key_json, ty)?;
    let hash = {
        use sha2::{Digest, Sha256};
        Sha256::digest(client_data_json.as_bytes()).to_vec()
    };
    tracing::debug!(%client_data_json, "vault: passkey expected clientDataJSON");
    Ok(ClientData::DefaultWithCustomHash { hash })
}

/// The clientDataJSON passkey-client emits for a web origin.
///
/// Mirrors its CollectedClientData serialization:
/// `{"type":"webauthn.get|create","challenge":…,"origin":…,"crossOrigin":false}`
fn web_client_data_json(
    origin: &str,
    public_key_json: &str,
    ty: &str,
) -> Result<String, VaultError> {
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
    // Values go through serde_json, as there, so escaping matches too.
    let quote = |s: &str| {
        serde_json::to_string(s).map_err(|e| VaultError::Other(format!("clientDataJSON: {e}")))
    };
    Ok(format!(
        r#"{{"type":{},"challenge":{},"origin":{},"crossOrigin":false}}"#,
        quote(ty)?,
        quote(&challenge_canon)?,
        quote(origin_clean)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_account_hint_prefers_display_name() {
        let pk =
            r#"{"user":{"id":"YWJj","name":"ada@ex.com","displayName":"Ada"},"challenge":"YQ"}"#;
        assert_eq!(create_account_hint(pk).as_deref(), Some("Ada"));
        assert_eq!(create_user_name(pk).as_deref(), Some("ada@ex.com"));
    }

    #[test]
    fn web_client_data_create_type() {
        let pk = r#"{"challenge":"YQ"}"#;
        let data = web_client_data("https://docs.example.com", pk, "webauthn.create").unwrap();
        match data {
            ClientData::DefaultWithCustomHash { hash } => {
                assert_eq!(hash.len(), 32);
            }
            _ => panic!("expected DefaultWithCustomHash"),
        }
    }

    #[test]
    fn web_client_data_json_keeps_order_and_escapes() {
        let pk = r#"{"challenge":"YQ=="}"#;
        let json = web_client_data_json("https://docs.example.com/", pk, "webauthn.get").unwrap();
        assert_eq!(
            json,
            r#"{"type":"webauthn.get","challenge":"YQ","origin":"https://docs.example.com","crossOrigin":false}"#
        );

        let json = web_client_data_json(r#"https://a"b\c.example"#, pk, "webauthn.get").unwrap();
        assert!(json.contains(r#""origin":"https://a\"b\\c.example","#));
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["origin"], r#"https://a"b\c.example"#);
    }

    #[test]
    fn b64url_round_trips_without_padding() {
        assert_eq!(b64url(&[0xfb, 0xff]), "-_8");
        assert_eq!(b64url(b"a"), "YQ");
        assert_eq!(b64url_decode("-_8").as_deref(), Some(&[0xfb, 0xff][..]));
    }

    #[test]
    fn b64url_decode_tolerates_padding_and_standard_alphabet() {
        assert_eq!(b64url_decode("YQ==").as_deref(), Some(&b"a"[..]));
        assert_eq!(b64url_decode("+/8=").as_deref(), Some(&[0xfb, 0xff][..]));
        assert_eq!(b64url_decode("").as_deref(), Some(&[][..]));
    }

    #[test]
    fn b64url_decode_rejects_inner_padding_and_junk() {
        assert_eq!(b64url_decode("YQ==YQ"), None);
        assert_eq!(b64url_decode("Y=Q"), None);
        assert_eq!(b64url_decode("Y!Q"), None);
    }

    #[test]
    fn allow_credential_ids_reads_non_empty_list() {
        let pk = serde_json::json!({
            "challenge": "YQ",
            "allowCredentials": [
                {"type": "public-key", "id": "-_8"},
                {"type": "public-key", "id": "+/8="},
                {"type": "public-key", "id": null},
            ],
        });
        assert_eq!(
            allow_credential_ids(&pk),
            Some(vec![vec![0xfb, 0xff], vec![0xfb, 0xff]])
        );
        assert_eq!(
            allow_credential_ids(&serde_json::json!({"allowCredentials": []})),
            None
        );
        assert_eq!(
            allow_credential_ids(&serde_json::json!({"challenge": "YQ"})),
            None
        );
    }

    mod store {
        use bitwarden_core::key_management::SymmetricKeySlotId;
        use bitwarden_crypto::PrimitiveEncryptable;
        use bitwarden_vault::Fido2Credential;

        use super::*;

        const ID_A: &str = "a36f3d35-5dae-4d07-8b24-f89e11082090";
        const ID_B: &str = "0b5d6c1e-2f3a-4b7c-8d9e-0f1a2b3c4d5e";
        const ID_C: &str = "c0ffee00-1234-4abc-9def-0123456789ab";
        const ID_UNKNOWN: &str = "deadbeef-0000-4000-8000-000000000000";

        /// A key store holding a fresh user key, as after unlock.
        fn unlocked_keys() -> KeyStore<KeySlotIds> {
            let keys = KeyStore::default();
            {
                let mut ctx = keys.context_mut();
                let local = ctx.generate_symmetric_key();
                ctx.persist_symmetric_key(local, SymmetricKeySlotId::User)
                    .unwrap();
            }
            keys
        }

        /// A personal login holding one passkey, encrypted as the vault does.
        fn passkey_login(
            keys: &KeyStore<KeySlotIds>,
            name: &str,
            rp_id: &str,
            credential_id: &str,
        ) -> CipherView {
            let mut ctx = keys.context();
            let mut enc = |s: &str| s.encrypt(&mut ctx, SymmetricKeySlotId::User).unwrap();
            let cred = Fido2Credential {
                credential_id: enc(credential_id),
                key_type: enc("public-key"),
                key_algorithm: enc("ECDSA"),
                key_curve: enc("P-256"),
                key_value: enc("unused"),
                rp_id: enc(rp_id),
                user_handle: None,
                user_name: None,
                counter: enc("0"),
                rp_name: None,
                user_display_name: None,
                discoverable: enc("true"),
                creation_date: chrono::Utc::now(),
            };
            let mut cipher = new_login_view(name.into(), None, None, None);
            cipher.login.as_mut().unwrap().fido2_credentials = Some(vec![cred]);
            cipher
        }

        fn vault(keys: &KeyStore<KeySlotIds>) -> VaultCredentialStore {
            let mut trashed = passkey_login(keys, "trashed", "example.com", ID_A);
            trashed.deleted_date = Some(chrono::Utc::now());
            let mut archived = passkey_login(keys, "archived", "example.com", ID_A);
            archived.archived_date = Some(chrono::Utc::now());
            VaultCredentialStore {
                ciphers: Arc::new(vec![
                    passkey_login(keys, "a", "example.com", ID_A),
                    passkey_login(keys, "b", "other.com", ID_B),
                    passkey_login(keys, "c", "login.example.com", ID_C),
                    trashed,
                    archived,
                    new_login_view("no passkey".into(), None, None, None),
                ]),
                keys: keys.clone(),
                saved: Mutex::new(None),
            }
        }

        fn guid(id: &str) -> Vec<u8> {
            string_to_guid_bytes(id).unwrap()
        }

        async fn found(
            store: &VaultCredentialStore,
            ids: Option<Vec<Vec<u8>>>,
            rp_id: &str,
        ) -> Vec<String> {
            store
                .find_credentials(ids, rp_id.into(), Some(b"user".to_vec()))
                .await
                .unwrap()
                .into_iter()
                .map(|c| c.name)
                .collect()
        }

        #[tokio::test]
        async fn get_matches_rp_id_exactly() {
            let keys = unlocked_keys();
            let store = vault(&keys);
            assert_eq!(found(&store, None, "example.com").await, ["a"]);
            assert_eq!(found(&store, None, "login.example.com").await, ["c"]);
            assert!(found(&store, None, "www.example.com").await.is_empty());
            assert!(found(&store, None, "com").await.is_empty());
        }

        #[tokio::test]
        async fn get_honours_allow_credentials() {
            let keys = unlocked_keys();
            let store = vault(&keys);
            let allow = Some(vec![guid(ID_UNKNOWN), guid(ID_A)]);
            assert_eq!(found(&store, allow, "example.com").await, ["a"]);
            // A listed id bound to another RP does not answer.
            let allow = Some(vec![guid(ID_B)]);
            assert!(found(&store, allow, "example.com").await.is_empty());
        }

        #[tokio::test]
        async fn exclude_trips_only_on_same_rp_and_listed_id() {
            let keys = unlocked_keys();
            let store = vault(&keys);
            // A passkey saved for another site must not block this create().
            let exclude = Some(vec![guid(ID_B), guid(ID_C), guid(ID_UNKNOWN)]);
            assert!(found(&store, exclude, "example.com").await.is_empty());
            let exclude = Some(vec![guid(ID_UNKNOWN), guid(ID_A)]);
            assert_eq!(found(&store, exclude, "example.com").await, ["a"]);
        }
    }
}
