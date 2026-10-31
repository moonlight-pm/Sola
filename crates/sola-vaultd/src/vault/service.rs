//! The unlocked vault: an SDK client holding the user key, the account's
//! tokens, and the synced (encrypted) items. Dropping it drops the keys
//! (the SDK's key store zeroizes them): that is how the vault locks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use bitwarden_api_api::models::CipherRequestModel;
use bitwarden_core::auth::login::TwoFactorEmailRequest;
use bitwarden_core::key_management::MasterPasswordAuthenticationData;
use bitwarden_crypto::MasterKey;
use bitwarden_pm::PasswordManagerClient;
use bitwarden_state::repository::Repository;
use bitwarden_sync::SyncRequest;
use bitwarden_vault::{Cipher, CipherView, DecryptCipherResult, EncryptionContext, Folder};
use sola_vault_proto::{ErrorCode, Server, TwoFactorKind};
use tracing::{info, warn};

use super::client::{self, Built};
use super::errors::{Result, api, err, internal, sdk};
use super::identity::{self, IdentityLogin, LoginStep, RefreshError};
use super::session::{self, Session};

/// What a login came to.
pub enum Login {
    Unlocked(Box<VaultService>),
    NeedsTwoFactor {
        kinds: Vec<TwoFactorKind>,
        preferred: TwoFactorKind,
        email_hint: Option<String>,
        email_sent: bool,
        unsupported: Vec<String>,
    },
}

/// What a sync changed: items and whether the folders did.
#[derive(Debug, Default)]
pub struct SyncChanges {
    pub ids: Vec<String>,
    pub folders: bool,
}

pub struct VaultService {
    pub(super) built: Built,
    session: Session,
    dir: PathBuf,
    /// Bitwarden refused the refresh token since last asked.
    ended: AtomicBool,
}

impl VaultService {
    /// Sign in with the master password, and a code when Bitwarden asked
    /// for one. Saves the locked form of the session (V1).
    pub async fn login(
        dir: &Path,
        server: &Server,
        device_id: &str,
        email: &str,
        password: &str,
        otp: Option<(TwoFactorKind, &str)>,
    ) -> Result<Login> {
        let built = client::build(server, device_id);
        let step = IdentityLogin {
            client: &built.pm.0,
            tokens: &built.tokens,
            server: server.clone(),
            device_id: device_id.to_string(),
            email: email.to_string(),
            password,
            otp,
        }
        .run()
        .await?;
        match step {
            LoginStep::Authenticated(session) => {
                session::save(dir, &session);
                Ok(Login::Unlocked(Box::new(Self {
                    built,
                    session,
                    dir: dir.to_path_buf(),
                    ended: AtomicBool::new(false),
                })))
            }
            LoginStep::NeedsTwoFactor {
                kinds,
                preferred,
                email_hint,
                email_sent,
                unsupported,
            } => Ok(Login::NeedsTwoFactor {
                kinds,
                preferred,
                email_hint,
                email_sent,
                unsupported,
            }),
        }
    }

    /// Unlock a signed-in account with its master password: the KDF, the
    /// wrapped user key, the key store. No round trip.
    pub async fn unlock(dir: &Path, session: Session, device_id: &str, password: &str) -> Result<Self> {
        let wrapped = session.wrapped_user_key()?;
        let master_key = MasterKey::derive(password, session.salt(), &session.kdf)
            .map_err(|e| internal(format!("master key: {e}")))?;
        let user_key = master_key
            .decrypt_user_key(wrapped)
            .map_err(|_| err(ErrorCode::BadPassword, "Wrong master password."))?;
        drop(master_key);
        let built = client::build(&session.server, device_id);
        built
            .tokens
            .set(session.access_token.clone(), session.refresh_token.clone());
        identity::init_crypto(&built.pm.0, &session, user_key).await?;
        info!("vault: unlocked");
        Ok(Self {
            built,
            session,
            dir: dir.to_path_buf(),
            ended: AtomicBool::new(false),
        })
    }

    /// Email a code for a login (or send it again).
    pub async fn send_code(
        server: &Server,
        device_id: &str,
        email: &str,
        password: &str,
        kind: TwoFactorKind,
    ) -> Result<()> {
        let built = client::build(server, device_id);
        match kind {
            TwoFactorKind::NewDevice => {
                let kdf = built
                    .pm
                    .0
                    .auth()
                    .prelogin(email.to_string())
                    .await
                    .map_err(|e| err(ErrorCode::Network, format!("Bitwarden could not be asked ({e}).")))?;
                let auth = MasterPasswordAuthenticationData::derive(password, &kdf, email)
                    .map_err(|e| internal(format!("master key: {e}")))?;
                let hash = auth.master_password_authentication_hash.to_string();
                identity::resend_new_device_otp(&built.pm.0, email, &hash).await
            }
            TwoFactorKind::Email => built
                .pm
                .0
                .auth()
                .send_two_factor_email(&TwoFactorEmailRequest {
                    email: email.to_string(),
                    password: password.to_string(),
                })
                .await
                .map_err(|e| err(ErrorCode::Invalid, format!("Could not send the code ({e})."))),
            TwoFactorKind::Authenticator => Err(err(
                ErrorCode::Invalid,
                "Authenticator codes come from your app — nothing to send.",
            )),
        }
    }

    pub fn pm(&self) -> &PasswordManagerClient {
        &self.built.pm
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    /// The session as it is now (fresh tokens), to keep when locking.
    pub fn into_session(self) -> Session {
        let mut session = self.session.clone();
        if let Some(access) = self.built.tokens.access() {
            session.access_token = access;
        }
        if let Some(refresh) = self.built.tokens.refresh() {
            session.refresh_token = Some(refresh);
        }
        session
    }

    /// Bitwarden refused this session's refresh token (since last asked):
    /// the account must sign in again.
    pub fn take_session_ended(&self) -> bool {
        self.ended.swap(false, Ordering::Relaxed)
    }

    /// Refresh the access token when it is about to expire, and save the
    /// new pair.
    pub async fn ensure_fresh_token(&self) -> Result<()> {
        let Some(access) = self.built.tokens.access() else {
            return Ok(());
        };
        if !session::access_needs_refresh(&access) {
            return Ok(());
        }
        let Some(refresh) = self.built.tokens.refresh() else {
            return Ok(());
        };
        match identity::refresh_access_token(&self.built.pm.0, &refresh).await {
            Ok((access, new_refresh)) => {
                let refresh = new_refresh.or(Some(refresh));
                self.built.tokens.set(access.clone(), refresh.clone());
                let mut saved = self.session.clone();
                saved.access_token = access;
                saved.refresh_token = refresh;
                session::save(&self.dir, &saved);
                Ok(())
            }
            Err(RefreshError::Rejected) => {
                warn!("vault: Bitwarden refused the refresh token");
                self.ended.store(true, Ordering::Relaxed);
                Err(err(
                    ErrorCode::SignedOut,
                    "Bitwarden ended this session. Sign in again.",
                ))
            }
            Err(RefreshError::Unreachable) => Err(err(
                ErrorCode::Network,
                "Bitwarden could not be reached to refresh the session.",
            )),
        }
    }

    /// A full sync (encrypted items into the stores), and what it changed.
    pub async fn sync(&self) -> Result<SyncChanges> {
        self.ensure_fresh_token().await?;
        let before = (revisions(&*self.built.ciphers).await, folder_revisions(&*self.built.folders).await);
        self.built
            .sync
            .sync(SyncRequest {
                force: true,
                exclude_subdomains: None,
            })
            .await
            .map_err(|e| {
                warn!(error = %e, "vault: sync failed");
                err(ErrorCode::Network, format!("Sync failed: {e}"))
            })?;
        let after = (revisions(&*self.built.ciphers).await, folder_revisions(&*self.built.folders).await);
        Ok(SyncChanges {
            ids: changed(&before.0, &after.0),
            folders: before.1 != after.1,
        })
    }

    /// Every item, decrypted, and those that would not decrypt.
    pub async fn all(&self) -> Result<DecryptCipherResult> {
        self.pm()
            .vault()
            .ciphers()
            .get_all()
            .await
            .map_err(|e| internal(format!("The vault could not be read ({e}).")))
    }

    /// One decrypted item. A failure other than "no such id" keeps its
    /// message instead of reading "not found".
    pub async fn cipher(&self, id: &str) -> Result<CipherView> {
        super::write::cipher_id(id)?;
        self.pm()
            .vault()
            .ciphers()
            .get(id)
            .await
            .map_err(|e| sdk("Could not open this item", e))
    }

    /// The encrypted items as synced (exports).
    pub async fn encrypted_ciphers(&self) -> Vec<Cipher> {
        self.built.ciphers.list().await.unwrap_or_default()
    }

    pub async fn encrypted_folders(&self) -> Vec<Folder> {
        self.built.folders.list().await.unwrap_or_default()
    }

    /// POST a new item or PUT an existing one. The item's id. (The caller
    /// syncs after.)
    pub async fn persist(&self, ctx: EncryptionContext) -> Result<String> {
        self.ensure_fresh_token().await?;
        let existing = ctx.cipher.id;
        let blob = ctx.cipher.data.is_some();
        let req: CipherRequestModel = ctx.into();
        info!(
            id = existing.map(|i| i.to_string()).as_deref().unwrap_or("-"),
            blob, "vault: item save"
        );
        let api_client = &self.pm().0.internal.get_api_configurations().api_client;
        let id = match existing {
            Some(id) => {
                api_client
                    .ciphers_api()
                    .put(id.into(), Some(req))
                    .await
                    .map_err(|e| api("update item", e))?;
                id.to_string()
            }
            None => api_client
                .ciphers_api()
                .post(Some(req))
                .await
                .map_err(|e| api("create item", e))?
                .id
                .map(|id| id.to_string())
                .ok_or_else(|| internal("Bitwarden saved the item but sent no id."))?,
        };
        Ok(id)
    }
}

impl Drop for VaultService {
    /// Locking: clear the key store outright, whoever else still holds
    /// the client.
    fn drop(&mut self) {
        self.built.pm.0.internal.get_key_store().clear();
        self.built.tokens.clear();
    }
}

async fn revisions(repo: &dyn Repository<Cipher>) -> HashMap<String, i64> {
    repo.list()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|c| Some((c.id?.to_string(), c.revision_date.timestamp_millis())))
        .collect()
}

async fn folder_revisions(repo: &dyn Repository<Folder>) -> HashMap<String, i64> {
    repo.list()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|f| Some((f.id?.to_string(), f.revision_date.timestamp_millis())))
        .collect()
}

/// Ids added, changed or removed between two snapshots, sorted.
fn changed(before: &HashMap<String, i64>, after: &HashMap<String, i64>) -> Vec<String> {
    let mut ids: Vec<String> = after
        .iter()
        .filter(|(id, rev)| before.get(*id) != Some(rev))
        .map(|(id, _)| id.clone())
        .chain(before.keys().filter(|id| !after.contains_key(*id)).cloned())
        .collect();
    ids.sort();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_are_adds_edits_and_removals() {
        let before: HashMap<String, i64> = [("a".into(), 1), ("b".into(), 1), ("c".into(), 1)].into();
        let after: HashMap<String, i64> = [("a".into(), 1), ("b".into(), 2), ("d".into(), 1)].into();
        assert_eq!(changed(&before, &after), ["b", "c", "d"]);
        assert!(changed(&after, &after).is_empty());
    }
}
