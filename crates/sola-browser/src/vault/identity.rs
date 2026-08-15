//! Direct Bitwarden identity `/connect/token` login.
//!
//! The SDK’s `login_password` path returns opaque `IdentityFail` for
//! new-device verification and does not surface the raw body. Submitting an
//! email OTP via the same API was still getting “new device verification
//! required” — we talk HTTP ourselves so we can log the response and send a
//! known-good form body.

use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bitwarden_core::{
    Client, ClientSettings, DeviceType, FromClient, UserId,
    auth::{ClientManagedTokens, JwtToken},
    key_management::{
        MasterPasswordUnlockData, SymmetricKeySlotId,
        account_cryptographic_state::WrappedAccountCryptographicState,
        crypto::{InitUserCryptoMethod, InitUserCryptoRequest},
    },
};
use bitwarden_crypto::{EncString, HashPurpose, Kdf, MasterKey};
use bitwarden_crypto_sync_handler::CryptoSyncHandler;
use bitwarden_pm::PasswordManagerClient;
use bitwarden_vault::FolderSyncHandler;
use serde_json::Value;
use tracing::{info, warn};
use zeroize::Zeroize;

use super::client::{LoginOutcome, TwoFactorKind};
use super::sync_cipher::CipherSyncHandler;

/// Same stable id the SDK password login uses (see bitwarden-core PasswordTokenRequest).
const DEVICE_ID: &str = "b86dd6ab-4265-4ddf-a7f1-eb28d5677f33";
const CLIENT_ID: &str = "web";

/// Access token cell used by the SDK middleware after custom login.
#[derive(Debug, Default)]
pub struct TokenCell {
    access: Mutex<Option<String>>,
}

impl TokenCell {
    pub fn set(&self, token: String) {
        *self.access.lock().expect("token mutex") = Some(token);
    }

    pub fn clear(&self) {
        *self.access.lock().expect("token mutex") = None;
    }
}

#[async_trait::async_trait]
impl ClientManagedTokens for TokenCell {
    async fn get_access_token(&self) -> Option<String> {
        self.access.lock().expect("token mutex").clone()
    }
}

/// Build a PM client that reads tokens from `tokens` (we set them after a
/// successful custom identity login).
///
/// Returns a **long-lived** [`bitwarden_sync::SyncClient`] with handlers
/// registered. `Client::sync()` / `PasswordManagerClient::sync()` construct a
/// *new* empty SyncClient each call — registering handlers on a temporary and
/// then calling `client.sync().sync()` later never runs cipher handlers
/// (empty vault after "sync ok").
pub fn build_pm_client(
    settings: ClientSettings,
    tokens: Arc<TokenCell>,
) -> (PasswordManagerClient, bitwarden_sync::SyncClient) {
    use bitwarden_core::auth::ClientManagedTokenHandler;
    use bitwarden_core::client::ClientBuilder;
    use bitwarden_core::key_management::{LocalUserDataKeyState, UserKeyState};
    use bitwarden_vault::{Cipher, Folder};

    use super::memory_repo::MemoryRepository;

    let client = ClientBuilder::new()
        .with_settings(settings)
        .with_token_handler(ClientManagedTokenHandler::new(tokens))
        .build();

    // Session-scoped stores. Without these, sync handlers have nowhere to
    // write and `ciphers().get_all()` always returns empty.
    let state = client.platform().state();
    // SettingItem is required if last_sync Setting is used; optional for force sync.
    state.register_client_managed(Arc::new(MemoryRepository::<Cipher>::default()));
    state.register_client_managed(Arc::new(MemoryRepository::<Folder>::default()));
    state.register_client_managed(Arc::new(MemoryRepository::<UserKeyState>::default()));
    state.register_client_managed(Arc::new(
        MemoryRepository::<LocalUserDataKeyState>::default(),
    ));

    let pm = PasswordManagerClient(client);
    // Keep this SyncClient for the process lifetime — handlers live on it.
    let sync = pm.sync();
    sync.register_sync_handler(Arc::new(CryptoSyncHandler::new(pm.0.clone())));
    sync.register_sync_handler(Arc::new(FolderSyncHandler::from_client(&pm.0)));
    sync.register_sync_handler(Arc::new(CipherSyncHandler::from_client(&pm.0)));
    (pm, sync)
}

pub struct IdentityLogin<'a> {
    pub client: &'a Client,
    pub tokens: &'a TokenCell,
    pub email: String,
    pub password: String,
    pub otp: Option<(TwoFactorKind, String)>,
}

/// Result of a raw identity login before crypto is initialized.
enum RawLogin {
    Success {
        access_token: String,
        private_key: String,
        unlock: MasterPasswordUnlockData,
        force_password_reset: bool,
    },
    /// `error: device_error` — complete with form field `newDeviceOtp`.
    NeedsNewDeviceOtp {
        email_hint: Option<String>,
    },
    /// Standard email two-step login — complete with twoFactorToken.
    NeedsEmailTwoFactor {
        email_hint: Option<String>,
    },
    NeedsAuthenticator,
    Failed {
        message: String,
    },
}

impl IdentityLogin<'_> {
    pub async fn run(self) -> Result<LoginOutcome, String> {
        let completing_2fa = self.otp.is_some();
        let email = self.email.clone();
        let mut password = self.password.clone();
        let t_login = Instant::now();

        // --- prelogin (KDF params from server) ---
        let t0 = Instant::now();
        let kdf = self
            .client
            .auth()
            .prelogin(email.clone())
            .await
            .map_err(|e| format!("prelogin: {e}"))?;
        info!(
            ms = t0.elapsed().as_millis() as u64,
            "vault: timing prelogin"
        );

        // --- master key once (PBKDF2 ~600k) ---
        // Chrome-class clients pay this cost **once**. We previously derived
        // for the server auth hash *and* again inside MasterPasswordUnlock.
        let t1 = Instant::now();
        let master_key =
            MasterKey::derive(&password, &email, &kdf).map_err(|e| format!("master key: {e}"))?;
        let hash = master_key
            .derive_master_key_hash(password.as_bytes(), HashPurpose::ServerAuthorization)
            .to_string();
        info!(
            ms = t1.elapsed().as_millis() as u64,
            "vault: timing master-key derive"
        );

        let t2 = Instant::now();
        let raw = self.post_token(&hash).await?;
        info!(
            ms = t2.elapsed().as_millis() as u64,
            status = raw.status,
            "vault: timing /connect/token"
        );
        let parsed = parse_token_body(&raw.body, raw.status)?;

        match parsed {
            RawLogin::Success {
                access_token,
                private_key,
                unlock,
                force_password_reset,
            } => {
                // SDK `initialize_user_crypto` needs a real user id for the
                // local-user-data-key step (`Unable to initialize local user
                // data key` if missing). Official clients take it from JWT `sub`.
                let user_id = user_id_from_access_token(&access_token)?;
                self.tokens.set(access_token);
                let private_key: EncString = private_key
                    .parse()
                    .map_err(|e| format!("private key: {e}"))?;
                // Prefer server-provided salt (usually the account email).
                let crypto_email = if unlock.salt.is_empty() {
                    email.clone()
                } else {
                    unlock.salt.clone()
                };

                // Reuse master key when salt/kdf match prelogin; only re-derive
                // if the unlock payload disagrees (rare).
                let t3 = Instant::now();
                let user_key = if crypto_email.eq_ignore_ascii_case(email.trim())
                    && unlock.kdf == kdf
                {
                    master_key
                        .decrypt_user_key(unlock.master_key_wrapped_user_key)
                        .map_err(|e| format!("decrypt user key: {e}"))?
                } else {
                    info!("vault: unlock salt/kdf differ from prelogin — re-deriving master key");
                    MasterKey::derive(&password, &crypto_email, &unlock.kdf)
                        .map_err(|e| format!("master key (unlock): {e}"))?
                        .decrypt_user_key(unlock.master_key_wrapped_user_key)
                        .map_err(|e| format!("decrypt user key: {e}"))?
                };
                info!(
                    ms = t3.elapsed().as_millis() as u64,
                    "vault: timing decrypt user key"
                );

                let t4 = Instant::now();
                self.client
                    .crypto()
                    .initialize_user_crypto(InitUserCryptoRequest {
                        user_id: Some(user_id),
                        kdf_params: unlock.kdf.clone(),
                        email: crypto_email,
                        account_cryptographic_state: WrappedAccountCryptographicState::V1 {
                            private_key,
                        },
                        // Skip a second 600k PBKDF2 — we already hold the user key.
                        method: InitUserCryptoMethod::DecryptedKey {
                            decrypted_user_key: user_key.to_base64().to_string(),
                        },
                        upgrade_token: None,
                    })
                    .await
                    .map_err(|e| format!("crypto init: {e}"))?;
                info!(
                    ms = t4.elapsed().as_millis() as u64,
                    "vault: timing crypto init"
                );

                // Confirm user key is present.
                let has_key = self
                    .client
                    .internal
                    .get_key_store()
                    .context()
                    .has_symmetric_key(SymmetricKeySlotId::User);
                if !has_key {
                    password.zeroize();
                    return Err("crypto init did not load user key".into());
                }

                password.zeroize();
                info!(
                    total_ms = t_login.elapsed().as_millis() as u64,
                    "vault: identity login authenticated"
                );
                Ok(LoginOutcome::Authenticated {
                    force_password_reset,
                })
            }
            RawLogin::NeedsNewDeviceOtp { email_hint } => {
                password.zeroize();
                if completing_2fa {
                    return Err(
                        "Invalid or expired verification code. Use the latest email or Resend."
                            .into(),
                    );
                }
                // Password grant already emailed a new-device code — do not resend.
                Ok(LoginOutcome::NeedsTwoFactor {
                    kinds: vec![TwoFactorKind::NewDevice],
                    preferred: TwoFactorKind::NewDevice,
                    email_hint,
                    email_sent: true,
                })
            }
            RawLogin::NeedsEmailTwoFactor { email_hint } => {
                password.zeroize();
                if completing_2fa {
                    return Err(
                        "Invalid or expired verification code. Use the latest email or Resend."
                            .into(),
                    );
                }
                Ok(LoginOutcome::NeedsTwoFactor {
                    kinds: vec![TwoFactorKind::Email],
                    preferred: TwoFactorKind::Email,
                    email_hint,
                    email_sent: true,
                })
            }
            RawLogin::NeedsAuthenticator => {
                password.zeroize();
                if completing_2fa {
                    return Err("Invalid authenticator code. Try again.".into());
                }
                Ok(LoginOutcome::NeedsTwoFactor {
                    kinds: vec![TwoFactorKind::Authenticator],
                    preferred: TwoFactorKind::Authenticator,
                    email_hint: None,
                    email_sent: false,
                })
            }
            RawLogin::Failed { message } => {
                password.zeroize();
                if completing_2fa {
                    return Err(if looks_like_device(&message) {
                        "Invalid or expired verification code. Use the latest email or Resend."
                            .into()
                    } else {
                        message
                    });
                }
                if looks_like_device(&message) {
                    Ok(LoginOutcome::NeedsTwoFactor {
                        kinds: vec![TwoFactorKind::NewDevice],
                        preferred: TwoFactorKind::NewDevice,
                        email_hint: None,
                        email_sent: true,
                    })
                } else {
                    Err(message)
                }
            }
        }
    }

    async fn post_token(&self, password_hash: &str) -> Result<RawHttp, String> {
        use serde::Serialize;

        let api = self.client.internal.get_api_configurations();
        let url = format!(
            "{}/connect/token",
            api.identity_config.base_path.trim_end_matches('/')
        );

        // Match SDK password login: ChromeBrowser + fixed device id + "firefox".
        // New-device protection uses `newDeviceOtp` (official clients).
        // Standard two-step login uses twoFactorToken + twoFactorProvider.
        let mut two_factor_token = None;
        let mut two_factor_provider = None;
        let mut two_factor_remember = None;
        let mut new_device_otp = None;

        if let Some((kind, token)) = &self.otp {
            match kind {
                TwoFactorKind::NewDevice => {
                    info!(
                        token_len = token.len(),
                        "vault: identity token + newDeviceOtp"
                    );
                    new_device_otp = Some(token.clone());
                }
                TwoFactorKind::Email => {
                    info!(token_len = token.len(), "vault: identity token + email 2FA");
                    two_factor_token = Some(token.clone());
                    two_factor_provider = Some(1u8);
                    two_factor_remember = Some(true);
                }
                TwoFactorKind::Authenticator => {
                    info!(
                        token_len = token.len(),
                        "vault: identity token + authenticator 2FA"
                    );
                    two_factor_token = Some(token.clone());
                    two_factor_provider = Some(0u8);
                    two_factor_remember = Some(true);
                }
            }
        } else {
            info!("vault: identity token request (password only)");
        }

        #[derive(Serialize)]
        struct PasswordTokenBodyFull {
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
            #[serde(rename = "twoFactorToken")]
            #[serde(skip_serializing_if = "Option::is_none")]
            two_factor_token: Option<String>,
            #[serde(rename = "twoFactorProvider")]
            #[serde(skip_serializing_if = "Option::is_none")]
            two_factor_provider: Option<u8>,
            #[serde(rename = "twoFactorRemember")]
            #[serde(skip_serializing_if = "Option::is_none")]
            two_factor_remember: Option<bool>,
            /// New device login protection (NOT the same as email 2FA).
            #[serde(rename = "newDeviceOtp")]
            #[serde(skip_serializing_if = "Option::is_none")]
            new_device_otp: Option<String>,
        }

        let body = PasswordTokenBodyFull {
            scope: "api offline_access".into(),
            client_id: CLIENT_ID.into(),
            device_type: DeviceType::ChromeBrowser as u8,
            device_identifier: DEVICE_ID.into(),
            device_name: "firefox".into(),
            grant_type: "password".into(),
            email: self.email.clone(),
            master_password_hash: password_hash.into(),
            two_factor_token,
            two_factor_provider,
            two_factor_remember,
            new_device_otp,
        };

        let body_str =
            serde_qs::to_string(&body).map_err(|e| format!("serialize token body: {e}"))?;

        // Use the same middleware client the SDK identity stack uses.
        let resp = api
            .identity_config
            .client
            .post(&url)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded; charset=utf-8",
            )
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::CACHE_CONTROL, "no-store")
            .header(reqwest::header::PRAGMA, "no-cache")
            .body(body_str)
            .send()
            .await
            .map_err(|e| format!("identity http: {e}"))?;

        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| format!("identity body: {e}"))?;

        let log_body = redact_secrets(&body);
        warn!(status, body = %log_body, "vault: identity /connect/token response");

        Ok(RawHttp { status, body })
    }
}

struct RawHttp {
    status: u16,
    body: String,
}

/// Extract `UserId` from a Bitwarden access-token JWT (`sub` claim).
fn user_id_from_access_token(access_token: &str) -> Result<UserId, String> {
    let jwt: JwtToken = access_token
        .parse()
        .map_err(|e| format!("access token jwt: {e}"))?;
    UserId::from_str(&jwt.sub).map_err(|e| format!("user id from jwt sub: {e}"))
}

fn looks_like_device(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("new device")
        || m.contains("device verification")
        || m.contains("device login protection")
        || m.contains("device_error")
}

/// POST /api/accounts/resend-new-device-otp (unauthenticated).
pub async fn resend_new_device_otp(
    client: &Client,
    email: &str,
    master_password_hash: &str,
) -> Result<(), String> {
    let api = client.internal.get_api_configurations();
    let url = format!(
        "{}/accounts/resend-new-device-otp",
        api.api_config.base_path.trim_end_matches('/')
    );
    let body = serde_json::json!({
        "email": email,
        "masterPasswordHash": master_password_hash,
    });
    info!(%url, "vault: resend new-device OTP");
    let resp = api
        .api_config
        .client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("resend otp http: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        warn!(status, body = %text, "vault: resend new-device OTP failed");
        return Err(format!(
            "Could not resend code (HTTP {status}). Wait for the previous email or try again."
        ));
    }
    Ok(())
}

fn redact_secrets(body: &str) -> String {
    let mut v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return body.chars().take(500).collect(),
    };
    if let Some(obj) = v.as_object_mut() {
        for k in [
            "access_token",
            "AccessToken",
            "refresh_token",
            "RefreshToken",
            "Key",
            "PrivateKey",
        ] {
            if obj.contains_key(k) {
                obj.insert(k.to_string(), Value::String("[redacted]".into()));
            }
        }
    }
    v.to_string()
}

fn parse_token_body(body: &str, status: u16) -> Result<RawLogin, String> {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::String(body.to_string()));

    // Success
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
            .ok_or_else(|| "login success missing private key".to_string())?
            .to_string();

        let unlock = extract_unlock_data(&v)
            .ok_or_else(|| "login success missing master password unlock data".to_string())?;

        let force_password_reset = v
            .get("ForcePasswordReset")
            .or_else(|| v.get("forcePasswordReset"))
            .and_then(|x| x.as_bool())
            .unwrap_or(false);

        let _ = status;
        return Ok(RawLogin::Success {
            access_token: access_token.to_string(),
            private_key,
            unlock,
            force_password_reset,
        });
    }

    // Official new-device protection: error == "device_error"
    let err_code = v.get("error").and_then(|x| x.as_str()).unwrap_or("");
    if err_code == "device_error" || looks_like_device(err_code) {
        return Ok(RawLogin::NeedsNewDeviceOtp { email_hint: None });
    }

    // Structured two-step login providers
    if let Some(providers) = v
        .get("TwoFactorProviders2")
        .or_else(|| v.get("twoFactorProviders2"))
    {
        let email_hint = providers
            .get("1")
            .and_then(|e| e.get("Email").or_else(|| e.get("email")))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let has_email = providers.get("1").is_some() || email_hint.is_some();
        let has_auth = providers.get("0").is_some();
        if has_email {
            return Ok(RawLogin::NeedsEmailTwoFactor { email_hint });
        }
        if has_auth {
            return Ok(RawLogin::NeedsAuthenticator);
        }
    }

    if let Some(arr) = v
        .get("TwoFactorProviders")
        .or_else(|| v.get("twoFactorProviders"))
        .and_then(|x| x.as_array())
    {
        let has_email = arr
            .iter()
            .any(|x| x.as_str() == Some("1") || x.as_i64() == Some(1));
        let has_auth = arr
            .iter()
            .any(|x| x.as_str() == Some("0") || x.as_i64() == Some(0));
        if has_email {
            return Ok(RawLogin::NeedsEmailTwoFactor { email_hint: None });
        }
        if has_auth {
            return Ok(RawLogin::NeedsAuthenticator);
        }
    }

    let message = v
        .pointer("/ErrorModel/Message")
        .or_else(|| v.pointer("/errorModel/message"))
        .and_then(|x| x.as_str())
        .or_else(|| v.get("error_description").and_then(|x| x.as_str()))
        .or_else(|| v.get("error").and_then(|x| x.as_str()))
        .unwrap_or(body)
        .to_string();

    if looks_like_device(&message) {
        return Ok(RawLogin::NeedsNewDeviceOtp { email_hint: None });
    }

    Ok(RawLogin::Failed { message })
}

fn extract_unlock_data(v: &Value) -> Option<MasterPasswordUnlockData> {
    // Modern shape
    let mpu = v
        .pointer("/UserDecryptionOptions/MasterPasswordUnlock")
        .or_else(|| v.pointer("/userDecryptionOptions/masterPasswordUnlock"))
        .or_else(|| v.pointer("/UserDecryptionOptions/masterPasswordUnlock"));

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
        let master_key_wrapped_user_key: EncString = wrapped.parse().ok()?;
        let salt = if salt.is_empty() {
            // fall back to email-like fields
            v.get("Email")
                .or_else(|| v.get("email"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string()
        } else {
            salt
        };
        return Some(MasterPasswordUnlockData {
            kdf,
            master_key_wrapped_user_key,
            salt,
        });
    }

    // Legacy: top-level Key + Kdf fields
    let wrapped = v
        .get("Key")
        .or_else(|| v.get("key"))
        .and_then(|x| x.as_str())?;
    let kdf = parse_kdf(v)?;
    let salt = v
        .get("Email")
        .or_else(|| v.get("email"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let master_key_wrapped_user_key: EncString = wrapped.parse().ok()?;
    Some(MasterPasswordUnlockData {
        kdf,
        master_key_wrapped_user_key,
        salt,
    })
}

fn parse_kdf(v: &Value) -> Option<Kdf> {
    // Nested Kdf object
    if let Some(kdf_obj) = v
        .get("Kdf")
        .or_else(|| v.get("kdf"))
        .filter(|x| x.is_object())
    {
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

    // Flat top-level Kdf / KdfIterations
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
    match kdf_type {
        0 => Some(Kdf::PBKDF2 {
            iterations: std::num::NonZeroU32::new(iterations)
                .unwrap_or(std::num::NonZeroU32::new(600_000).unwrap()),
        }),
        1 => {
            let memory = v
                .get("KdfMemory")
                .or_else(|| v.get("kdfMemory"))
                .or_else(|| v.get("Memory"))
                .or_else(|| v.get("memory"))
                .and_then(|x| x.as_u64())
                .unwrap_or(64) as u32;
            let parallelism = v
                .get("KdfParallelism")
                .or_else(|| v.get("kdfParallelism"))
                .or_else(|| v.get("Parallelism"))
                .or_else(|| v.get("parallelism"))
                .and_then(|x| x.as_u64())
                .unwrap_or(4) as u32;
            Some(Kdf::Argon2id {
                iterations: std::num::NonZeroU32::new(iterations)
                    .unwrap_or(std::num::NonZeroU32::new(3).unwrap()),
                memory: std::num::NonZeroU32::new(memory)
                    .unwrap_or(std::num::NonZeroU32::new(64).unwrap()),
                parallelism: std::num::NonZeroU32::new(parallelism)
                    .unwrap_or(std::num::NonZeroU32::new(4).unwrap()),
            })
        }
        _ => None,
    }
}
