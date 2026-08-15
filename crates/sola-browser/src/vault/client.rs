//! Vault service: login (custom identity token + crypto), 2FA / new-device
//! email verify, sync, match, fill (official Bitwarden cloud).

use std::sync::Arc;

use bitwarden_core::auth::login::TwoFactorEmailRequest;
use bitwarden_core::key_management::MasterPasswordAuthenticationData;
use bitwarden_core::{ClientSettings, DeviceType};
use bitwarden_pm::PasswordManagerClient;
use bitwarden_sync::{SyncClient, SyncRequest};
use bitwarden_api_api::models::CipherRequestModel;
use bitwarden_vault::{
    CipherRepromptType, CipherType, CipherView, LoginUriView, LoginView, UriMatchType,
};
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
    /// Best-effort last-used unix seconds (0 = unknown).
    pub last_used: i64,
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
        let mru = super::prefs::VaultPrefs::last_used_map();
        let mut out = Vec::new();
        for view in listed.successes {
            if let Some(summary) = match_summary_if_login(&view, page_url, &mru) {
                out.push(summary);
            }
        }
        // Most recently used first (our fill/passkey clock, then Bitwarden
        // local last-used, then cipher revision).
        out.sort_by(|a, b| b.last_used.cmp(&a.last_used));
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

    /// Encrypt + POST a personal login, then sync so match lists see it.
    pub async fn create_login(
        &self,
        name: String,
        username: Option<String>,
        password: Option<String>,
        uri: Option<String>,
    ) -> Result<(Option<String>, FillMaterial), VaultError> {
        if !self.session_authenticated {
            return Err(VaultError::NotLoggedIn);
        }
        if !self.client.is_unlocked() {
            return Err(VaultError::Locked);
        }

        let name = {
            let t = name.trim();
            if t.is_empty() {
                "Login".to_string()
            } else {
                t.to_string()
            }
        };
        let mut login = LoginView {
            username: username.clone().filter(|s| !s.is_empty()),
            password: password.clone().filter(|s| !s.is_empty()),
            password_revision_date: None,
            uris: None,
            totp: None,
            autofill_on_page_load: None,
            fido2_credentials: None,
        };
        if let Some(uri) = uri
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            login.uris = Some(vec![LoginUriView {
                uri: Some(uri),
                r#match: Some(UriMatchType::Domain),
                uri_checksum: None,
            }]);
        }
        login.generate_checksums();

        let now = chrono::Utc::now();
        let view = CipherView {
            id: None,
            organization_id: None,
            folder_id: None,
            collection_ids: Vec::new(),
            key: None,
            name,
            notes: None,
            r#type: CipherType::Login,
            login: Some(login),
            identity: None,
            card: None,
            secure_note: None,
            ssh_key: None,
            bank_account: None,
            drivers_license: None,
            passport: None,
            favorite: false,
            reprompt: CipherRepromptType::None,
            organization_use_totp: false,
            edit: true,
            permissions: None,
            view_password: true,
            local_data: None,
            attachments: None,
            attachment_decryption_failures: None,
            fields: None,
            password_history: None,
            creation_date: now,
            deleted_date: None,
            revision_date: now,
            archived_date: None,
        };

        let ctx = self
            .client
            .vault()
            .ciphers()
            .encrypt(view)
            .await
            .map_err(|e| VaultError::Other(format!("encrypt login: {e}")))?;

        let mut req: CipherRequestModel = ctx
            .cipher
            .try_into()
            .map_err(|e| VaultError::Other(format!("cipher request: {e}")))?;
        req.encrypted_for = Some(ctx.encrypted_for.into());

        let created = self
            .client
            .0
            .internal
            .get_api_configurations()
            .api_client
            .ciphers_api()
            .post(Some(req))
            .await
            .map_err(|e| VaultError::Other(format!("create login: {e}")))?;
        let id = created.id.map(|id| id.to_string());

        if let Err(e) = self.sync().await {
            tracing::warn!(error = %e, "vault: created login but sync failed");
        }

        Ok((
            id,
            FillMaterial {
                username,
                password,
            },
        ))
    }
}

impl Default for VaultService {
    fn default() -> Self {
        Self::new()
    }
}

fn match_summary_if_login(
    view: &CipherView,
    page_url: &str,
    mru: &std::collections::HashMap<String, i64>,
) -> Option<MatchSummary> {
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

    let bw_used = view.local_data.as_ref().and_then(|ld| {
        let v = serde_json::to_value(ld).ok()?;
        v.get("lastUsedDate")?.as_i64()
    });
    let ours = mru.get(&id).copied();
    let last_used = ours
        .into_iter()
        .chain(bw_used)
        .max()
        .unwrap_or_else(|| view.revision_date.timestamp());

    Some(MatchSummary {
        id,
        name: view.name.clone(),
        username: login.username.clone(),
        uri: matched_uri,
        has_passkey,
        last_used,
    })
}
