//! One-time pre-keys: what another device takes from the server to start a
//! session with this one. The server hands each out once, so the stock is
//! topped up when it runs low (as Signal-Desktop's `AccountManager`:
//! 100 at a time, when fewer than 10 are left).

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use libsignal_net::chat::ChatConnection;
use libsignal_protocol::{GenericSignedPreKey, KeyPair, KyberPreKeyRecord, PreKeyRecord, kem};
use rand::Rng;
use serde::Deserialize;
use serde_json::json;

use crate::account::{Identity, IdentityKeys};
use crate::net;
use crate::store::ProtocolStore;

const BATCH: u32 = 100;
const MINIMUM: u32 = 10;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Counts {
    count: u32,
    pq_count: u32,
}

/// Top up `identity`'s one-time pre-keys on the server if they run low.
pub async fn replenish(
    chat: &ChatConnection,
    store: &ProtocolStore,
    identity: Identity,
    keys: &IdentityKeys,
) -> Result<()> {
    let path = format!("/v2/keys?identity={}", identity.as_str());
    let response = net::request(chat, http::Method::GET, &path, None, None).await?;
    let counts: Counts =
        serde_json::from_slice(response.body.as_deref().unwrap_or_default()).context("the key counts")?;

    let e = |e| anyhow!("pre-key: {e}");
    let mut body = serde_json::Map::new();
    if counts.count < MINIMUM {
        let mut uploads = Vec::new();
        for id in ids() {
            let record = PreKeyRecord::new(id.into(), &KeyPair::generate(&mut rand::rng()));
            store.save_pre_key_record(&record)?;
            uploads.push(json!({
                "keyId": id,
                "publicKey": BASE64.encode(record.public_key().map_err(e)?.serialize()),
            }));
        }
        body.insert("preKeys".into(), uploads.into());
    }
    if counts.pq_count < MINIMUM {
        let mut uploads = Vec::new();
        for id in ids() {
            let record = KyberPreKeyRecord::generate(kem::KeyType::Kyber1024, id.into(), keys.key_pair.private_key())
                .map_err(e)?;
            store.save_kyber_record(&record)?;
            uploads.push(json!({
                "keyId": id,
                "publicKey": BASE64.encode(record.public_key().map_err(e)?.serialize()),
                "signature": BASE64.encode(record.signature().map_err(e)?),
            }));
        }
        body.insert("pqPreKeys".into(), uploads.into());
    }
    if body.is_empty() {
        return Ok(());
    }
    tracing::info!(
        identity = identity.as_str(),
        ec = counts.count,
        pq = counts.pq_count,
        "uploading one-time pre-keys"
    );
    net::request(chat, http::Method::PUT, &path, None, Some(&body.into())).await?;
    Ok(())
}

/// A run of ids from a random start in 24 bits: an id still on the server
/// from an earlier batch is not met again for a long while.
fn ids() -> impl Iterator<Item = u32> {
    let start = rand::rng().random_range(1..=0xff_ffff - BATCH);
    start..start + BATCH
}
