//! Export the account's own items (not organizations') with their
//! folders, as Bitwarden JSON (plain or under a password of its own) or
//! CSV, by the SDK's exporter.

use bitwarden_exporters::{ExportFormat as SdkFormat, ExporterClientExt};
use sola_vault_proto::{ErrorCode, ExportFormat};
use zeroize::Zeroizing;

use super::errors::{Result, err};
use super::service::VaultService;

pub async fn export(svc: &VaultService, format: &ExportFormat) -> Result<Zeroizing<String>> {
    let format = match format {
        ExportFormat::Json => SdkFormat::Json,
        ExportFormat::Csv => SdkFormat::Csv,
        ExportFormat::EncryptedJson { password } => {
            if password.is_empty() {
                return Err(err(ErrorCode::Invalid, "An encrypted export needs a password."));
            }
            SdkFormat::EncryptedJson {
                password: password.clone(),
            }
        }
    };
    let ciphers = svc
        .encrypted_ciphers()
        .await
        .into_iter()
        .filter(|c| c.organization_id.is_none() && c.deleted_date.is_none())
        .collect();
    let folders = svc.encrypted_folders().await;
    let data = svc
        .pm()
        .0
        .exporters()
        .export_vault(folders, ciphers, format)
        .await
        .map_err(|e| err(ErrorCode::Internal, format!("The export failed ({e}).")))?;
    tracing::info!(bytes = data.len(), "vault: exported");
    Ok(Zeroizing::new(data))
}
