//! Changing the vault: new items, edits, the trash, favorites, the folder
//! an item is in, moving one into an organization. The caller syncs after
//! each.

use bitwarden_api_api::models::CipherPartialRequestModel;
use bitwarden_collections::collection::CollectionId;
use bitwarden_core::OrganizationId;
use bitwarden_vault::{CipherId, FolderId};
use sola_vault_proto::{ErrorCode, ItemDraft};
use tracing::info;

use super::client::BlobCipherWrites;
use super::edit::apply_draft;
use super::errors::{Result, api, err, internal, sdk};
use super::new_item::new_view;
use super::service::VaultService;

/// A new item from a blank draft. Its id.
pub async fn create(svc: &VaultService, draft: &ItemDraft) -> Result<String> {
    if !draft.is_new() {
        return Err(err(ErrorCode::Invalid, "A new item has no id yet."));
    }
    let mut view = new_view(draft.kind);
    apply_draft(&mut view, draft).map_err(|e| err(ErrorCode::Invalid, e))?;
    let ctx = {
        let _blob = BlobCipherWrites::enter(&svc.pm().0);
        svc.pm().vault().ciphers().encrypt(view).await
    }
    .map_err(|e| internal(format!("encrypt item: {e}")))?;
    let id = svc.persist(ctx).await?;
    info!(%id, kind = ?draft.kind, "vault: item created");
    Ok(id)
}

/// Apply an edit onto the item as the server has it (the caller syncs
/// just before), and save.
pub async fn update(svc: &VaultService, draft: &ItemDraft) -> Result<String> {
    let mut view = svc.cipher(&draft.id).await?;
    apply_draft(&mut view, draft).map_err(|e| err(ErrorCode::Invalid, e))?;
    let ctx = {
        // `get` above ran at state 1, so checksum-less websites are still
        // on the view. Raise it only for the encrypt.
        let _blob = BlobCipherWrites::enter(&svc.pm().0);
        svc.pm().vault().ciphers().encrypt(view).await
    }
    .map_err(|e| internal(format!("encrypt item: {e}")))?;
    let id = svc.persist(ctx).await?;
    info!(%id, "vault: item updated");
    Ok(id)
}

pub async fn trash(svc: &VaultService, id: &str) -> Result<()> {
    svc.ensure_fresh_token().await?;
    svc.pm()
        .vault()
        .ciphers()
        .soft_delete(cipher_id(id)?)
        .await
        .map_err(|e| sdk("Could not move it to the trash", e))?;
    info!(%id, "vault: item trashed");
    Ok(())
}

pub async fn restore(svc: &VaultService, id: &str) -> Result<()> {
    svc.ensure_fresh_token().await?;
    svc.pm()
        .vault()
        .ciphers()
        .restore(cipher_id(id)?)
        .await
        .map_err(|e| sdk("Could not restore it", e))?;
    info!(%id, "vault: item restored");
    Ok(())
}

/// Delete forever.
pub async fn delete(svc: &VaultService, id: &str) -> Result<()> {
    svc.ensure_fresh_token().await?;
    svc.pm()
        .vault()
        .ciphers()
        .delete(cipher_id(id)?)
        .await
        .map_err(|e| sdk("Could not delete it", e))?;
    info!(%id, "vault: item deleted");
    Ok(())
}

/// Favorite and folder: the per-account parts of an item, writable even
/// on an organization item this account cannot edit.
pub async fn set_place(svc: &VaultService, id: &str, folder_id: Option<String>, favorite: bool) -> Result<()> {
    svc.ensure_fresh_token().await?;
    let cid = cipher_id(id)?;
    if let Some(f) = folder_id.as_deref() {
        f.parse::<FolderId>()
            .map_err(|_| err(ErrorCode::Invalid, "No such folder."))?;
    }
    svc.pm()
        .0
        .internal
        .get_api_configurations()
        .api_client
        .ciphers_api()
        .put_partial(
            cid.into(),
            Some(CipherPartialRequestModel {
                folder_id,
                favorite: Some(favorite),
            }),
        )
        .await
        .map_err(|e| api("update item", e))?;
    Ok(())
}

/// Move a personal item into an organization's collections. It can't
/// come back out.
pub async fn share(svc: &VaultService, id: &str, organization_id: &str, collection_ids: &[String]) -> Result<()> {
    svc.ensure_fresh_token().await?;
    let view = svc.cipher(id).await?;
    if view.organization_id.is_some() {
        return Err(err(ErrorCode::Invalid, "This item already belongs to an organization."));
    }
    let org: OrganizationId = organization_id
        .parse()
        .map_err(|_| err(ErrorCode::Invalid, "No such organization."))?;
    let collections = collection_ids
        .iter()
        .map(|c| c.parse::<CollectionId>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| err(ErrorCode::Invalid, "No such collection."))?;
    if collections.is_empty() {
        return Err(err(ErrorCode::Invalid, "Choose at least one collection."));
    }
    svc.pm()
        .vault()
        .ciphers()
        .share_cipher(view, org, collections, None)
        .await
        .map_err(|e| sdk("Could not move it", e))?;
    info!(%id, "vault: item moved into an organization");
    Ok(())
}

pub fn cipher_id(id: &str) -> Result<CipherId> {
    id.parse().map_err(|_| err(ErrorCode::NotFound, "No such item."))
}
