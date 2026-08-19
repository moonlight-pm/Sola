//! Load organization vault keys from a Bitwarden sync response.
//!
//! Personal items decrypt with the user key (initialized at login). Organization
//! items are wrapped with per-org keys on `profile.organizations[].key`. Without
//! `initialize_org_crypto`, `ciphers().get_all()` only yields the personal vault.

use std::collections::HashMap;
use std::str::FromStr;

use bitwarden_core::key_management::crypto::InitOrgCryptoRequest;
use bitwarden_core::{Client, OrganizationId};
use bitwarden_crypto::UnsignedSharedKey;
use bitwarden_sync::{SyncHandler, SyncHandlerError};

/// Sync handler that decrypts org keys into the key store after each full sync.
pub struct OrgCryptoSyncHandler {
    client: Client,
}

impl OrgCryptoSyncHandler {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl SyncHandler for OrgCryptoSyncHandler {
    async fn on_sync(
        &self,
        response: &bitwarden_api_api::models::SyncResponseModel,
    ) -> Result<(), SyncHandlerError> {
        let (keys, names) = org_keys_from_sync(response);
        let n = keys.len();
        if n == 0 {
            tracing::info!("vault: no organization keys in sync (personal vault only)");
            return Ok(());
        }
        match self
            .client
            .crypto()
            .initialize_org_crypto(InitOrgCryptoRequest {
                organization_keys: keys,
            })
            .await
        {
            Ok(()) => tracing::info!(n, orgs = ?names, "vault: org crypto initialized"),
            Err(e) => tracing::warn!(n, error = %e, "vault: org crypto init failed"),
        }
        Ok(())
    }
}

/// Encrypted org keys + display names from a sync profile.
///
/// Merges `organizations`, `organizationsNew`, and `providerOrganizations` by
/// id. Entries without a parseable key are skipped (invited / unconfirmed).
pub(crate) fn org_keys_from_sync(
    response: &bitwarden_api_api::models::SyncResponseModel,
) -> (HashMap<OrganizationId, UnsignedSharedKey>, Vec<String>) {
    let mut keys = HashMap::new();
    let mut names = Vec::new();
    let Some(profile) = response.profile.as_deref() else {
        return (keys, names);
    };

    let mut by_id = HashMap::new();
    for org in profile.organizations.iter().flatten() {
        if let Some(id) = org.id {
            by_id.insert(id, (org.key.clone(), org.name.clone()));
        }
    }
    for org in profile.organizations_new.iter().flatten() {
        if let Some(id) = org.id {
            by_id.insert(id, (org.key.clone(), org.name.clone()));
        }
    }
    for org in profile.provider_organizations.iter().flatten() {
        if let Some(id) = org.id {
            by_id
                .entry(id)
                .or_insert((org.key.clone(), org.name.clone()));
        }
    }

    for (id, (key, name)) in by_id {
        let Some(raw) = key else {
            continue;
        };
        if raw.is_empty() {
            continue;
        }
        let org_id = OrganizationId::new(id);
        match UnsignedSharedKey::from_str(&raw) {
            Ok(parsed) => {
                keys.insert(org_id, parsed);
                names.push(
                    name.filter(|s| !s.is_empty())
                        .unwrap_or_else(|| id.to_string()),
                );
            }
            Err(e) => {
                tracing::warn!(
                    org_id = %id,
                    error = %e,
                    "vault: skipping unparseable organization key"
                );
            }
        }
    }
    names.sort();
    (keys, names)
}

#[cfg(test)]
mod tests {
    use bitwarden_api_api::models::{
        ProfileOrganizationResponseModel, ProfileProviderOrganizationResponseModel,
        ProfileResponseModel, SyncResponseModel,
    };

    use super::*;

    // Official SDK test-account org key (`test@bitwarden.com`).
    const ORG_KEY: &str = "4.rY01mZFXHOsBAg5Fq4gyXuklWfm6mQASm42DJpx05a+e2mmp+P5W6r54WU2hlREX0uoTxyP91bKKwickSPdCQQ58J45LXHdr9t2uzOYyjVzpzebFcdMw1eElR9W2DW8wEk9+mvtWvKwu7yTebzND+46y1nRMoFydi5zPVLSlJEf81qZZ4Uh1UUMLwXz+NRWfixnGXgq2wRq1bH0n3mqDhayiG4LJKgGdDjWXC8W8MMXDYx24SIJrJu9KiNEMprJE+XVF9nQVNijNAjlWBqkDpsfaWTUfeVLRLctfAqW1blsmIv4RQ91PupYJZDNc8nO9ZTF3TEVM+2KHoxzDJrLs2Q==";
    const ORG_ID: &str = "1bc9ac1e-f5aa-45f2-94bf-b181009709b8";

    fn org(id: &str, key: Option<&str>, name: &str) -> ProfileOrganizationResponseModel {
        ProfileOrganizationResponseModel {
            id: Some(id.parse().unwrap()),
            key: key.map(str::to_string),
            name: Some(name.into()),
            ..Default::default()
        }
    }

    fn sync_with(
        organizations: Option<Vec<ProfileOrganizationResponseModel>>,
        organizations_new: Option<Vec<ProfileOrganizationResponseModel>>,
        provider_organizations: Option<Vec<ProfileProviderOrganizationResponseModel>>,
    ) -> SyncResponseModel {
        SyncResponseModel {
            profile: Some(Box::new(ProfileResponseModel {
                organizations,
                organizations_new,
                provider_organizations,
                ..Default::default()
            })),
            ..Default::default()
        }
    }

    #[test]
    fn empty_sync_has_no_org_keys() {
        let (keys, names) = org_keys_from_sync(&SyncResponseModel::default());
        assert!(keys.is_empty());
        assert!(names.is_empty());
    }

    #[test]
    fn skips_orgs_without_keys() {
        let response = sync_with(Some(vec![org(ORG_ID, None, "Work")]), None, None);
        let (keys, _) = org_keys_from_sync(&response);
        assert!(keys.is_empty());
    }

    #[test]
    fn parses_organization_key() {
        let response = sync_with(Some(vec![org(ORG_ID, Some(ORG_KEY), "Work")]), None, None);
        let (keys, names) = org_keys_from_sync(&response);
        assert_eq!(keys.len(), 1);
        assert!(keys.contains_key(&ORG_ID.parse().unwrap()));
        assert_eq!(names, vec!["Work"]);
    }

    #[test]
    fn organizations_new_overrides_same_id() {
        let other_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        let response = sync_with(
            Some(vec![
                org(ORG_ID, None, "Old"),
                org(other_id, Some(ORG_KEY), "Keep"),
            ]),
            Some(vec![org(ORG_ID, Some(ORG_KEY), "Work")]),
            None,
        );
        let (keys, mut names) = org_keys_from_sync(&response);
        names.sort();
        assert_eq!(keys.len(), 2);
        assert_eq!(names, vec!["Keep", "Work"]);
    }

    #[test]
    fn includes_provider_orgs_not_already_listed() {
        let provider_id = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
        let provider = ProfileProviderOrganizationResponseModel {
            id: Some(provider_id.parse().unwrap()),
            key: Some(ORG_KEY.into()),
            name: Some("Managed".into()),
            ..Default::default()
        };
        let response = sync_with(
            Some(vec![org(ORG_ID, Some(ORG_KEY), "Work")]),
            None,
            Some(vec![provider]),
        );
        let (keys, mut names) = org_keys_from_sync(&response);
        names.sort();
        assert_eq!(keys.len(), 2);
        assert_eq!(names, vec!["Managed", "Work"]);
    }
}
