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
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyAssertionJson {
    pub id: String,
    pub raw_id: String,
    pub client_data_json: String,
    pub authenticator_data: String,
    pub signature: String,
    pub user_handle: String,
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

    let result = client
        .authenticate(
            Origin::Web(origin.to_string()),
            request,
            // Web path: no android package. Empty string still tags the
            // ClientData variant that lets passkey-rs build standard
            // clientDataJSON (challenge/origin/type).
            ClientData::DefaultWithExtraData {
                android_package_name: String::new(),
            },
        )
        .await
        .map_err(|e| VaultError::Other(format!("passkey authenticate failed: {e}")))?;

    Ok(PasskeyAssertionJson {
        id: result.id,
        raw_id: b64url(&result.raw_id),
        client_data_json: b64url(&result.response.client_data_json),
        authenticator_data: b64url(&result.response.authenticator_data),
        signature: b64url(&result.response.signature),
        user_handle: b64url(&result.response.user_handle),
    })
}
