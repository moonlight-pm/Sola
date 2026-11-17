//! The account's record on Signal's storage service: its contacts with
//! their profile keys, its groups, its settings. Encrypted under a key
//! derived from the account's entropy pool, which the phone hands over at
//! link. Read here, never written: the phone keeps it.

use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use hkdf::Hkdf;
use libsignal_account_keys::AccountEntropyPool;
use prost::Message;
use serde::Deserialize;
use sha2::Sha256;
use signal_crypto::CryptographicMac;
use tokio::sync::mpsc;

use super::inbox::{self, Effects};
use super::jobs::Job;
use super::{Shared, profile};
use crate::http;
use crate::net;
use crate::proto::{ManifestRecord, ReadOperation, StorageItems, StorageManifest, StorageRecord, manifest_record, storage_record};

const SERVER: &str = "https://storage.signal.org";
/// Records asked for in one request.
const CHUNK: usize = 1000;

fn mac(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let mut mac = CryptographicMac::new("HmacSha256", key).map_err(|e| anyhow!("hmac: {e}"))?;
    mac.update(data);
    Ok(mac.finalize())
}

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

/// Read the record if it changed since we last did, and take what it says
/// of people and groups into the store.
pub async fn sync(shared: &Shared, more: &mpsc::UnboundedSender<Job>) -> Result<Effects> {
    let store = &shared.store;
    let mut fx = Effects::default();
    let account = shared.account.borrow().clone().context("not linked")?;
    // Without the pool there is nothing to read with; the phone sends it
    // when asked (`Keys`).
    let Some(pool) = account.account_entropy_pool else { return Ok(fx) };
    let pool = AccountEntropyPool::from_str(&pool).map_err(|_| anyhow!("the account's entropy pool does not parse"))?;
    let storage_key = mac(&pool.derive_svr_key(), b"Storage Service Encryption")?;

    let wire = shared.wire.borrow().clone().context("not connected")?;
    let response = net::request(&wire.chat, ::http::Method::GET, "/v1/storage/auth", None, None).await?;
    let credentials: Credentials =
        serde_json::from_slice(response.body.as_deref().unwrap_or_default()).context("storage credentials")?;
    let headers = || {
        vec![
            http::basic(&credentials.username, &credentials.password),
            ("Content-Type".to_owned(), "application/x-protobuf".to_owned()),
        ]
    };

    let known = store.setting("storage-version")?.and_then(|v| v.try_into().ok()).map_or(0, u64::from_le_bytes);
    let path = match known {
        0 => "/v1/storage/manifest".to_owned(),
        version => format!("/v1/storage/manifest/version/{version}"),
    };
    let answer = http::send("GET", format!("{SERVER}{path}"), headers(), None).await?;
    if answer.status == 204 || answer.status == 404 {
        return Ok(fx);
    }
    let sealed = StorageManifest::decode(answer.ok("storage manifest")?.as_slice()).context("storage manifest")?;
    let manifest_key = mac(&storage_key, format!("Manifest_{}", sealed.version).as_bytes())?;
    let manifest = ManifestRecord::decode(profile::decrypt(&manifest_key, &sealed.value)?.as_slice()).context("manifest")?;

    use manifest_record::identifier::Type;
    let wanted: Vec<Vec<u8>> = manifest
        .identifiers
        .iter()
        .filter(|id| matches!(id.r#type(), Type::Contact | Type::Groupv2 | Type::Account))
        .map(|id| id.raw.clone())
        .collect();
    for chunk in wanted.chunks(CHUNK) {
        let ask = ReadOperation { read_key: chunk.to_vec() }.encode_to_vec();
        let answer = http::send("PUT", format!("{SERVER}/v1/storage/read"), headers(), Some(ask)).await?;
        let items = StorageItems::decode(answer.ok("storage records")?.as_slice()).context("storage records")?;
        for item in items.items {
            let key = item_key(&storage_key, &manifest.record_ikm, &item.key)?;
            let record = profile::decrypt(&key, &item.value)
                .and_then(|plain| StorageRecord::decode(plain.as_slice()).context("storage record"));
            match record {
                Ok(record) => take(shared, record, more, &mut fx)?,
                Err(e) => tracing::debug!("a storage record was not read: {e:#}"),
            }
        }
    }
    store.set_setting("storage-version", &manifest.version.to_le_bytes())?;
    fx.people = true;
    Ok(fx)
}

/// A record's key: from the manifest's key material where it has some, the
/// older way from the storage key where it has not.
fn item_key(storage_key: &[u8], record_ikm: &[u8], id: &[u8]) -> Result<Vec<u8>> {
    if record_ikm.is_empty() {
        return mac(storage_key, format!("Item_{}", BASE64.encode(id)).as_bytes());
    }
    let mut key = vec![0u8; 32];
    let info = [b"20240801_SIGNAL_STORAGE_SERVICE_ITEM_".as_slice(), id].concat();
    Hkdf::<Sha256>::new(None, record_ikm).expand(&info, &mut key).map_err(|_| anyhow!("storage record key"))?;
    Ok(key)
}

fn take(shared: &Shared, record: StorageRecord, more: &mpsc::UnboundedSender<Job>, fx: &mut Effects) -> Result<()> {
    let store = &shared.store;
    match record.record {
        Some(storage_record::Record::Contact(contact)) => {
            let aci = (!contact.aci_binary.is_empty()).then_some(contact.aci_binary.as_slice());
            let Some(aci) = inbox::aci(aci, Some(&contact.aci)) else { return Ok(()) };
            let id = store.recipient_for(aci)?;
            let join = |given: &str, family: &str| {
                let name = format!("{given} {family}").trim().to_owned();
                (!name.is_empty()).then_some(name)
            };
            let nickname = contact.nickname.as_ref().and_then(|name| join(&name.given, &name.family));
            let system = join(&contact.system_given_name, &contact.system_family_name);
            let number = (!contact.e164.is_empty()).then_some(contact.e164.as_str());
            store.set_contact(id, number, nickname.or(system).as_deref())?;
            if let Some(name) = join(&contact.given_name, &contact.family_name) {
                store.set_profile_name_if_none(id, &name)?;
            }
            store.set_blocked_one(id, contact.blocked)?;
            if contact.profile_key.len() == 32 && id != shared.me.get() && store.set_profile_key(id, &contact.profile_key)? {
                let _ = more.send(Job::Profile(id));
            }
            if contact.muted_until_timestamp > 0 || contact.archived {
                let conversation = store.conversation_with(id)?;
                store.set_muted(conversation, contact.muted_until_timestamp, contact.archived)?;
            }
        }
        Some(storage_record::Record::GroupV2(group)) => {
            if group.master_key.len() != 32 {
                return Ok(());
            }
            let id = store.group_for(&group.master_key, &inbox::group_identifier(&group.master_key)?)?;
            let conversation = store.conversation_in(id)?;
            store.set_muted(conversation, group.muted_until_timestamp, group.archived)?;
            if store.group(id)?.revision < 0 {
                let _ = more.send(Job::Group(id));
            }
        }
        Some(storage_record::Record::Account(account)) => {
            store.set_read_receipts(account.read_receipts)?;
            fx.people = true;
        }
        _ => {}
    }
    Ok(())
}
