//! The account's organizations and their collections, as the last sync
//! listed them. Collection names stay encrypted here; they are decrypted
//! (with the organization keys) when asked for.

use std::collections::BTreeMap;
use std::sync::Mutex;

use bitwarden_api_api::models::{CollectionDetailsResponseModel, SyncResponseModel};
use bitwarden_collections::collection::Collection;
use bitwarden_sync::{SyncHandler, SyncHandlerError};
use sola_vault_proto::Organization;

#[derive(Default)]
pub struct SyncOrgs {
    organizations: Mutex<Vec<Organization>>,
    collections: Mutex<Vec<CollectionDetailsResponseModel>>,
}

impl SyncOrgs {
    pub fn organizations(&self) -> Vec<Organization> {
        self.organizations.lock().expect("orgs mutex").clone()
    }

    /// The collections, encrypted; those that do not parse are left out.
    pub fn collections(&self) -> Vec<Collection> {
        self.collections
            .lock()
            .expect("collections mutex")
            .iter()
            .filter_map(|c| {
                Collection::try_from(c.clone())
                    .inspect_err(|e| tracing::warn!(error = %e, "vault: collection skipped"))
                    .ok()
            })
            .collect()
    }
}

/// Fills a [`SyncOrgs`] on every sync.
pub struct SyncOrgsHandler(pub std::sync::Arc<SyncOrgs>);

#[async_trait::async_trait]
impl SyncHandler for SyncOrgsHandler {
    async fn on_sync(&self, response: &SyncResponseModel) -> Result<(), SyncHandlerError> {
        let orgs = organizations_from_sync(response);
        let collections = response.collections.clone().unwrap_or_default();
        tracing::info!(
            organizations = orgs.len(),
            collections = collections.len(),
            "vault: organizations synced"
        );
        *self.0.organizations.lock().expect("orgs mutex") = orgs;
        *self.0.collections.lock().expect("collections mutex") = collections;
        Ok(())
    }
}

/// The profile's organizations by id (`organizations`, then
/// `organizationsNew`, then provider organizations not already listed),
/// sorted by name.
fn organizations_from_sync(response: &SyncResponseModel) -> Vec<Organization> {
    let Some(profile) = response.profile.as_deref() else {
        return Vec::new();
    };
    let mut by_id: BTreeMap<String, String> = BTreeMap::new();
    for org in profile
        .organizations
        .iter()
        .flatten()
        .chain(profile.organizations_new.iter().flatten())
    {
        if let Some(id) = org.id {
            by_id.insert(id.to_string(), org.name.clone().unwrap_or_default());
        }
    }
    for org in profile.provider_organizations.iter().flatten() {
        if let Some(id) = org.id {
            by_id
                .entry(id.to_string())
                .or_insert_with(|| org.name.clone().unwrap_or_default());
        }
    }
    let mut out: Vec<Organization> = by_id
        .into_iter()
        .map(|(id, name)| Organization {
            name: if name.is_empty() { id.clone() } else { name },
            id,
        })
        .collect();
    out.sort_by_key(|o| o.name.to_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use bitwarden_api_api::models::{ProfileOrganizationResponseModel, ProfileResponseModel};

    use super::*;

    #[test]
    fn organizations_merge_by_id_and_sort_by_name() {
        let org = |id: &str, name: &str| ProfileOrganizationResponseModel {
            id: Some(id.parse().unwrap()),
            name: Some(name.into()),
            ..Default::default()
        };
        let response = SyncResponseModel {
            profile: Some(Box::new(ProfileResponseModel {
                organizations: Some(vec![
                    org("1bc9ac1e-f5aa-45f2-94bf-b181009709b8", "Work"),
                    org("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "Old"),
                ]),
                organizations_new: Some(vec![org("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "Family")]),
                ..Default::default()
            })),
            ..Default::default()
        };
        let names: Vec<String> = organizations_from_sync(&response)
            .into_iter()
            .map(|o| o.name)
            .collect();
        assert_eq!(names, ["Family", "Work"]);
        assert!(organizations_from_sync(&SyncResponseModel::default()).is_empty());
    }
}
