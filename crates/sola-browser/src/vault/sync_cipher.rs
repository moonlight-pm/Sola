//! Persist ciphers from a Bitwarden sync response into SDK-managed state.
//!
//! Upstream `PasswordManagerClient::new_with_sync` only registers folder +
//! crypto handlers today; ciphers still need a handler for
//! `vault().ciphers().list()` to see data.

use std::sync::Arc;

use bitwarden_core::{FromClient, require};
use bitwarden_state::repository::{Repository, RepositoryOption};
use bitwarden_sync::{SyncHandler, SyncHandlerError};
use bitwarden_vault::{Cipher, CipherId};

/// Sync handler that replaces all stored ciphers from `SyncResponseModel`.
#[derive(FromClient)]
pub struct CipherSyncHandler {
    repository: Option<Arc<dyn Repository<Cipher>>>,
}

#[async_trait::async_trait]
impl SyncHandler for CipherSyncHandler {
    async fn on_sync(
        &self,
        response: &bitwarden_api_api::models::SyncResponseModel,
    ) -> Result<(), SyncHandlerError> {
        let repository = self.repository.require()?;
        let api_ciphers = require!(response.ciphers.as_ref());

        let ciphers: Vec<(CipherId, Cipher)> = api_ciphers
            .iter()
            .filter_map(|c| {
                Cipher::try_from(c.clone())
                    .inspect_err(|e| {
                        tracing::error!(id = ?c.id, error = %e, "failed to deserialize cipher")
                    })
                    .ok()
                    .and_then(|cipher| {
                        let id = cipher.id.or_else(|| {
                            tracing::error!("skipping cipher with missing id");
                            None
                        })?;
                        Some((id, cipher))
                    })
            })
            .collect();

        tracing::info!(count = ciphers.len(), "vault: cipher sync replace");
        repository.replace_all(ciphers).await?;
        Ok(())
    }
}
