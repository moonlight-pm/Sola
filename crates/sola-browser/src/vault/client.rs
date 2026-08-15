//! Vault service: login (custom identity token + crypto), 2FA / new-device
//! email verify, sync, match, fill (official Bitwarden cloud).

use std::sync::Arc;

use bitwarden_api_api::models::CipherRequestModel;
use bitwarden_core::auth::login::TwoFactorEmailRequest;
use bitwarden_core::key_management::MasterPasswordAuthenticationData;
use bitwarden_core::{ClientSettings, DeviceType};
use bitwarden_pm::PasswordManagerClient;
use bitwarden_sync::{SyncClient, SyncRequest};
use bitwarden_vault::{
    CipherRepromptType, CipherType, CipherView, EncryptionContext, LoginUriView, LoginView,
    UriMatchType,
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

/// One card for the picker (no PAN / CVV).
#[derive(Debug, Clone)]
pub struct CardSummary {
    pub id: String,
    pub name: String,
    pub brand: Option<String>,
    /// Last 4 (Amex last 5) for the subtitle only.
    pub last4: Option<String>,
    /// Display expiry (`MM/YY`) when both parts exist.
    pub exp: Option<String>,
    pub last_used: i64,
}

impl CardSummary {
    /// `Visa · •••• 1111` / `•••• 1111` / name-only fallback left to chrome.
    pub fn subtitle(&self) -> String {
        card_subtitle(self.brand.as_deref(), self.last4.as_deref())
    }
}

/// Card fields for page fill — zeroize PAN / CVV on drop.
#[derive(Debug, Clone)]
pub struct CardFillMaterial {
    pub cardholder_name: Option<String>,
    pub number: Option<String>,
    pub exp_month: Option<String>,
    pub exp_year: Option<String>,
    pub code: Option<String>,
    pub brand: Option<String>,
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

impl Drop for CardFillMaterial {
    fn drop(&mut self) {
        if let Some(ref mut n) = self.number {
            n.zeroize();
        }
        if let Some(ref mut c) = self.code {
            c.zeroize();
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

    /// Every non-deleted card (cards rarely have URIs — list all).
    pub async fn list_cards(&self) -> Result<Vec<CardSummary>, VaultError> {
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

        let mru = super::prefs::VaultPrefs::last_used_map();
        let mut out = Vec::new();
        for view in listed.successes {
            if let Some(summary) = card_summary_if_card(&view, &mru) {
                out.push(summary);
            }
        }
        out.sort_by(|a, b| b.last_used.cmp(&a.last_used).then(a.name.cmp(&b.name)));
        tracing::info!(n = out.len(), "vault: card list");
        Ok(out)
    }

    pub async fn fill_card(&self, cipher_id: &str) -> Result<CardFillMaterial, VaultError> {
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

        if view.r#type != CipherType::Card {
            return Err(VaultError::NotFound);
        }
        let card = view.card.ok_or(VaultError::NotFound)?;
        Ok(CardFillMaterial {
            cardholder_name: card.cardholder_name,
            number: card.number,
            exp_month: card.exp_month,
            exp_year: card.exp_year,
            code: card.code,
            brand: card.brand,
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

        let view = new_login_view(name, username.clone(), password.clone(), uri);
        let ctx = self
            .client
            .vault()
            .ciphers()
            .encrypt(view)
            .await
            .map_err(|e| VaultError::Other(format!("encrypt login: {e}")))?;
        let id = self.persist_encryption_context(ctx).await?;

        Ok((id, FillMaterial { username, password }))
    }

    /// POST a new cipher or PUT an existing one, then sync.
    pub async fn persist_encryption_context(
        &self,
        ctx: EncryptionContext,
    ) -> Result<Option<String>, VaultError> {
        if !self.session_authenticated {
            return Err(VaultError::NotLoggedIn);
        }
        if !self.client.is_unlocked() {
            return Err(VaultError::Locked);
        }

        let existing_id = ctx.cipher.id;
        let req: CipherRequestModel = ctx.into();
        let api = self.client.0.internal.get_api_configurations();

        let id = if let Some(id) = existing_id {
            api.api_client
                .ciphers_api()
                .put(id.into(), Some(req))
                .await
                .map_err(|e| VaultError::Other(format!("update login: {e}")))?;
            Some(id.to_string())
        } else {
            let created = api
                .api_client
                .ciphers_api()
                .post(Some(req))
                .await
                .map_err(|e| VaultError::Other(format!("create login: {e}")))?;
            created.id.map(|id| id.to_string())
        };

        if let Err(e) = self.sync().await {
            tracing::warn!(error = %e, "vault: persisted cipher but sync failed");
        }
        Ok(id)
    }
}

/// Decrypted personal login ready to encrypt (create-login and passkey create).
pub fn new_login_view(
    name: String,
    username: Option<String>,
    password: Option<String>,
    uri: Option<String>,
) -> CipherView {
    let name = {
        let t = name.trim();
        if t.is_empty() {
            "Login".to_string()
        } else {
            t.to_string()
        }
    };
    let mut login = LoginView {
        username: username.filter(|s| !s.is_empty()),
        password: password.filter(|s| !s.is_empty()),
        password_revision_date: None,
        uris: None,
        totp: None,
        autofill_on_page_load: None,
        fido2_credentials: None,
    };
    if let Some(uri) = uri.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        login.uris = Some(vec![LoginUriView {
            uri: Some(uri),
            r#match: Some(UriMatchType::Domain),
            uri_checksum: None,
        }]);
    }
    login.generate_checksums();

    let now = chrono::Utc::now();
    CipherView {
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

fn card_summary_if_card(
    view: &CipherView,
    mru: &std::collections::HashMap<String, i64>,
) -> Option<CardSummary> {
    if view.r#type != CipherType::Card {
        return None;
    }
    if view.deleted_date.is_some() {
        return None;
    }
    let card = view.card.as_ref()?;
    let id = view.id.map(|id| id.to_string()).unwrap_or_default();
    if id.is_empty() {
        return None;
    }
    let last_used = cipher_last_used(view, &id, mru);
    Some(CardSummary {
        id,
        name: view.name.clone(),
        brand: card.brand.clone(),
        last4: card_last4(card.number.as_deref()),
        exp: card_exp_display(card.exp_month.as_deref(), card.exp_year.as_deref()),
        last_used,
    })
}

fn cipher_last_used(
    view: &CipherView,
    id: &str,
    mru: &std::collections::HashMap<String, i64>,
) -> i64 {
    let bw_used = view.local_data.as_ref().and_then(|ld| {
        let v = serde_json::to_value(ld).ok()?;
        v.get("lastUsedDate")?.as_i64()
    });
    let ours = mru.get(id).copied();
    ours.into_iter()
        .chain(bw_used)
        .max()
        .unwrap_or_else(|| view.revision_date.timestamp())
}

/// Last 4 digits, or 5 for Amex (34/37). None if the PAN is too short.
pub fn card_last4(number: Option<&str>) -> Option<String> {
    let digits: String = number?.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 4 {
        return None;
    }
    let take = if digits.starts_with("34") || digits.starts_with("37") {
        5.min(digits.len())
    } else {
        4
    };
    Some(digits[digits.len() - take..].to_string())
}

pub fn card_subtitle(brand: Option<&str>, last4: Option<&str>) -> String {
    match (
        brand.filter(|s| !s.is_empty()),
        last4.filter(|s| !s.is_empty()),
    ) {
        (Some(b), Some(n)) => format!("{b} · •••• {n}"),
        (Some(b), None) => b.to_string(),
        (None, Some(n)) => format!("•••• {n}"),
        (None, None) => String::new(),
    }
}

pub fn card_exp_display(month: Option<&str>, year: Option<&str>) -> Option<String> {
    let m = month.map(str::trim).filter(|s| !s.is_empty())?;
    let y = year.map(str::trim).filter(|s| !s.is_empty())?;
    let mm = if m.len() == 1 {
        format!("0{m}")
    } else {
        m.to_string()
    };
    let yy = if y.len() >= 4 { &y[y.len() - 2..] } else { y };
    Some(format!("{mm}/{yy}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last4_visa_and_amex() {
        assert_eq!(
            card_last4(Some("4111111111111111")).as_deref(),
            Some("1111")
        );
        assert_eq!(
            card_last4(Some("3782 822463 10005")).as_deref(),
            Some("10005")
        );
        assert_eq!(card_last4(Some("12")), None);
        assert_eq!(card_last4(None), None);
    }

    #[test]
    fn subtitle_and_exp() {
        assert_eq!(
            card_subtitle(Some("Visa"), Some("1111")),
            "Visa · •••• 1111"
        );
        assert_eq!(card_subtitle(None, Some("4444")), "•••• 4444");
        assert_eq!(
            card_exp_display(Some("3"), Some("2028")).as_deref(),
            Some("03/28")
        );
        assert_eq!(
            card_exp_display(Some("12"), Some("28")).as_deref(),
            Some("12/28")
        );
        assert_eq!(card_exp_display(Some("12"), None), None);
    }
}
