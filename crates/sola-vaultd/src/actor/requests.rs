//! Requests on an unlocked vault: reading, writing, the browser's fills
//! and passkeys, the generator, export, and deliveries into other apps.

use std::time::Instant;

use sola_vault_proto::{DeliverField, ErrorCode, ItemDraft, Reply, Request};
use tracing::info;

use super::Vault;
use crate::peer::Peer;
use crate::tickets::RedeemError;
use crate::vault::errors::{Result, err};
use crate::vault::match_uri::log_host;
use crate::vault::{export, folders, generate, passkey, read, write};

impl Vault {
    pub(super) async fn vault_request(&mut self, req: Request, peer: &Peer) -> Result<Reply> {
        match req {
            Request::Generate { options } => Ok(Reply::Generated(generate::generate(&options).await?)),
            Request::Redeem { ticket } => self.redeem(&ticket, peer),
            Request::Sync { .. } => {
                self.sync_now().await?;
                Ok(Reply::Done)
            }
            req => self.read_or_write(req).await,
        }
    }

    async fn read_or_write(&mut self, req: Request) -> Result<Reply> {
        let mru = self.prefs.last_used.clone();
        let svc = self.svc()?;
        match req {
            Request::List { url } => {
                let (items, unreadable) = read::list(svc, url.as_deref().unwrap_or(""), &mru).await?;
                Ok(Reply::Items { items, unreadable })
            }
            Request::Matches { url } => Ok(Reply::Items {
                items: read::matches(svc, &url, &mru).await?,
                unreadable: 0,
            }),
            Request::Get { id } => Ok(Reply::Item(read::get(svc, &id).await?)),
            Request::Draft { id } => Ok(Reply::Draft(ItemDraft::from_record(&read::get(svc, &id).await?))),
            Request::Code { id } => {
                let (code, period, remaining) = read::code(svc, &id).await?;
                Ok(Reply::Code {
                    code,
                    period,
                    remaining,
                })
            }
            Request::Folders => Ok(Reply::Folders(folders::list(svc).await?)),
            Request::Organizations => Ok(Reply::Organizations(folders::organizations(svc))),
            Request::Collections => Ok(Reply::Collections(folders::collections(svc))),
            Request::Export { format } => {
                let mut data = export::export(svc, &format).await?;
                Ok(Reply::Exported {
                    data: std::mem::take(&mut *data),
                })
            }

            Request::FillMaterial { id, url } => {
                let with_code = self.settings.copy_code_on_fill;
                let material = read::fill(svc, &id, &url, with_code).await?;
                info!(%id, host = %log_host(&url), "vault: fill");
                self.prefs.touch(&self.dir, &id);
                Ok(Reply::Fill(material))
            }
            Request::PasskeyCandidates {
                rp_id,
                public_key_json,
            } => Ok(Reply::Passkeys(passkey::candidates(svc, &rp_id, &public_key_json).await?)),
            Request::PasskeyAssert {
                origin,
                public_key_json,
                cipher_id,
            } => {
                let json = passkey::assert(svc, &origin, &public_key_json, &cipher_id).await?;
                self.prefs.touch(&self.dir, &cipher_id);
                self.saved(vec![cipher_id]).await;
                Ok(Reply::Credential { json })
            }
            Request::PasskeyCreate {
                origin,
                public_key_json,
                cipher_id,
            } => {
                self.sync_before_write().await?;
                let (json, id) = passkey::create(self.svc()?, &origin, &public_key_json, cipher_id).await?;
                self.prefs.touch(&self.dir, &id);
                self.saved(vec![id]).await;
                Ok(Reply::Credential { json })
            }

            Request::Deliver { id, field, pid } => {
                let value = read::value(svc, &id, &field).await?;
                let ticket = self.tickets.issue(pid, value, Instant::now());
                info!(%id, field = field_name(&field), pid, "vault: delivery ticket made");
                self.prefs.touch(&self.dir, &id);
                Ok(Reply::Ticket { ticket })
            }

            Request::Create { draft } => {
                let id = write::create(svc, &draft).await?;
                self.saved(vec![id.clone()]).await;
                Ok(Reply::Saved { id })
            }
            Request::Update { draft } => {
                self.sync_before_write().await?;
                let id = write::update(self.svc()?, &draft).await?;
                self.saved(vec![id.clone()]).await;
                Ok(Reply::Saved { id })
            }
            Request::Trash { id } => {
                write::trash(svc, &id).await?;
                self.saved(vec![id]).await;
                Ok(Reply::Done)
            }
            Request::Restore { id } => {
                write::restore(svc, &id).await?;
                self.saved(vec![id]).await;
                Ok(Reply::Done)
            }
            Request::Delete { id } => {
                write::delete(svc, &id).await?;
                self.saved(vec![id]).await;
                Ok(Reply::Done)
            }
            Request::SetFavorite { id, favorite } => {
                let folder = svc.cipher(&id).await?.folder_id.map(|f| f.to_string());
                write::set_place(svc, &id, folder, favorite).await?;
                self.saved(vec![id]).await;
                Ok(Reply::Done)
            }
            Request::MoveToFolder { id, folder_id } => {
                let favorite = svc.cipher(&id).await?.favorite;
                write::set_place(svc, &id, folder_id, favorite).await?;
                self.saved(vec![id]).await;
                Ok(Reply::Done)
            }
            Request::ShareToOrganization {
                id,
                organization_id,
                collection_ids,
            } => {
                write::share(svc, &id, &organization_id, &collection_ids).await?;
                self.saved(vec![id]).await;
                Ok(Reply::Done)
            }
            Request::CreateFolder { name } => {
                let id = folders::create(svc, &name).await?;
                self.saved(Vec::new()).await;
                Ok(Reply::Saved { id })
            }
            Request::RenameFolder { id, name } => {
                folders::rename(svc, &id, &name).await?;
                self.saved(Vec::new()).await;
                Ok(Reply::Done)
            }
            Request::DeleteFolder { id } => {
                folders::delete(svc, &id).await?;
                self.saved(Vec::new()).await;
                Ok(Reply::Done)
            }
            other => {
                tracing::error!(request = ?std::mem::discriminant(&other), "vault: request not routed");
                Err(err(ErrorCode::Internal, "This request is not handled."))
            }
        }
    }

    /// Sync before an edit, so the edit lands on the server's copy. Only a
    /// session Bitwarden ended stops it.
    async fn sync_before_write(&mut self) -> Result<()> {
        match self.sync_now().await {
            Err(e) if e.code == ErrorCode::SignedOut => Err(e),
            Err(e) => {
                tracing::warn!(error = %e, "vault: sync before the edit failed");
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    }

    /// After a write: sync, and tell subscribers `ids` changed. The write
    /// stands even when the sync fails.
    async fn saved(&mut self, ids: Vec<String>) {
        if let Err(e) = self.sync_with(ids).await {
            tracing::warn!(error = %e, "vault: saved, but the sync after failed");
        }
    }

    fn redeem(&mut self, ticket: &str, peer: &Peer) -> Result<Reply> {
        match self.tickets.redeem(ticket, peer.pid, Instant::now()) {
            Ok(mut value) => {
                info!(pid = peer.pid, exe = %peer.exe, "vault: delivery redeemed");
                Ok(Reply::Value(std::mem::take(&mut *value)))
            }
            Err(RedeemError::Unknown) => Err(err(ErrorCode::NotFound, "This delivery has expired.")),
            Err(RedeemError::WrongProcess) => {
                tracing::warn!(pid = peer.pid, exe = %peer.exe, "vault: delivery redeemed by another process; refused");
                Err(err(ErrorCode::Denied, "This delivery is for another window."))
            }
        }
    }
}

fn field_name(field: &DeliverField) -> &str {
    match field {
        DeliverField::Username => "username",
        DeliverField::Password => "password",
        DeliverField::Code => "code",
        DeliverField::Field { key } => key,
    }
}
