//! Folders (the account's own), organizations and their collections.

use bitwarden_vault::{FolderAddEditRequest, FolderId};
use sola_vault_proto::{Collection, ErrorCode, Folder, Organization};
use tracing::info;

use super::errors::{Result, api, err, internal};
use super::service::VaultService;

/// The folders, by name.
pub async fn list(svc: &VaultService) -> Result<Vec<Folder>> {
    let mut out: Vec<Folder> = svc
        .pm()
        .vault()
        .folders()
        .list()
        .await
        .map_err(|e| internal(format!("The folders could not be read ({e}).")))?
        .into_iter()
        .filter_map(|f| {
            Some(Folder {
                id: f.id?.to_string(),
                name: f.name,
            })
        })
        .collect();
    out.sort_by_key(|f| f.name.to_lowercase());
    Ok(out)
}

/// A new folder's id.
pub async fn create(svc: &VaultService, name: &str) -> Result<String> {
    let name = folder_name(name)?;
    svc.ensure_fresh_token().await?;
    let made = svc
        .pm()
        .vault()
        .folders()
        .create(FolderAddEditRequest { name })
        .await
        .map_err(|e| internal(format!("Could not make the folder ({e}).")))?;
    let id = made
        .id
        .map(|i| i.to_string())
        .ok_or_else(|| internal("Bitwarden made the folder but sent no id."))?;
    info!(%id, "vault: folder created");
    Ok(id)
}

pub async fn rename(svc: &VaultService, id: &str, name: &str) -> Result<()> {
    let name = folder_name(name)?;
    svc.ensure_fresh_token().await?;
    svc.pm()
        .vault()
        .folders()
        .edit(folder_id(id)?, FolderAddEditRequest { name })
        .await
        .map_err(|e| internal(format!("Could not rename the folder ({e}).")))?;
    info!(%id, "vault: folder renamed");
    Ok(())
}

/// Delete a folder; its items stay, in no folder.
pub async fn delete(svc: &VaultService, id: &str) -> Result<()> {
    folder_id(id)?;
    svc.ensure_fresh_token().await?;
    svc.pm()
        .0
        .internal
        .get_api_configurations()
        .api_client
        .folders_api()
        .delete(id)
        .await
        .map_err(|e| api("delete folder", e))?;
    info!(%id, "vault: folder deleted");
    Ok(())
}

pub fn organizations(svc: &VaultService) -> Vec<Organization> {
    svc.built.orgs.organizations()
}

/// The collections this account sees, by organization and name; those
/// whose organization key did not load are left out.
pub fn collections(svc: &VaultService) -> Vec<Collection> {
    let client = svc.pm().vault().collections();
    let mut out: Vec<Collection> = svc
        .built
        .orgs
        .collections()
        .into_iter()
        .filter_map(|c| {
            let view = client
                .decrypt(c)
                .inspect_err(|e| tracing::debug!(error = %e, "vault: collection unreadable"))
                .ok()?;
            Some(Collection {
                id: view.id?.to_string(),
                organization_id: view.organization_id.to_string(),
                name: view.name,
                read_only: view.read_only,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a.organization_id
            .cmp(&b.organization_id)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

fn folder_id(id: &str) -> Result<FolderId> {
    id.parse().map_err(|_| err(ErrorCode::NotFound, "No such folder."))
}

fn folder_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err(ErrorCode::Invalid, "A folder needs a name."));
    }
    Ok(name.to_string())
}
