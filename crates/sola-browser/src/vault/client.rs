//! Vault service: login (custom identity token + crypto), 2FA / new-device
//! email verify, sync, match, fill (official Bitwarden cloud).

use std::sync::Arc;

use bitwarden_api_api::models::CipherRequestModel;
use bitwarden_core::auth::login::TwoFactorEmailRequest;
use bitwarden_core::key_management::{
    BLOB_SECURITY_VERSION, KeySlotIds, MasterPasswordAuthenticationData,
};
use bitwarden_core::{Client, ClientSettings, DeviceType};
use bitwarden_crypto::KeyStore;
use bitwarden_pm::PasswordManagerClient;
use bitwarden_sync::{SyncClient, SyncRequest};
use bitwarden_vault::{
    CipherRepromptType, CipherType, CipherView, EncryptionContext, LoginUriView, LoginView,
    UriMatchType,
};
use thiserror::Error;
use zeroize::Zeroize;

use super::edit::{ItemDraft, apply_draft};
use super::identity::{
    IdentityLogin, RefreshError, TokenCell, apply_persisted_session, build_pm_client,
    refresh_access_token,
};
use super::item::{
    IdentityFillMaterial, ItemRecord, ItemSummary, record_from_view, summary_from_view,
};
use super::match_uri::uri_matches;
use super::persist;

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
    pub uris: Vec<crate::engine::SiteUri>,
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
    /// Bitwarden refused the refresh token; the worker logs out.
    #[error("Bitwarden ended this session. Unlock again.")]
    SessionEnded,
    #[error("vault is locked")]
    Locked,
    #[error("cipher not found")]
    NotFound,
    #[error("{0}")]
    Login(String),
    #[error("{0}")]
    Sync(String),
    #[error("{0}")]
    Other(String),
}

/// Protocol version advertised as `Bitwarden-Client-Version`. Must match the
/// official desktop `YYYY.M.P` shape; this SDK pin is the 2026.8 line.
const BITWARDEN_CLIENT_VERSION: &str = "2026.8.0";

/// Keystore version after V1 account init. Decrypt enforces a login-URI
/// checksum only when that cipher has its own key.
const READ_SECURITY_STATE_VERSION: u64 = 1;

/// While alive, personal cipher writes use blob encryption (security-state 2).
/// Drop restores [`READ_SECURITY_STATE_VERSION`].
///
/// State 2 also makes decrypt discard login URIs that lack a checksum.
/// Official clients only do that when the cipher has its own key. Hold this
/// across `ciphers().encrypt` (and the FIDO calls that encrypt inside the
/// SDK). Do not hold it across `get` / `get_all` / sync.
pub(crate) struct BlobCipherWrites {
    store: KeyStore<KeySlotIds>,
}

impl BlobCipherWrites {
    pub(crate) fn enter(client: &Client) -> Self {
        let store = client.internal.get_key_store().clone();
        store.set_security_state_version(BLOB_SECURITY_VERSION);
        Self { store }
    }
}

impl Drop for BlobCipherWrites {
    fn drop(&mut self) {
        self.store
            .set_security_state_version(READ_SECURITY_STATE_VERSION);
    }
}

/// In-process Bitwarden password-manager client (official cloud).
pub struct VaultService {
    pub(crate) client: PasswordManagerClient,
    /// Long-lived sync client with cipher/folder/crypto handlers registered.
    sync: SyncClient,
    tokens: Arc<TokenCell>,
    email: Option<String>,
    session_authenticated: bool,
    /// Bitwarden refused the refresh token during the last command.
    ended: std::sync::atomic::AtomicBool,
}

impl VaultService {
    pub fn new() -> Self {
        let settings = ClientSettings {
            user_agent: format!("SolaBrowser/{}", env!("CARGO_PKG_VERSION")),
            device_type: DeviceType::LinuxDesktop,
            // Cloud parses this as YYYY.M.P. Crate version is 0.1.0; sending
            // that makes PUT /ciphers return 400 "Update to the latest version
            // of Bitwarden" on blob/passkey items.
            bitwarden_client_version: Some(BITWARDEN_CLIENT_VERSION.into()),
            bitwarden_package_type: Some("desktop".into()),
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
            ended: Default::default(),
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

    /// Restore an unlocked session from disk. `Ok(true)` means logged in.
    pub async fn try_restore(&mut self) -> Result<bool, VaultError> {
        let Some(mut sess) = persist::load() else {
            return Ok(false);
        };
        if persist::access_needs_refresh(&sess.access_token) {
            let Some(refresh) = sess.refresh_token.clone() else {
                persist::clear();
                return Ok(false);
            };
            match refresh_access_token(&self.client.0, &refresh).await {
                Ok((access, new_refresh)) => {
                    sess.access_token = access;
                    if new_refresh.is_some() {
                        sess.refresh_token = new_refresh;
                    }
                    persist::save(&sess);
                }
                Err(RefreshError::Rejected(e)) => {
                    tracing::warn!(error = %e, "vault: session refresh refused — clearing");
                    persist::clear();
                    return Ok(false);
                }
                Err(RefreshError::Unreachable(e)) => {
                    // The keys are local: unlock now, refresh on the next sync.
                    tracing::warn!(error = %e, "vault: session refresh unreachable — restoring anyway");
                }
            }
        }
        apply_persisted_session(&self.client.0, &self.tokens, &sess)
            .await
            .map_err(|e| {
                persist::clear();
                VaultError::Login(e)
            })?;
        self.email = Some(sess.email.clone());
        self.session_authenticated = true;
        tracing::info!(email = %sess.email, "vault: session restored");
        Ok(true)
    }

    /// Master-password re-prompt: does `password` unwrap this account's
    /// user key? (Runs the account's KDF; nothing is stored or sent.)
    pub fn verify_master_password(&self, password: &str) -> Result<bool, VaultError> {
        let sess = persist::load().ok_or(VaultError::NotLoggedIn)?;
        let Some(wrapped) = sess.wrapped_user_key.as_deref() else {
            return Err(VaultError::Other(
                "Log out and unlock again once to use master-password re-prompt.".into(),
            ));
        };
        let wrapped: bitwarden_crypto::EncString = wrapped
            .parse()
            .map_err(|e| VaultError::Other(format!("stored key: {e}")))?;
        let key = bitwarden_crypto::MasterKey::derive(password, &sess.salt, &sess.kdf)
            .map_err(|e| VaultError::Other(format!("master key: {e}")))?;
        Ok(key.decrypt_user_key(wrapped).is_ok())
    }

    /// Bitwarden refused this session's refresh token (since last asked).
    pub fn take_session_ended(&self) -> bool {
        self.ended
            .swap(false, std::sync::atomic::Ordering::Relaxed)
    }

    /// Drop tokens, keys, and the on-disk session. Next use is a full login.
    pub fn logout(&mut self) {
        persist::clear();
        self.tokens.clear();
        *self = Self::new();
        tracing::info!("vault: logged out");
    }

    async fn ensure_fresh_token(&self) -> Result<(), VaultError> {
        let Some(access) = self.tokens.access() else {
            return Ok(());
        };
        if !persist::access_needs_refresh(&access) {
            return Ok(());
        }
        let Some(refresh) = self.tokens.refresh() else {
            return Ok(());
        };
        match refresh_access_token(&self.client.0, &refresh).await {
            Ok((access, new_refresh)) => {
                self.tokens.set(access.clone());
                if new_refresh.is_some() {
                    self.tokens.set_refresh(new_refresh.clone());
                }
                if let Some(mut sess) = persist::load() {
                    sess.access_token = access;
                    if new_refresh.is_some() {
                        sess.refresh_token = new_refresh;
                    }
                    persist::save(&sess);
                }
                Ok(())
            }
            Err(RefreshError::Rejected(e)) => {
                tracing::warn!(error = %e, "vault: refresh token refused");
                self.ended
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                Err(VaultError::SessionEnded)
            }
            Err(RefreshError::Unreachable(e)) => Err(VaultError::Sync(e)),
        }
    }

    /// Force a full vault sync (encrypted ciphers → local state).
    pub async fn sync(&self) -> Result<bool, VaultError> {
        if !self.session_authenticated {
            return Err(VaultError::NotLoggedIn);
        }
        self.ensure_fresh_token().await?;
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
        self.require_unlocked()?;

        let listed = self
            .client
            .vault()
            .ciphers()
            .get_all()
            .await
            .map_err(|e| VaultError::Other(e.to_string()))?;

        let n_ok = listed.successes.len();
        let n_fail = listed.failures.len();
        let n_org = listed
            .successes
            .iter()
            .filter(|v| v.organization_id.is_some())
            .count();
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
            let fail_orgs: Vec<String> = listed
                .failures
                .iter()
                .filter_map(|c| c.organization_id.map(|id| id.to_string()))
                .collect();
            tracing::warn!(
                n = n_fail,
                org_ids = ?fail_orgs,
                "vault: some ciphers failed to decrypt"
            );
        }
        tracing::info!(
            decrypted = n_ok,
            org = n_org,
            personal = n_ok.saturating_sub(n_org),
            decrypt_fail = n_fail,
            matches = out.len(),
            host = %super::match_uri::log_host(page_url),
            "vault: match scan"
        );
        Ok(out)
    }

    fn require_unlocked(&self) -> Result<(), VaultError> {
        if !self.session_authenticated {
            return Err(VaultError::NotLoggedIn);
        }
        if !self.client.is_unlocked() {
            return Err(VaultError::Locked);
        }
        Ok(())
    }

    /// One decrypted cipher. A failure other than "no such id" (a key or
    /// decrypt error) keeps its message instead of reading "not found".
    pub(crate) async fn cipher(&self, id: &str) -> Result<CipherView, VaultError> {
        self.client.vault().ciphers().get(id).await.map_err(|e| {
            let msg = e.to_string();
            if msg.to_lowercase().contains("not found") {
                VaultError::NotFound
            } else {
                tracing::warn!(%id, error = %msg, "vault: cipher open failed");
                VaultError::Other(format!("Could not open this item: {msg}"))
            }
        })
    }

    /// Unlocked session ready for fill / passkey.
    pub fn is_ready_for_passkey(&self) -> bool {
        self.session_authenticated && self.client.is_unlocked()
    }

    pub async fn fill_fields(&self, cipher_id: &str) -> Result<FillMaterial, VaultError> {
        self.require_unlocked()?;

        let view = self.cipher(cipher_id).await?;

        let login = view.login.ok_or(VaultError::NotFound)?;
        let uris = login
            .uris
            .iter()
            .flatten()
            .filter_map(|u| Some(super::scope::site_uri(u.uri.as_deref()?, u.r#match)))
            .collect();
        Ok(FillMaterial {
            username: login.username,
            password: login.password,
            uris,
        })
    }

    pub async fn fill_totp(&self, cipher_id: &str) -> Result<String, VaultError> {
        self.require_unlocked()?;
        let view = self.cipher(cipher_id).await?;
        let login = view.login.ok_or(VaultError::NotFound)?;
        let raw = login
            .totp
            .filter(|s| !s.is_empty())
            .ok_or(VaultError::NotFound)?;
        let spec = super::totp::TotpSpec::parse(&raw).ok_or(VaultError::NotFound)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Ok(spec.code_at(now))
    }

    pub async fn fill_card(&self, cipher_id: &str) -> Result<CardFillMaterial, VaultError> {
        self.require_unlocked()?;

        let view = self.cipher(cipher_id).await?;

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

    /// Every non-deleted, non-archived cipher as a list row (no secrets).
    /// The list, and how many ciphers could not be decrypted (shown, so
    /// items never vanish without a word).
    pub async fn list_items(&self, page_url: &str) -> Result<(Vec<ItemSummary>, usize), VaultError> {
        self.require_unlocked()?;

        let version = self
            .client
            .0
            .internal
            .get_key_store()
            .context()
            .get_security_state_version();
        if version > READ_SECURITY_STATE_VERSION {
            tracing::error!(
                version,
                "vault: security-state would drop checksum-less login URIs"
            );
        }

        let listed = self
            .client
            .vault()
            .ciphers()
            .get_all()
            .await
            .map_err(|e| VaultError::Other(e.to_string()))?;

        let n_fail = listed.failures.len();
        let mru = super::prefs::VaultPrefs::last_used_map();
        let mut out: Vec<ItemSummary> = listed
            .successes
            .iter()
            .filter_map(|view| summary_from_view(view, page_url, &mru))
            .collect();
        out.sort_by(|a, b| {
            b.uri_match
                .cmp(&a.uri_match)
                .then(b.last_used.cmp(&a.last_used))
                .then(a.name.cmp(&b.name))
        });
        if n_fail > 0 {
            tracing::warn!(n = n_fail, "vault: some ciphers failed to decrypt");
        }
        tracing::debug!(
            n = out.len(),
            host = %super::match_uri::log_host(page_url),
            "vault: item list"
        );
        Ok((out, n_fail))
    }

    /// Full decrypted record for the item view.
    pub async fn get_item(&self, cipher_id: &str) -> Result<ItemRecord, VaultError> {
        self.require_unlocked()?;
        let view = self.cipher(cipher_id).await?;
        record_from_view(view).ok_or(VaultError::NotFound)
    }

    pub async fn fill_identity(&self, cipher_id: &str) -> Result<IdentityFillMaterial, VaultError> {
        self.require_unlocked()?;
        let view = self.cipher(cipher_id).await?;
        if view.r#type != CipherType::Identity {
            return Err(VaultError::NotFound);
        }
        let identity = view.identity.ok_or(VaultError::NotFound)?;
        Ok(IdentityFillMaterial::from(&identity))
    }

    /// Encrypt + POST a personal login, then sync so match lists see it.
    pub async fn create_login(
        &self,
        name: String,
        username: Option<String>,
        password: Option<String>,
        uri: Option<String>,
    ) -> Result<(Option<String>, FillMaterial), VaultError> {
        self.require_unlocked()?;

        let view = new_login_view(name, username.clone(), password.clone(), uri);
        let ctx = {
            let _blob = BlobCipherWrites::enter(&self.client.0);
            self.client.vault().ciphers().encrypt(view).await
        }
        .map_err(|e| VaultError::Other(format!("encrypt login: {e}")))?;
        let id = self.persist_encryption_context(ctx).await?;

        Ok((
            id,
            FillMaterial {
                username,
                password,
                uris: Vec::new(),
            },
        ))
    }

    /// Apply a chrome edit onto the live cipher, PUT, then sync.
    pub async fn update_item(&self, draft: ItemDraft) -> Result<ItemRecord, VaultError> {
        self.require_unlocked()?;

        match self.sync().await {
            Err(VaultError::SessionEnded) => return Err(VaultError::SessionEnded),
            Err(e) => tracing::warn!(error = %e, "vault: sync before edit failed"),
            Ok(_) => {}
        }
        let mut view = self.cipher(&draft.id).await?;
        apply_draft(&mut view, &draft).map_err(VaultError::Other)?;
        let record = record_from_view(view.clone()).ok_or(VaultError::NotFound)?;
        let ctx = {
            // `get` above ran at version 1 so checksum-less URIs are still
            // on the view. Raise only for the encrypt.
            let _blob = BlobCipherWrites::enter(&self.client.0);
            self.client.vault().ciphers().encrypt(view).await
        }
        .map_err(|e| VaultError::Other(format!("encrypt item: {e}")))?;
        self.persist_encryption_context(ctx).await?;
        Ok(record)
    }

    /// POST a new cipher or PUT an existing one, then sync.
    pub async fn persist_encryption_context(
        &self,
        ctx: EncryptionContext,
    ) -> Result<Option<String>, VaultError> {
        self.require_unlocked()?;

        self.ensure_fresh_token().await?;
        let existing_id = ctx.cipher.id;
        let blob = ctx.cipher.data.is_some();
        let req: CipherRequestModel = ctx.into();
        tracing::info!(
            id = existing_id.map(|i| i.to_string()).as_deref().unwrap_or("-"),
            blob,
            last_known = req.last_known_revision_date.as_deref().unwrap_or("-"),
            "vault: cipher persist"
        );
        let api = self.client.0.internal.get_api_configurations();

        let id = if let Some(id) = existing_id {
            api.api_client
                .ciphers_api()
                .put(id.into(), Some(req))
                .await
                .map_err(|e| persist_conflict("update item", e))?;
            Some(id.to_string())
        } else {
            let created = api
                .api_client
                .ciphers_api()
                .post(Some(req))
                .await
                .map_err(|e| persist_conflict("create login", e))?;
            created.id.map(|id| id.to_string())
        };

        if let Err(e) = self.sync().await {
            tracing::warn!(error = %e, "vault: persisted cipher but sync failed");
        }
        Ok(id)
    }
}

pub(crate) fn persist_conflict(op: &str, e: impl std::fmt::Display) -> VaultError {
    let s = e.to_string();
    let lower = s.to_lowercase();
    if lower.contains("update to the latest version") {
        VaultError::Other(
            "Bitwarden rejected this save (client too old for the item format). Retry after a Sola browser update."
                .into(),
        )
    } else if lower.contains("out of date")
        || lower.contains("outdated")
        || lower.contains("revision")
        || (lower.contains("version") && lower.contains("old"))
    {
        VaultError::Other(
            "Bitwarden has a newer copy of this item (often after a passkey was used elsewhere). Try Save again."
                .into(),
        )
    } else {
        VaultError::Other(format!("{op}: {s}"))
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
    if view.deleted_date.is_some() || view.archived_date.is_some() {
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

    let last_used = super::item::cipher_last_used(view, &id, mru);

    Some(MatchSummary {
        id,
        name: view.name.clone(),
        username: login.username.clone(),
        uri: matched_uri,
        has_passkey,
        last_used,
    })
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

    #[test]
    fn blob_cipher_writes_raise_security_state_only_while_held() {
        let client = Client::new(None);
        let store = client.internal.get_key_store().clone();
        assert_eq!(
            store.context().get_security_state_version(),
            READ_SECURITY_STATE_VERSION
        );
        {
            let _blob = BlobCipherWrites::enter(&client);
            assert_eq!(
                store.context().get_security_state_version(),
                BLOB_SECURITY_VERSION
            );
        }
        assert_eq!(
            store.context().get_security_state_version(),
            READ_SECURITY_STATE_VERSION
        );
    }

    #[test]
    fn bitwarden_client_version_is_yyyy_m() {
        let y: u32 = BITWARDEN_CLIENT_VERSION
            .split('.')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            y >= 2026,
            "cloud rejects crate version 0.1.0: {BITWARDEN_CLIENT_VERSION}"
        );
    }

    #[test]
    fn persist_conflict_maps_stale_revision() {
        let e = persist_conflict("update item", "Cipher revisionDate is outdated.");
        let s = e.to_string();
        assert!(s.contains("newer copy"), "{s}");
        // Bitwarden's own wording (CipherService.ValidateCipherLastKnownRevisionDate).
        let e = persist_conflict(
            "update item",
            r#"400: {"message":"The cipher you are updating is out of date. Please save your work, sync your vault, and try again."}"#,
        );
        assert!(e.to_string().contains("newer copy"), "{e}");
        let e = persist_conflict("update item", "the version is old");
        assert!(e.to_string().contains("newer copy"));
        let e = persist_conflict("update item", "network down");
        assert!(e.to_string().contains("update item: network down"));
        let e = persist_conflict(
            "update item",
            r#"400: {"message":"Cannot edit item. Update to the latest version of Bitwarden and try again."}"#,
        );
        assert!(e.to_string().contains("client too old"), "{}", e);
    }
}
