//! Passkeys (FIDO2): sign a WebAuthn get() or make a create() with the
//! vault's credentials, through `bitwarden-fido`'s WebAuthn client. The
//! browser sends the page's options and the frame's origin; the rp id is
//! checked against that origin here again. The page polyfill rebuilds a
//! `PublicKeyCredential` from the base64url fields returned.

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
    CipherListView, CipherRepromptType, CipherType, CipherView, EncryptionContext,
    Fido2CredentialNewView, Fido2CredentialView,
};
use serde::Serialize;
use sola_vault_proto::{ErrorCode, PasskeyCandidate, PasskeyInfo};

use super::client::BlobCipherWrites;
use super::errors::{Result, err, internal, locked};
use super::match_uri::{apex_domain, log_host};
use super::new_item::new_login_view;
use super::scope::{origin_host, rp_id_allowed};
use super::service::VaultService;

struct AutoUserInterface {
    preferred_cipher_id: Option<String>,
    /// A new login to hold a created passkey (no id yet).
    new_cipher: Option<CipherView>,
    ciphers: Arc<Vec<CipherView>>,
}

fn cipher_id_eq(cipher: &CipherView, want: &str) -> bool {
    cipher.id.is_some_and(|id| id.to_string() == want)
}

#[async_trait::async_trait]
impl Fido2UserInterface for AutoUserInterface {
    async fn check_user<'a>(
        &self,
        _options: CheckUserOptions,
        hint: UiHint<'a, CipherView>,
    ) -> std::result::Result<CheckUserResult, Fido2CallbackError> {
        if matches!(hint, UiHint::InformExcludedCredentialFound(_)) {
            return Err(Fido2CallbackError::Unknown(
                "A passkey for this account is already saved.".into(),
            ));
        }
        // The browser asked the person before it sent the request.
        Ok(CheckUserResult {
            user_present: true,
            user_verified: true,
        })
    }

    async fn pick_credential_for_authentication(
        &self,
        available: Vec<CipherView>,
    ) -> std::result::Result<CipherView, Fido2CallbackError> {
        if let Some(want) = self.preferred_cipher_id.as_deref() {
            if let Some(c) = available.iter().find(|c| cipher_id_eq(c, want)) {
                return Ok(c.clone());
            }
        }
        available
            .into_iter()
            .next()
            .ok_or(Fido2CallbackError::OperationCancelled)
    }

    async fn check_user_and_pick_credential_for_creation(
        &self,
        _options: CheckUserOptions,
        _new_credential: Fido2CredentialNewView,
    ) -> std::result::Result<(CipherView, CheckUserResult), Fido2CallbackError> {
        let approved = CheckUserResult {
            user_present: true,
            user_verified: true,
        };
        if let Some(want) = self.preferred_cipher_id.as_deref() {
            return match self.ciphers.iter().find(|c| cipher_id_eq(c, want)) {
                Some(c) => Ok((c.clone(), approved)),
                None => Err(Fido2CallbackError::Unknown(
                    "The chosen login is not in the vault.".into(),
                )),
            };
        }
        match &self.new_cipher {
            Some(c) => Ok((c.clone(), approved)),
            None => Err(Fido2CallbackError::Unknown("No login chosen for the new passkey.".into())),
        }
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
    ) -> std::result::Result<Vec<CipherView>, Fido2CallbackError> {
        Ok(self
            .ciphers
            .iter()
            .filter(|c| matching_passkey(&self.keys, c, &rp_id, ids.as_deref()).is_some())
            .cloned()
            .collect())
    }

    async fn all_credentials(&self) -> std::result::Result<Vec<CipherListView>, Fido2CallbackError> {
        Ok(Vec::new())
    }

    async fn save_credential(&self, cred: EncryptionContext) -> std::result::Result<(), Fido2CallbackError> {
        *self.saved.lock().expect("saved mutex") = Some(cred);
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

/// The public facts of an item's passkeys, for its record.
pub fn infos(keys: &KeyStore<KeySlotIds>, cipher: &CipherView) -> Vec<PasskeyInfo> {
    let has = cipher
        .login
        .as_ref()
        .and_then(|l| l.fido2_credentials.as_ref())
        .is_some_and(|c| !c.is_empty());
    if !has {
        return Vec::new();
    }
    match cipher.decrypt_fido2_credentials(&mut keys.context()) {
        Ok(creds) => creds
            .into_iter()
            .map(|c| PasskeyInfo {
                rp_id: c.rp_id.clone(),
                user_name: c.user_name.clone(),
                user_display_name: c.user_display_name.clone(),
                created: Some(c.creation_date.to_rfc3339()),
            })
            .collect(),
        Err(e) => {
            tracing::debug!(error = %e, "passkey: facts unreadable");
            Vec::new()
        }
    }
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

/// get(): the page's response. Field names are WebAuthn's
/// (`clientDataJSON` with capital JSON).
#[derive(Debug, Serialize)]
struct AssertionJson {
    id: String,
    #[serde(rename = "rawId")]
    raw_id: String,
    #[serde(rename = "clientDataJSON")]
    client_data_json: String,
    #[serde(rename = "authenticatorData")]
    authenticator_data: String,
    signature: String,
    /// Left out when empty: Google rejects an empty userHandle.
    #[serde(rename = "userHandle", skip_serializing_if = "Option::is_none")]
    user_handle: Option<String>,
}

/// create(): the page's response.
#[derive(Debug, Serialize)]
struct AttestationJson {
    id: String,
    #[serde(rename = "rawId")]
    raw_id: String,
    #[serde(rename = "clientDataJSON")]
    client_data_json: String,
    #[serde(rename = "authenticatorData")]
    authenticator_data: String,
    #[serde(rename = "attestationObject")]
    attestation_object: String,
    #[serde(rename = "publicKey", skip_serializing_if = "Option::is_none")]
    public_key: Option<String>,
    #[serde(rename = "publicKeyAlgorithm")]
    public_key_algorithm: i64,
    transports: Vec<String>,
}

fn pk_json(public_key_json: &str) -> Result<serde_json::Value> {
    serde_json::from_str(public_key_json).map_err(|e| err(ErrorCode::Invalid, format!("publicKey JSON: {e}")))
}

fn json_str<'a>(v: &'a serde_json::Value, path: &str) -> Option<&'a str> {
    v.pointer(path)
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// The rp id the options name, else the origin's host; refused unless
/// the origin may use it.
fn checked_rp_id(origin: &str, named: Option<&str>) -> Result<String> {
    let rp_id = match named {
        Some(rp) => rp.to_ascii_lowercase(),
        None => origin_host(origin).ok_or_else(|| err(ErrorCode::Denied, "This page has no origin."))?,
    };
    if !rp_id_allowed(origin, &rp_id) {
        tracing::warn!(origin = %log_host(origin), %rp_id, "vault: passkey rp id refused for origin");
        return Err(err(ErrorCode::Denied, "This page may not use a passkey for that site."));
    }
    Ok(rp_id)
}

/// The vault's passkeys that can answer a get() for `rp_id`: exact rp id,
/// and the options' `allowCredentials` when it is not empty.
pub async fn candidates(svc: &VaultService, rp_id: &str, public_key_json: &str) -> Result<Vec<PasskeyCandidate>> {
    let pk = pk_json(public_key_json)?;
    let allow = allow_credential_ids(&pk);
    let listed = svc.all().await?;
    let keys = svc.pm().0.internal.get_key_store();
    let mut out = Vec::new();
    for cipher in listed.successes {
        let Some(cred) = matching_passkey(keys, &cipher, rp_id, allow.as_deref()) else {
            continue;
        };
        let Some(cipher_id) = cipher.id.map(|id| id.to_string()) else {
            continue;
        };
        let user = [cred.user_name.clone(), cred.user_display_name.clone()]
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
            rp_id: cred.rp_id.clone(),
            reprompt: cipher.reprompt == CipherRepromptType::Password,
            name: cipher.name,
        });
    }
    out.sort_by_key(|c| c.name.to_lowercase());
    Ok(out)
}

/// Sign a get() with the passkey on `cipher_id`. Returns the response
/// JSON for the page; the counter's change is saved.
pub async fn assert(svc: &VaultService, origin: &str, public_key_json: &str, cipher_id: &str) -> Result<String> {
    let pk = pk_json(public_key_json)?;
    checked_rp_id(origin, json_str(&pk, "/rpId"))?;
    let listed = svc.all().await?;
    let ciphers = Arc::new(listed.successes);
    let store = VaultCredentialStore {
        ciphers: ciphers.clone(),
        keys: svc.pm().0.internal.get_key_store().clone(),
        saved: Mutex::new(None),
    };
    let ui = AutoUserInterface {
        preferred_cipher_id: Some(cipher_id.to_string()),
        new_cipher: None,
        ciphers,
    };
    let fido = svc.pm().0.fido2();
    let mut client = fido.create_client(&ui, &store);
    let request = format!(r#"{{"publicKey":{public_key_json}}}"#);
    // Web RPs (Google) reject a clientDataJSON with `androidPackageName`,
    // which `DefaultWithExtraData` always adds. CustomHash with the hash
    // of the same clean JSON the client emits keeps signature and JSON in
    // step.
    let client_data = web_client_data(origin, public_key_json, "webauthn.get")?;
    let result = {
        // `get_all` above ran at security state 1; the SDK encrypts the
        // updated item inside `authenticate`.
        let _blob = BlobCipherWrites::enter(&svc.pm().0);
        client
            .authenticate(Origin::Web(origin.to_string()), request, client_data)
            .await
    }
    .map_err(|e| err(ErrorCode::Invalid, format!("The passkey could not sign in ({e}).")))?;

    if let Some(ctx) = store.saved.lock().expect("saved mutex").take() {
        if let Err(e) = svc.persist(ctx).await {
            tracing::warn!(error = %e, "vault: passkey counter save failed");
        }
    }
    tracing::info!(
        origin = %log_host(origin),
        sig_len = result.response.signature.len(),
        "vault: passkey assertion"
    );
    let user_handle = (!result.response.user_handle.is_empty()).then(|| b64url(&result.response.user_handle));
    serde_json::to_string(&AssertionJson {
        id: result.id,
        raw_id: b64url(&result.raw_id),
        client_data_json: b64url(&result.response.client_data_json),
        authenticator_data: b64url(&result.response.authenticator_data),
        signature: b64url(&result.response.signature),
        user_handle,
    })
    .map_err(|e| internal(e.to_string()))
}

/// Make a passkey for a create(): onto `cipher_id`, or a new personal
/// login named after the site. Saves it; returns the response JSON and
/// the item's id. (The caller syncs before and after.)
pub async fn create(
    svc: &VaultService,
    origin: &str,
    public_key_json: &str,
    cipher_id: Option<String>,
) -> Result<(String, String)> {
    if !svc.pm().is_unlocked() {
        return Err(locked());
    }
    let pk = pk_json(public_key_json)?;
    checked_rp_id(origin, json_str(&pk, "/rp/id"))?;
    let listed = svc.all().await?;
    let ciphers = Arc::new(listed.successes);
    let new_cipher = cipher_id
        .is_none()
        .then(|| new_login_for_passkey(origin, &pk));
    let store = VaultCredentialStore {
        ciphers: ciphers.clone(),
        keys: svc.pm().0.internal.get_key_store().clone(),
        saved: Mutex::new(None),
    };
    let ui = AutoUserInterface {
        preferred_cipher_id: cipher_id,
        new_cipher,
        ciphers,
    };
    let fido = svc.pm().0.fido2();
    let mut client = fido.create_client(&ui, &store);
    let request = format!(r#"{{"publicKey":{public_key_json}}}"#);
    let client_data = web_client_data(origin, public_key_json, "webauthn.create")?;
    let result = {
        let _blob = BlobCipherWrites::enter(&svc.pm().0);
        client
            .register(Origin::Web(origin.to_string()), request, client_data)
            .await
    }
    .map_err(|e| err(ErrorCode::Invalid, format!("The passkey could not be made ({e}).")))?;

    let saved = store
        .saved
        .lock()
        .expect("saved mutex")
        .take()
        .ok_or_else(|| internal("The passkey was made but no item came with it."))?;
    let id = svc.persist(saved).await?;
    tracing::info!(origin = %log_host(origin), %id, "vault: passkey created");
    let json = serde_json::to_string(&AttestationJson {
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
    })
    .map_err(|e| internal(e.to_string()))?;
    Ok((json, id))
}

fn new_login_for_passkey(origin: &str, pk: &serde_json::Value) -> CipherView {
    let apex = apex_domain(origin);
    let name = if !apex.is_empty() {
        apex.clone()
    } else if let Some(host) = origin_host(origin) {
        host
    } else {
        json_str(pk, "/rp/name").unwrap_or("Passkey").to_string()
    };
    let uri = (!apex.is_empty()).then_some(apex);
    new_login_view(name, json_str(pk, "/user/name").map(str::to_string), None, uri)
}

fn b64url(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

/// A byte string from WebAuthn JSON: base64url, tolerating `=` padding
/// and standard base64 (`+/`), both of which passkey-rs accepts.
fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=').replace('+', "-").replace('/', "_");
    URL_SAFE_NO_PAD.decode(s).ok()
}

/// `ClientData::DefaultWithCustomHash` matching the clean web
/// clientDataJSON.
fn web_client_data(origin: &str, public_key_json: &str, ty: &str) -> Result<ClientData> {
    use sha2::{Digest, Sha256};
    let json = web_client_data_json(origin, public_key_json, ty)?;
    Ok(ClientData::DefaultWithCustomHash {
        hash: Sha256::digest(json.as_bytes()).to_vec(),
    })
}

/// The clientDataJSON passkey-client emits for a web origin:
/// `{"type":…,"challenge":…,"origin":…,"crossOrigin":false}`, in that
/// order, values through serde_json as there.
fn web_client_data_json(origin: &str, public_key_json: &str, ty: &str) -> Result<String> {
    let pk = pk_json(public_key_json)?;
    let challenge = json_str(&pk, "/challenge")
        .ok_or_else(|| err(ErrorCode::Invalid, "publicKey.challenge missing"))?;
    // Decode and encode again to match passkey-rs's base64url (no pad).
    let challenge = b64url(
        &b64url_decode(challenge).ok_or_else(|| err(ErrorCode::Invalid, "publicKey.challenge not base64url"))?,
    );
    // Origin::Web trims a trailing `/` for display.
    let origin = origin.trim_end_matches('/');
    let quote = |s: &str| serde_json::to_string(s).map_err(|e| internal(format!("clientDataJSON: {e}")));
    Ok(format!(
        r#"{{"type":{},"challenge":{},"origin":{},"crossOrigin":false}}"#,
        quote(ty)?,
        quote(&challenge)?,
        quote(origin)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_client_data_create_type() {
        let data = web_client_data("https://docs.example.com", r#"{"challenge":"YQ"}"#, "webauthn.create").unwrap();
        match data {
            ClientData::DefaultWithCustomHash { hash } => assert_eq!(hash.len(), 32),
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
        assert_eq!(b64url_decode("YQ==").as_deref(), Some(&b"a"[..]));
        assert_eq!(b64url_decode("+/8=").as_deref(), Some(&[0xfb, 0xff][..]));
        assert_eq!(b64url_decode("YQ==YQ"), None);
        assert_eq!(b64url_decode("Y!Q"), None);
    }

    #[test]
    fn allow_credential_ids_reads_a_non_empty_list() {
        let pk = serde_json::json!({
            "allowCredentials": [
                {"type": "public-key", "id": "-_8"},
                {"type": "public-key", "id": "+/8="},
                {"type": "public-key", "id": null},
            ],
        });
        assert_eq!(allow_credential_ids(&pk), Some(vec![vec![0xfb, 0xff], vec![0xfb, 0xff]]));
        assert_eq!(allow_credential_ids(&serde_json::json!({"allowCredentials": []})), None);
        assert_eq!(allow_credential_ids(&serde_json::json!({"challenge": "YQ"})), None);
    }

    #[test]
    fn the_rp_id_is_checked_against_the_origin() {
        assert_eq!(checked_rp_id("https://login.example.com", Some("example.com")).unwrap(), "example.com");
        assert_eq!(checked_rp_id("https://login.example.com", None).unwrap(), "login.example.com");
        let e = checked_rp_id("https://evil.net", Some("example.com")).unwrap_err();
        assert_eq!(e.code, ErrorCode::Denied);
        assert!(checked_rp_id("http://example.com", None).is_err());
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
                ctx.persist_symmetric_key(local, SymmetricKeySlotId::User).unwrap();
            }
            keys
        }

        /// A personal login holding one passkey, encrypted as the vault does.
        fn passkey_login(keys: &KeyStore<KeySlotIds>, name: &str, rp_id: &str, credential_id: &str) -> CipherView {
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

        async fn found(store: &VaultCredentialStore, ids: Option<Vec<Vec<u8>>>, rp_id: &str) -> Vec<String> {
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
            let exclude = Some(vec![guid(ID_B), guid(ID_C), guid(ID_UNKNOWN)]);
            assert!(found(&store, exclude, "example.com").await.is_empty());
            let exclude = Some(vec![guid(ID_UNKNOWN), guid(ID_A)]);
            assert_eq!(found(&store, exclude, "example.com").await, ["a"]);
        }

        #[test]
        fn a_record_lists_its_passkeys_facts() {
            let keys = unlocked_keys();
            let cipher = passkey_login(&keys, "a", "example.com", ID_A);
            let facts = infos(&keys, &cipher);
            assert_eq!(facts.len(), 1);
            assert_eq!(facts[0].rp_id, "example.com");
        }
    }
}
