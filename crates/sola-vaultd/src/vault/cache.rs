//! The last sync on disk, as Bitwarden sent it: the items, folders and
//! collections encrypted with the user key (and organization keys), and
//! the organization list whose keys the user key unwraps — what
//! Bitwarden's own clients cache. Age-encrypted with the desk key besides
//! ([`sola_core::Encrypted`]), mode `0600`. An unlock without network
//! loads it, so the vault shows the last synced items; logging out
//! deletes it. Nothing in it opens without the master password.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bitwarden_api_api::models::{ProfileResponseModel, SyncResponseModel};
use bitwarden_sync::{SyncHandler, SyncHandlerError};
use serde::{Deserialize, Serialize};
use sola_core::Encrypted;
use tracing::{info, warn};

const FILE_NAME: &str = "sync.json";

#[derive(Serialize, Deserialize)]
struct OnDisk {
    /// Whose sync this is: another account's is never loaded.
    user_id: String,
    /// Unix seconds of the sync.
    at: u64,
    sync: Encrypted<SyncResponseModel>,
}

/// A sync as it was cached.
pub struct Cached {
    pub at: u64,
    pub sync: SyncResponseModel,
}

pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// What is kept of a sync: items, folders, collections and the
/// organizations (with their wrapped keys). Not the profile's other
/// keys (the session keeps those), sends, policies or domains.
pub fn trim(response: &SyncResponseModel) -> SyncResponseModel {
    let profile = response.profile.as_deref().map(|p| {
        Box::new(ProfileResponseModel {
            id: p.id,
            organizations: p.organizations.clone(),
            organizations_new: p.organizations_new.clone(),
            provider_organizations: p.provider_organizations.clone(),
            ..Default::default()
        })
    });
    SyncResponseModel {
        profile,
        folders: response.folders.clone(),
        collections: response.collections.clone(),
        ciphers: response.ciphers.clone(),
        ..Default::default()
    }
}

/// The cached sync for `user_id`, if there is one.
pub fn load(dir: &Path, user_id: &str) -> Option<Cached> {
    let path = path(dir);
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "vault: sync cache read failed");
            return None;
        }
    };
    match serde_json::from_str::<OnDisk>(&raw) {
        Ok(disk) if disk.user_id == user_id => Some(Cached {
            at: disk.at,
            sync: disk.sync.0,
        }),
        Ok(_) => {
            info!("vault: sync cache is another account's; ignored");
            None
        }
        Err(e) => {
            warn!(path = %path.display(), error = %e, "vault: sync cache decrypt/parse failed");
            None
        }
    }
}

pub fn save(dir: &Path, user_id: &str, at: u64, response: &SyncResponseModel) {
    let path = path(dir);
    let disk = OnDisk {
        user_id: user_id.to_string(),
        at,
        sync: Encrypted(trim(response)),
    };
    let body = match serde_json::to_vec(&disk) {
        Ok(b) => b,
        Err(e) => {
            warn!(error = %e, "vault: sync cache serialize failed");
            return;
        }
    };
    match crate::paths::write_private(&path, &body) {
        Ok(()) => info!(path = %path.display(), "vault: sync cached"),
        Err(e) => warn!(path = %path.display(), error = %e, "vault: sync cache write failed"),
    }
}

pub fn clear(dir: &Path) {
    let path = path(dir);
    match std::fs::remove_file(&path) {
        Ok(()) => info!(path = %path.display(), "vault: sync cache cleared"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => warn!(path = %path.display(), error = %e, "vault: sync cache clear failed"),
    }
}

/// Keeps every sync that worked (registered last, after the stores took
/// it).
pub struct CacheSyncHandler {
    pub dir: PathBuf,
    pub user_id: String,
}

#[async_trait::async_trait]
impl SyncHandler for CacheSyncHandler {
    async fn on_sync(&self, response: &SyncResponseModel) -> Result<(), SyncHandlerError> {
        save(&self.dir, &self.user_id, super::read::unix_now(), response);
        Ok(())
    }
}

/// Give a cached sync to the handlers that fill the stores, as a sync
/// would.
pub async fn replay(handlers: &[Arc<dyn SyncHandler>], sync: &SyncResponseModel) -> Result<(), SyncHandlerError> {
    for h in handlers {
        h.on_sync(sync).await?;
    }
    for h in handlers {
        h.on_sync_complete().await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use bitwarden_api_api::models::{
        CipherDetailsResponseModel, CipherType, FolderResponseModel, ProfileOrganizationResponseModel,
    };
    use bitwarden_state::repository::Repository;
    use sola_vault_proto::Server;

    use super::*;

    const ID: &str = "1bc9ac1e-f5aa-45f2-94bf-b181009709b8";
    const NAME: &str = "2.pMS6/icTQABtulw52pq2lg==|XXbxKxDTh+mWiN1HjH2N1w==|Q6PkuT+KX/axrgN9ubD5Ajk2YNwxQkgs3WJM0S0wOSg=";

    fn response() -> SyncResponseModel {
        SyncResponseModel {
            profile: Some(Box::new(ProfileResponseModel {
                id: Some(ID.parse().unwrap()),
                key: Some("wrapped".into()),
                private_key: Some("private".into()),
                email: Some("a@b.c".into()),
                organizations: Some(vec![ProfileOrganizationResponseModel {
                    id: Some(ID.parse().unwrap()),
                    name: Some("Work".into()),
                    ..Default::default()
                }]),
                ..Default::default()
            })),
            folders: Some(vec![FolderResponseModel {
                id: Some(ID.parse().unwrap()),
                name: Some(NAME.into()),
                revision_date: Some("2026-10-31T10:00:00Z".into()),
                ..Default::default()
            }]),
            collections: Some(Vec::new()),
            ciphers: Some(vec![CipherDetailsResponseModel {
                id: Some(ID.parse().unwrap()),
                r#type: Some(CipherType::SecureNote),
                name: Some(NAME.into()),
                creation_date: Some("2026-10-31T10:00:00Z".into()),
                revision_date: Some("2026-10-31T10:00:00Z".into()),
                ..Default::default()
            }]),
            sends: Some(Vec::new()),
            policies: Some(Vec::new()),
            ..Default::default()
        }
    }

    #[test]
    fn trimming_keeps_items_and_organizations_only() {
        let t = trim(&response());
        let p = t.profile.as_deref().unwrap();
        assert_eq!(p.organizations.as_ref().unwrap().len(), 1);
        assert!(p.key.is_none() && p.private_key.is_none() && p.email.is_none());
        assert_eq!(t.ciphers.as_ref().unwrap().len(), 1);
        assert_eq!(t.folders.as_ref().unwrap().len(), 1);
        assert!(t.collections.is_some());
        assert!(t.sends.is_none() && t.policies.is_none());
        // The wire form reads back as it was.
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<SyncResponseModel>(&json).unwrap(), t);
    }

    #[tokio::test]
    async fn a_replayed_sync_fills_the_stores() {
        let built = super::super::client::build(&Server::Us, "dev");
        assert!(built.ciphers.list().await.unwrap().is_empty());
        replay(&built.replay, &trim(&response())).await.unwrap();
        let ciphers = built.ciphers.list().await.unwrap();
        assert_eq!(ciphers.len(), 1);
        assert_eq!(ciphers[0].id.unwrap().to_string(), ID);
        assert_eq!(built.folders.list().await.unwrap().len(), 1);
        assert_eq!(built.orgs.organizations()[0].name, "Work");
    }

    #[test]
    fn clearing_removes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(path(dir.path()), b"{}").unwrap();
        clear(dir.path());
        assert!(!path(dir.path()).exists());
        clear(dir.path());
        assert!(load(dir.path(), "u").is_none());
    }
}
