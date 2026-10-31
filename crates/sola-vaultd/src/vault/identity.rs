//! Direct Bitwarden identity `/connect/token` login.
//!
//! The SDK's `login_password` path returns an opaque `IdentityFail` for
//! new-device verification and does not surface the raw body; submitting
//! an email code through it still got "new device verification required".
//! So we talk HTTP ourselves, with a known-good form body, and log only
//! the response's error fields.

use std::str::FromStr;
use std::sync::Mutex;
use std::time::Instant;

use bitwarden_core::{
    Client, DeviceType, UserId,
    auth::{ClientManagedTokens, JwtToken},
    key_management::{
        MasterPasswordUnlockData, SymmetricKeySlotId,
        account_cryptographic_state::WrappedAccountCryptographicState,
        crypto::{InitUserCryptoMethod, InitUserCryptoRequest},
    },
};
use bitwarden_crypto::{EncString, HashPurpose, Kdf, MasterKey, SymmetricCryptoKey};
use serde_json::Value;
use sola_vault_proto::{ErrorCode, Server, TwoFactorKind};
use tracing::{info, warn};
use zeroize::Zeroize;

use super::errors::{Result, err, internal};
use super::session::Session;

const CLIENT_ID: &str = "web";
/// The name Bitwarden lists this device under.
const DEVICE_NAME: &str = "Sola Vault";

/// The access and refresh tokens the SDK's API calls read.
#[derive(Debug, Default)]
pub struct TokenCell {
    access: Mutex<Option<String>>,
    refresh: Mutex<Option<String>>,
}

impl TokenCell {
    pub fn set(&self, access: String, refresh: Option<String>) {
        *self.access.lock().expect("token mutex") = Some(access);
        *self.refresh.lock().expect("token mutex") = refresh;
    }

    pub fn access(&self) -> Option<String> {
        self.access.lock().expect("token mutex").clone()
    }

    pub fn refresh(&self) -> Option<String> {
        self.refresh.lock().expect("token mutex").clone()
    }

    pub fn clear(&self) {
        for cell in [&self.access, &self.refresh] {
            let mut g = cell.lock().expect("token mutex");
            if let Some(t) = g.as_mut() {
                t.zeroize();
            }
            *g = None;
        }
    }
}

impl Drop for TokenCell {
    fn drop(&mut self) {
        self.clear();
    }
}

#[async_trait::async_trait]
impl ClientManagedTokens for TokenCell {
    async fn get_access_token(&self) -> Option<String> {
        self.access()
    }
}

/// What a password (or second-step) login came to.
pub enum LoginStep {
    /// Signed in, the client's crypto initialized with the user key.
    Authenticated(Session),
    NeedsTwoFactor {
        kinds: Vec<TwoFactorKind>,
        preferred: TwoFactorKind,
        email_hint: Option<String>,
        email_sent: bool,
        unsupported: Vec<String>,
    },
}

pub struct IdentityLogin<'a> {
    pub client: &'a Client,
    pub tokens: &'a TokenCell,
    pub server: Server,
    pub device_id: String,
    pub email: String,
    pub password: &'a str,
    pub otp: Option<(TwoFactorKind, &'a str)>,
}

/// The token endpoint's answer before crypto is initialized.
enum RawLogin {
    Success {
        access_token: String,
        refresh_token: Option<String>,
        private_key: String,
        unlock: MasterPasswordUnlockData,
    },
    /// `error: device_error`: complete with the form field `newDeviceOtp`.
    NeedsNewDeviceOtp,
    /// Two-step login with the providers we can complete.
    NeedsTwoFactor {
        authenticator: bool,
        email: bool,
        email_hint: Option<String>,
        unsupported: Vec<String>,
    },
    Failed {
        message: String,
    },
}

impl IdentityLogin<'_> {
    pub async fn run(self) -> Result<LoginStep> {
        let completing = self.otp.is_some();
        let email = self.email.trim().to_string();
        let t_login = Instant::now();

        let kdf = self
            .client
            .auth()
            .prelogin(email.clone())
            .await
            .map_err(|e| err(ErrorCode::Network, format!("Bitwarden could not be asked ({e}).")))?;

        // The master key once (PBKDF2 ~600k): for the server hash, and
        // below to unwrap the user key.
        let master_key = MasterKey::derive(self.password, &email, &kdf)
            .map_err(|e| internal(format!("master key: {e}")))?;
        let hash = master_key
            .derive_master_key_hash(self.password.as_bytes(), HashPurpose::ServerAuthorization)
            .to_string();

        let raw = self.post_token(&hash).await?;
        info!(
            ms = t_login.elapsed().as_millis() as u64,
            status = raw.status,
            "vault: identity /connect/token"
        );
        match parse_token_body(&raw.body)? {
            RawLogin::Success {
                access_token,
                refresh_token,
                private_key,
                unlock,
            } => {
                // Crypto init needs the user id (JWT `sub`).
                let user_id = user_id_from_access_token(&access_token)?;
                self.tokens.set(access_token.clone(), refresh_token.clone());
                let salt = if unlock.salt.is_empty() {
                    email.clone()
                } else {
                    unlock.salt.clone()
                };
                let wrapped_user_key = unlock.master_key_wrapped_user_key.to_string();
                // Reuse the master key when salt and KDF match prelogin.
                let user_key = if salt.eq_ignore_ascii_case(&email) && unlock.kdf == kdf {
                    master_key.decrypt_user_key(unlock.master_key_wrapped_user_key)
                } else {
                    info!("vault: unlock salt/kdf differ from prelogin — deriving again");
                    MasterKey::derive(self.password, &salt, &unlock.kdf)
                        .map_err(|e| internal(format!("master key: {e}")))?
                        .decrypt_user_key(unlock.master_key_wrapped_user_key)
                }
                .map_err(|e| internal(format!("The account's key could not be opened ({e}).")))?;
                let session = Session {
                    email: email.clone(),
                    server: self.server.clone(),
                    access_token,
                    refresh_token,
                    kdf: unlock.kdf.clone(),
                    salt,
                    user_id: user_id.to_string(),
                    wrapped_user_key,
                    private_key,
                };
                init_crypto(self.client, &session, user_key).await?;
                info!(
                    total_ms = t_login.elapsed().as_millis() as u64,
                    "vault: identity login authenticated"
                );
                Ok(LoginStep::Authenticated(session))
            }
            RawLogin::NeedsNewDeviceOtp => {
                if completing {
                    return Err(bad_code(
                        "Invalid or expired verification code. Use the latest email or Resend.",
                    ));
                }
                // The password grant already emailed a new-device code.
                Ok(LoginStep::NeedsTwoFactor {
                    kinds: vec![TwoFactorKind::NewDevice],
                    preferred: TwoFactorKind::NewDevice,
                    email_hint: None,
                    email_sent: true,
                    unsupported: Vec::new(),
                })
            }
            RawLogin::NeedsTwoFactor {
                authenticator,
                email,
                email_hint,
                unsupported,
            } => {
                if completing {
                    return Err(bad_code(if email {
                        "Invalid or expired code. Check it, or use the latest email."
                    } else {
                        "Invalid authenticator code. Try again."
                    }));
                }
                // An authenticator needs no round trip; with more than one
                // provider Bitwarden emails no code until asked. Email
                // alone is sent with this response.
                let kinds: Vec<TwoFactorKind> = [
                    authenticator.then_some(TwoFactorKind::Authenticator),
                    email.then_some(TwoFactorKind::Email),
                ]
                .into_iter()
                .flatten()
                .collect();
                let preferred = kinds[0];
                Ok(LoginStep::NeedsTwoFactor {
                    kinds,
                    preferred,
                    email_hint,
                    email_sent: preferred == TwoFactorKind::Email,
                    unsupported,
                })
            }
            RawLogin::Failed { message } => {
                if looks_like_device(&message) {
                    if completing {
                        return Err(bad_code(
                            "Invalid or expired verification code. Use the latest email or Resend.",
                        ));
                    }
                    return Ok(LoginStep::NeedsTwoFactor {
                        kinds: vec![TwoFactorKind::NewDevice],
                        preferred: TwoFactorKind::NewDevice,
                        email_hint: None,
                        email_sent: true,
                        unsupported: Vec::new(),
                    });
                }
                Err(login_refused(message))
            }
        }
    }

    async fn post_token(&self, password_hash: &str) -> Result<RawHttp> {
        use serde::Serialize;

        let api = self.client.internal.get_api_configurations();
        let url = format!(
            "{}/connect/token",
            api.identity_config.base_path.trim_end_matches('/')
        );

        // New-device protection uses `newDeviceOtp`; two-step login uses
        // twoFactorToken + twoFactorProvider.
        let (mut two_factor_token, mut two_factor_provider, mut new_device_otp) = (None, None, None);
        if let Some((kind, token)) = self.otp {
            info!(?kind, token_len = token.len(), "vault: identity token with a code");
            match kind {
                TwoFactorKind::NewDevice => new_device_otp = Some(token.to_string()),
                TwoFactorKind::Email => {
                    two_factor_token = Some(token.to_string());
                    two_factor_provider = Some(1u8);
                }
                TwoFactorKind::Authenticator => {
                    two_factor_token = Some(token.to_string());
                    two_factor_provider = Some(0u8);
                }
            }
        }

        #[derive(Serialize)]
        struct PasswordTokenBody {
            scope: String,
            client_id: String,
            #[serde(rename = "deviceType")]
            device_type: u8,
            #[serde(rename = "deviceIdentifier")]
            device_identifier: String,
            #[serde(rename = "deviceName")]
            device_name: String,
            grant_type: String,
            #[serde(rename = "username")]
            email: String,
            #[serde(rename = "password")]
            master_password_hash: String,
            #[serde(rename = "twoFactorToken", skip_serializing_if = "Option::is_none")]
            two_factor_token: Option<String>,
            #[serde(rename = "twoFactorProvider", skip_serializing_if = "Option::is_none")]
            two_factor_provider: Option<u8>,
            #[serde(rename = "newDeviceOtp", skip_serializing_if = "Option::is_none")]
            new_device_otp: Option<String>,
        }

        let body = PasswordTokenBody {
            scope: "api offline_access".into(),
            client_id: CLIENT_ID.into(),
            // As the browser sent it, so a migrated device stays known.
            device_type: DeviceType::ChromeBrowser as u8,
            device_identifier: self.device_id.clone(),
            device_name: DEVICE_NAME.into(),
            grant_type: "password".into(),
            email: self.email.trim().to_string(),
            master_password_hash: password_hash.into(),
            two_factor_token,
            two_factor_provider,
            new_device_otp,
        };
        let mut body_str =
            serde_qs::to_string(&body).map_err(|e| internal(format!("token body: {e}")))?;

        let sent = api
            .identity_config
            .client
            .post(&url)
            .header("content-type", "application/x-www-form-urlencoded; charset=utf-8")
            .header("accept", "application/json")
            .header("cache-control", "no-store")
            .header("pragma", "no-cache")
            .body(body_str.clone())
            .send()
            .await;
        body_str.zeroize();
        let resp = sent.map_err(|e| {
            warn!(error = %e, "vault: identity unreachable");
            err(ErrorCode::Network, "Bitwarden could not be reached.")
        })?;
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| err(ErrorCode::Network, format!("Bitwarden's answer was cut off ({e}).")))?;
        if !(200..300).contains(&status) {
            warn!(status, error = %token_error_summary(&body), "vault: identity /connect/token refused");
        }
        Ok(RawHttp { status, body })
    }
}

/// Load the user key, the private key and the user id into `client`'s
/// key store. The key is dropped (zeroized) here.
pub async fn init_crypto(client: &Client, session: &Session, user_key: SymmetricCryptoKey) -> Result<()> {
    let user_id =
        UserId::from_str(&session.user_id).map_err(|e| internal(format!("user id: {e}")))?;
    let private_key: EncString = session
        .private_key
        .parse()
        .map_err(|e| internal(format!("private key: {e}")))?;
    let mut decrypted_user_key = user_key.to_base64().to_string();
    drop(user_key);
    let result = client
        .crypto()
        .initialize_user_crypto(InitUserCryptoRequest {
            user_id: Some(user_id),
            kdf_params: session.kdf.clone(),
            email: session.salt().to_string(),
            account_cryptographic_state: WrappedAccountCryptographicState::V1 { private_key },
            method: InitUserCryptoMethod::DecryptedKey {
                decrypted_user_key: decrypted_user_key.clone(),
            },
            upgrade_token: None,
        })
        .await;
    decrypted_user_key.zeroize();
    result.map_err(|e| internal(format!("crypto init: {e}")))?;
    // V1 init leaves the security state at 1. Leave it there: 2 makes
    // decrypt drop login URIs that have no checksum. Personal writes raise
    // it only around `ciphers().encrypt` (`BlobCipherWrites`).
    let has_key = client
        .internal
        .get_key_store()
        .context()
        .has_symmetric_key(SymmetricKeySlotId::User);
    if !has_key {
        return Err(internal("crypto init did not load the user key"));
    }
    Ok(())
}

/// Why a token refresh failed.
#[derive(Debug)]
pub enum RefreshError {
    /// Bitwarden refused the refresh token: the session is over.
    Rejected,
    /// Bitwarden could not be asked (offline, DNS, a 5xx): keep the
    /// session and try again later.
    Unreachable,
}

/// Exchange a refresh token for a new access (+ refresh) pair.
pub async fn refresh_access_token(
    client: &Client,
    refresh_token: &str,
) -> std::result::Result<(String, Option<String>), RefreshError> {
    use serde::Serialize;

    let api = client.internal.get_api_configurations();
    let url = format!(
        "{}/connect/token",
        api.identity_config.base_path.trim_end_matches('/')
    );

    #[derive(Serialize)]
    struct RefreshBody<'a> {
        grant_type: &'a str,
        client_id: &'a str,
        refresh_token: &'a str,
    }
    let mut body = serde_qs::to_string(&RefreshBody {
        grant_type: "refresh_token",
        client_id: CLIENT_ID,
        refresh_token,
    })
    .map_err(|_| RefreshError::Unreachable)?;

    let sent = api
        .identity_config
        .client
        .post(&url)
        .header("content-type", "application/x-www-form-urlencoded; charset=utf-8")
        .header("accept", "application/json")
        .header("cache-control", "no-store")
        .body(body.clone())
        .send()
        .await;
    body.zeroize();
    let resp = sent.map_err(|e| {
        warn!(error = %e, "vault: refresh unreachable");
        RefreshError::Unreachable
    })?;
    let status = resp.status().as_u16();
    let mut text = resp.text().await.map_err(|_| RefreshError::Unreachable)?;
    if !(200..300).contains(&status) {
        warn!(status, error = %token_error_summary(&text), "vault: refresh token failed");
        // 400 invalid_grant / 401: the token is dead. Anything else may pass.
        return Err(if matches!(status, 400 | 401) {
            RefreshError::Rejected
        } else {
            RefreshError::Unreachable
        });
    }
    let parsed: std::result::Result<Value, _> = serde_json::from_str(&text);
    text.zeroize();
    let v = parsed.map_err(|_| RefreshError::Unreachable)?;
    let access = v
        .get("access_token")
        .or_else(|| v.get("accessToken"))
        .and_then(|x| x.as_str())
        .ok_or(RefreshError::Unreachable)?
        .to_string();
    let refresh = v
        .get("refresh_token")
        .or_else(|| v.get("refreshToken"))
        .and_then(|x| x.as_str())
        .map(str::to_string);
    Ok((access, refresh))
}

/// POST /api/accounts/resend-new-device-otp (unauthenticated).
pub async fn resend_new_device_otp(client: &Client, email: &str, master_password_hash: &str) -> Result<()> {
    let api = client.internal.get_api_configurations();
    let url = format!(
        "{}/accounts/resend-new-device-otp",
        api.api_config.base_path.trim_end_matches('/')
    );
    let body = serde_json::json!({
        "email": email,
        "masterPasswordHash": master_password_hash,
    });
    info!("vault: resend new-device code");
    let resp = api
        .api_config
        .client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|_| err(ErrorCode::Network, "Bitwarden could not be reached."))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        warn!(status, error = %token_error_summary(&text), "vault: resend new-device code failed");
        return Err(err(
            ErrorCode::Invalid,
            format!("Could not resend the code (HTTP {status}). Wait for the last email or try again."),
        ));
    }
    Ok(())
}

struct RawHttp {
    status: u16,
    body: String,
}

impl Drop for RawHttp {
    fn drop(&mut self) {
        self.body.zeroize();
    }
}

fn bad_code(message: &str) -> sola_vault_proto::VaultError {
    err(ErrorCode::TwoFactor, message)
}

/// A refused login: a wrong password reads as one.
fn login_refused(message: String) -> sola_vault_proto::VaultError {
    let lower = message.to_lowercase();
    if lower.contains("password is incorrect") || lower.contains("invalid_username_or_password") {
        err(ErrorCode::BadPassword, "Email or master password is incorrect.")
    } else {
        err(ErrorCode::Invalid, message)
    }
}

/// Extract the user id from an access-token JWT (`sub`).
fn user_id_from_access_token(access_token: &str) -> Result<UserId> {
    let jwt: JwtToken = access_token
        .parse()
        .map_err(|e| internal(format!("access token: {e}")))?;
    UserId::from_str(&jwt.sub).map_err(|e| internal(format!("user id: {e}")))
}

fn looks_like_device(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("new device")
        || m.contains("device verification")
        || m.contains("device login protection")
        || m.contains("device_error")
}

/// The error fields of an identity response, for the log. Bodies are
/// never logged whole: a success carries tokens, the wrapped user key,
/// KDF and salt (enough to attack the master password offline), and a
/// two-step response can carry a remember-device token.
fn token_error_summary(body: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return format!("<{} bytes, not json>", body.len());
    };
    let mut out = serde_json::Map::new();
    for k in ["error", "error_description", "message", "TwoFactorProviders"] {
        if let Some(x) = v.get(k) {
            out.insert(k.into(), x.clone());
        }
    }
    if let Some(m) = v.pointer("/ErrorModel/Message") {
        out.insert("ErrorModel.Message".into(), m.clone());
    }
    Value::Object(out).to_string()
}

fn parse_token_body(body: &str) -> Result<RawLogin> {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::String(body.to_string()));

    if let Some(access_token) = v
        .get("access_token")
        .or_else(|| v.get("accessToken"))
        .and_then(|x| x.as_str())
    {
        let private_key = v
            .get("PrivateKey")
            .or_else(|| v.get("private_key"))
            .or_else(|| v.get("privateKey"))
            .and_then(|x| x.as_str())
            .or_else(|| {
                v.pointer("/AccountKeys/publicKeyEncryptionKeyPair/wrappedPrivateKey")
                    .and_then(|x| x.as_str())
            })
            .ok_or_else(|| internal("The login answer had no private key."))?
            .to_string();
        let unlock = extract_unlock_data(&v)
            .ok_or_else(|| internal("The login answer had no master-password unlock data."))?;
        let refresh_token = v
            .get("refresh_token")
            .or_else(|| v.get("refreshToken"))
            .and_then(|x| x.as_str())
            .map(str::to_string);
        return Ok(RawLogin::Success {
            access_token: access_token.to_string(),
            refresh_token,
            private_key,
            unlock,
        });
    }

    let err_code = v.get("error").and_then(|x| x.as_str()).unwrap_or("");
    if err_code == "device_error" || looks_like_device(err_code) {
        return Ok(RawLogin::NeedsNewDeviceOtp);
    }

    if let Some(providers) = v
        .get("TwoFactorProviders2")
        .or_else(|| v.get("twoFactorProviders2"))
        .and_then(|p| p.as_object())
    {
        let email_hint = providers
            .get("1")
            .and_then(|e| e.get("Email").or_else(|| e.get("email")))
            .and_then(|x| x.as_str())
            .map(str::to_string);
        let ids: Vec<&str> = providers.keys().map(String::as_str).collect();
        return Ok(two_factor(&ids, email_hint));
    }

    if let Some(arr) = v
        .get("TwoFactorProviders")
        .or_else(|| v.get("twoFactorProviders"))
        .and_then(|x| x.as_array())
    {
        let ids: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string).or_else(|| x.as_i64().map(|n| n.to_string())))
            .collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        return Ok(two_factor(&ids, None));
    }

    let message = v
        .pointer("/ErrorModel/Message")
        .or_else(|| v.pointer("/errorModel/message"))
        .and_then(|x| x.as_str())
        .or_else(|| v.get("error_description").and_then(|x| x.as_str()))
        .or_else(|| v.get("error").and_then(|x| x.as_str()))
        .unwrap_or("Bitwarden refused the login.")
        .to_string();
    if looks_like_device(&message) {
        return Ok(RawLogin::NeedsNewDeviceOtp);
    }
    Ok(RawLogin::Failed { message })
}

/// The two-step login we can complete (`0` authenticator, `1` email), or
/// why not. Other providers are named for a message.
fn two_factor(provider_ids: &[&str], email_hint: Option<String>) -> RawLogin {
    let authenticator = provider_ids.contains(&"0");
    let email = provider_ids.contains(&"1");
    let unsupported: Vec<String> = provider_ids
        .iter()
        .filter_map(|id| provider_name(id))
        .map(str::to_string)
        .collect();
    if authenticator || email {
        RawLogin::NeedsTwoFactor {
            authenticator,
            email,
            email_hint,
            unsupported,
        }
    } else {
        RawLogin::Failed {
            message: "This account's two-step login (security key, Duo or YubiKey) isn't \
                      supported here yet. Add an authenticator app or email codes in Bitwarden."
                .into(),
        }
    }
}

/// Bitwarden's `TwoFactorProviderType`s Sola cannot do.
fn provider_name(id: &str) -> Option<&'static str> {
    Some(match id {
        "2" => "Duo",
        "3" => "YubiKey OTP",
        "4" => "U2F",
        "5" => "Remember this device",
        "6" => "Duo (organization)",
        "7" => "Security key",
        _ => return None,
    })
}

fn extract_unlock_data(v: &Value) -> Option<MasterPasswordUnlockData> {
    let mpu = v
        .pointer("/UserDecryptionOptions/MasterPasswordUnlock")
        .or_else(|| v.pointer("/userDecryptionOptions/masterPasswordUnlock"))
        .or_else(|| v.pointer("/UserDecryptionOptions/masterPasswordUnlock"));
    let email = || {
        v.get("Email")
            .or_else(|| v.get("email"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string()
    };

    if let Some(mpu) = mpu {
        let wrapped = mpu
            .get("MasterKeyEncryptedUserKey")
            .or_else(|| mpu.get("masterKeyEncryptedUserKey"))
            .and_then(|x| x.as_str())?;
        let salt = mpu
            .get("Salt")
            .or_else(|| mpu.get("salt"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let kdf = parse_kdf(mpu).or_else(|| parse_kdf(v))?;
        return Some(MasterPasswordUnlockData {
            kdf,
            master_key_wrapped_user_key: wrapped.parse().ok()?,
            salt: if salt.is_empty() { email() } else { salt },
        });
    }

    // Legacy: top-level Key + Kdf fields.
    let wrapped = v.get("Key").or_else(|| v.get("key")).and_then(|x| x.as_str())?;
    Some(MasterPasswordUnlockData {
        kdf: parse_kdf(v)?,
        master_key_wrapped_user_key: wrapped.parse().ok()?,
        salt: email(),
    })
}

fn parse_kdf(v: &Value) -> Option<Kdf> {
    if let Some(kdf_obj) = v.get("Kdf").or_else(|| v.get("kdf")).filter(|x| x.is_object()) {
        let kdf_type = kdf_obj
            .get("KdfType")
            .or_else(|| kdf_obj.get("kdfType"))
            .and_then(|x| x.as_i64())
            .unwrap_or(0);
        let iterations = kdf_obj
            .get("Iterations")
            .or_else(|| kdf_obj.get("iterations"))
            .and_then(|x| x.as_u64())
            .unwrap_or(600_000) as u32;
        return kdf_from_parts(kdf_type, iterations, kdf_obj);
    }
    let kdf_type = v
        .get("Kdf")
        .or_else(|| v.get("kdf"))
        .and_then(|x| x.as_i64())
        .unwrap_or(0);
    let iterations = v
        .get("KdfIterations")
        .or_else(|| v.get("kdfIterations"))
        .and_then(|x| x.as_u64())
        .unwrap_or(600_000) as u32;
    kdf_from_parts(kdf_type, iterations, v)
}

fn kdf_from_parts(kdf_type: i64, iterations: u32, v: &Value) -> Option<Kdf> {
    use std::num::NonZeroU32;
    let nz = |n: u32, or: u32| NonZeroU32::new(n).unwrap_or(NonZeroU32::new(or).expect("non-zero"));
    let field = |names: [&str; 4], or: u64| {
        names
            .iter()
            .find_map(|n| v.get(*n).and_then(|x| x.as_u64()))
            .unwrap_or(or) as u32
    };
    match kdf_type {
        0 => Some(Kdf::PBKDF2 {
            iterations: nz(iterations, 600_000),
        }),
        1 => Some(Kdf::Argon2id {
            iterations: nz(iterations, 3),
            memory: nz(field(["KdfMemory", "kdfMemory", "Memory", "memory"], 64), 64),
            parallelism: nz(
                field(["KdfParallelism", "kdfParallelism", "Parallelism", "parallelism"], 4),
                4,
            ),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_error_asks_for_the_emailed_code() {
        let body = r#"{"error":"device_error","error_description":"New device verification required."}"#;
        assert!(matches!(parse_token_body(body).unwrap(), RawLogin::NeedsNewDeviceOtp));
    }

    #[test]
    fn two_step_providers_are_read_and_named() {
        let body = r#"{"error":"invalid_grant","TwoFactorProviders2":{"0":null,"1":{"Email":"a***@b.c"},"7":{}}}"#;
        match parse_token_body(body).unwrap() {
            RawLogin::NeedsTwoFactor {
                authenticator,
                email,
                email_hint,
                unsupported,
            } => {
                assert!(authenticator && email);
                assert_eq!(email_hint.as_deref(), Some("a***@b.c"));
                assert_eq!(unsupported, ["Security key"]);
            }
            _ => panic!("expected two-step"),
        }
        let body = r#"{"TwoFactorProviders":[2,3]}"#;
        assert!(matches!(parse_token_body(body).unwrap(), RawLogin::Failed { .. }));
    }

    #[test]
    fn a_wrong_password_reads_as_one() {
        let body = r#"{"error":"invalid_grant","ErrorModel":{"Message":"Username or password is incorrect. Try again."}}"#;
        let RawLogin::Failed { message } = parse_token_body(body).unwrap() else {
            panic!("expected failure");
        };
        assert_eq!(login_refused(message).code, ErrorCode::BadPassword);
    }

    #[test]
    fn kdf_reads_argon2_and_pbkdf2() {
        let v = serde_json::json!({"Kdf": 1, "KdfIterations": 3, "KdfMemory": 64, "KdfParallelism": 4});
        assert!(matches!(parse_kdf(&v), Some(Kdf::Argon2id { .. })));
        let v = serde_json::json!({"Kdf": {"KdfType": 0, "Iterations": 600000}});
        assert!(matches!(parse_kdf(&v), Some(Kdf::PBKDF2 { .. })));
    }
}
