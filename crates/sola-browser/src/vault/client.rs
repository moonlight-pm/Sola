//! Vault service: login (custom identity token + crypto), 2FA / new-device
//! email verify, sync, match, fill (official Bitwarden cloud).

use std::sync::Arc;

use bitwarden_core::auth::login::TwoFactorEmailRequest;
use bitwarden_core::key_management::MasterPasswordAuthenticationData;
use bitwarden_core::{ClientSettings, DeviceType};
use bitwarden_pm::PasswordManagerClient;
use bitwarden_sync::{SyncClient, SyncRequest};
use bitwarden_vault::{CipherType, CipherView};
use thiserror::Error;
use zeroize::Zeroize;

use super::identity::{IdentityLogin, TokenCell, build_pm_client};
use super::match_uri::uri_matches;

/// Non-secret vault status for chrome.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VaultStatus {
    pub logged_in: bool,
    pub unlocked: bool,
    pub email: Option<String>,
}

/// One URI match for the picker (no password).
#[derive(Debug, Clone)]
pub struct MatchSummary {
    pub id: String,
    pub name: String,
    pub username: Option<String>,
    pub uri: Option<String>,
    /// Cipher has at least one FIDO2 / passkey credential.
    pub has_passkey: bool,
}

/// Credentials for page fill — zeroize on drop.
#[derive(Debug, Clone)]
pub struct FillMaterial {
    pub username: Option<String>,
    pub password: Option<String>,
}

impl Drop for FillMaterial {
    fn drop(&mut self) {
        if let Some(ref mut p) = self.password {
            p.zeroize();
        }
        if let Some(ref mut u) = self.username {
            u.zeroize();
        }
    }
}

/// Second-factor channel we can complete in chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwoFactorKind {
    /// Official “new device login protection” — form field `newDeviceOtp`
    /// (not twoFactorToken). Distinct from email 2FA.
    NewDevice,
    /// User-enabled email two-step login (`twoFactorProvider=1`).
    Email,
    /// Authenticator app TOTP (`twoFactorProvider=0`).
    Authenticator,
}

/// Outcome of a password (or 2FA) login attempt.
#[derive(Debug, Clone)]
pub enum LoginOutcome {
    /// Fully authenticated + crypto unlocked.
    Authenticated { force_password_reset: bool },
    /// Need a second factor (email code for new device, TOTP, etc.).
    NeedsTwoFactor {
        kinds: Vec<TwoFactorKind>,
        preferred: TwoFactorKind,
        email_hint: Option<String>,
        email_sent: bool,
    },
}

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("not logged in")]
    NotLoggedIn,
    #[error("vault is locked")]
    Locked,
    #[error("login failed")]
    LoginFailed,
    #[error("cipher not found")]
    NotFound,
    #[error("{0}")]
    Login(String),
    #[error("{0}")]
    Sync(String),
    #[error("{0}")]
    Other(String),
}

/// In-process Bitwarden password-manager client (official cloud).
pub struct VaultService {
    pub(crate) client: PasswordManagerClient,
    /// Long-lived sync client with cipher/folder/crypto handlers registered.
    sync: SyncClient,
    tokens: Arc<TokenCell>,
    email: Option<String>,
    session_authenticated: bool,
}

impl VaultService {
    pub fn new() -> Self {
        let settings = ClientSettings {
            user_agent: format!("SolaBrowser/{}", env!("CARGO_PKG_VERSION")),
            device_type: DeviceType::LinuxDesktop,
            bitwarden_client_version: Some(env!("CARGO_PKG_VERSION").into()),
            bitwarden_package_type: Some("sola-browser".into()),
            ..ClientSettings::default()
        };
        let tokens = Arc::new(TokenCell::default());
        let (client, sync) = build_pm_client(settings, tokens.clone());

        Self {
            client,
            sync,
            tokens,
            email: None,
            session_authenticated: false,
        }
    }

    pub fn status(&self) -> VaultStatus {
        VaultStatus {
            logged_in: self.session_authenticated,
            unlocked: self.client.is_unlocked(),
            email: self.email.clone(),
        }
    }

    /// Login with email + master password (no 2FA token yet).
    pub async fn login(
        &mut self,
        email: String,
        mut password: String,
    ) -> Result<LoginOutcome, VaultError> {
        let outcome = self.login_inner(email, password.clone(), None).await;
        password.zeroize();
        outcome
    }

    /// Complete login with a 2FA / new-device verification code.
    pub async fn login_with_two_factor(
        &mut self,
        email: String,
        mut password: String,
        token: String,
        kind: TwoFactorKind,
        _remember: bool,
    ) -> Result<LoginOutcome, VaultError> {
        let outcome = self
            .login_inner(email, password.clone(), Some((kind, token)))
            .await;
        password.zeroize();
        outcome
    }

    /// Resend OTP. New-device uses `/accounts/resend-new-device-otp`; email 2FA
    /// uses the two-factor email endpoint.
    pub async fn resend_otp(
        &self,
        email: String,
        mut password: String,
        kind: TwoFactorKind,
    ) -> Result<(), VaultError> {
        match kind {
            TwoFactorKind::NewDevice => {
                let kdf = self
                    .client
                    .0
                    .auth()
                    .prelogin(email.clone())
                    .await
                    .map_err(|e| VaultError::Login(e.to_string()))?;
                let auth = MasterPasswordAuthenticationData::derive(&password, &kdf, &email)
                    .map_err(|e| VaultError::Login(e.to_string()))?;
                let hash = auth.master_password_authentication_hash.to_string();
                password.zeroize();
                super::identity::resend_new_device_otp(&self.client.0, &email, &hash)
                    .await
                    .map_err(VaultError::Login)
            }
            TwoFactorKind::Email => {
                let result = self
                    .client
                    .0
                    .auth()
                    .send_two_factor_email(&TwoFactorEmailRequest {
                        email,
                        password: password.clone(),
                    })
                    .await;
                password.zeroize();
                result.map_err(|e| VaultError::Login(e.to_string()))
            }
            TwoFactorKind::Authenticator => {
                password.zeroize();
                Err(VaultError::Login(
                    "Authenticator codes are generated by your app — nothing to resend.".into(),
                ))
            }
        }
    }

    async fn login_inner(
        &mut self,
        email: String,
        password: String,
        otp: Option<(TwoFactorKind, String)>,
    ) -> Result<LoginOutcome, VaultError> {
        // Fresh token state for each login attempt.
        self.tokens.clear();
        self.session_authenticated = false;

        let outcome = IdentityLogin {
            client: &self.client.0,
            tokens: &self.tokens,
            email: email.clone(),
            password,
            otp,
        }
        .run()
        .await
        .map_err(VaultError::Login)?;

        if matches!(outcome, LoginOutcome::Authenticated { .. }) {
            self.email = Some(email);
            self.session_authenticated = true;
        }

        Ok(outcome)
    }

    /// Force a full vault sync (encrypted ciphers → local state).
    pub async fn sync(&self) -> Result<bool, VaultError> {
        if !self.session_authenticated {
            return Err(VaultError::NotLoggedIn);
        }
        // Must use the SyncClient that still has handlers registered —
        // not `self.client.sync()` which builds a fresh empty one.
        self.sync
            .sync(SyncRequest {
                force: true,
                exclude_subdomains: None,
            })
            .await
            .map_err(|e| VaultError::Sync(e.to_string()))
    }

    pub async fn matches_for_url(&self, page_url: &str) -> Result<Vec<MatchSummary>, VaultError> {
        if !self.session_authenticated {
            return Err(VaultError::NotLoggedIn);
        }
        if !self.client.is_unlocked() {
            return Err(VaultError::Locked);
        }

        let listed = self
            .client
            .vault()
            .ciphers()
            .get_all()
            .await
            .map_err(|e| VaultError::Other(e.to_string()))?;

        let n_ok = listed.successes.len();
        let n_fail = listed.failures.len();
        let mut out = Vec::new();
        for view in listed.successes {
            if let Some(summary) = match_summary_if_login(&view, page_url) {
                out.push(summary);
            }
        }
        if n_fail > 0 {
            tracing::warn!(n = n_fail, "vault: some ciphers failed to decrypt");
        }
        tracing::info!(
            decrypted = n_ok,
            decrypt_fail = n_fail,
            matches = out.len(),
            %page_url,
            "vault: match scan"
        );
        Ok(out)
    }

    /// Unlocked session ready for fill / passkey.
    pub fn is_ready_for_passkey(&self) -> bool {
        self.session_authenticated && self.client.is_unlocked()
    }

    pub async fn fill_fields(&self, cipher_id: &str) -> Result<FillMaterial, VaultError> {
        if !self.session_authenticated {
            return Err(VaultError::NotLoggedIn);
        }
        if !self.client.is_unlocked() {
            return Err(VaultError::Locked);
        }

        let view = self
            .client
            .vault()
            .ciphers()
            .get(cipher_id)
            .await
            .map_err(|_| VaultError::NotFound)?;

        let login = view.login.ok_or(VaultError::NotFound)?;
        Ok(FillMaterial {
            username: login.username,
            password: login.password,
        })
    }
}

impl Default for VaultService {
    fn default() -> Self {
        Self::new()
    }
}

fn match_summary_if_login(view: &CipherView, page_url: &str) -> Option<MatchSummary> {
    if view.r#type != CipherType::Login {
        return None;
    }
    if view.deleted_date.is_some() {
        return None;
    }
    let login = view.login.as_ref()?;
    let uris = login.uris.as_ref()?;

    let mut matched_uri = None;
    for u in uris {
        let Some(ref uri) = u.uri else {
            continue;
        };
        if uri_matches(page_url, uri, u.r#match) {
            matched_uri = Some(uri.clone());
            break;
        }
    }
    matched_uri.as_ref()?;

    let id = view.id.map(|id| id.to_string()).unwrap_or_default();
    if id.is_empty() {
        return None;
    }

    let has_passkey = login
        .fido2_credentials
        .as_ref()
        .map(|c| !c.is_empty())
        .unwrap_or(false);

    Some(MatchSummary {
        id,
        name: view.name.clone(),
        username: login.username.clone(),
        uri: matched_uri,
        has_passkey,
    })
}
