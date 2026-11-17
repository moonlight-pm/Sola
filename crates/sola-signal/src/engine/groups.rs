//! Groups: their name and members are kept by Signal's group server,
//! encrypted under the group's key, which only members hold. We prove
//! membership with a credential that does not say which member we are.

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use libsignal_core::{Aci, ServiceId};
use prost::Message;
use rand::Rng;
use serde::Deserialize;
use zkgroup::ServerPublicParams;
use zkgroup::auth::AuthCredentialWithPniResponse;
use zkgroup::groups::{GroupMasterKey, GroupSecretParams, ProfileKeyCiphertext, UuidCiphertext};

use super::Shared;
use crate::http;
use crate::net;
use crate::proto::groups::{Group, GroupAttributeBlob, GroupResponse, group_attribute_blob};

const SERVER: &str = "https://storage.signal.org";
const DAY: u64 = 86_400;

/// The public half of the keys Signal's server signs credentials with
/// (Signal-Desktop `config/production.json`, `serverPublicParams`).
fn server_params() -> Result<ServerPublicParams> {
    let bytes = BASE64.decode(include_str!("server_public_params.b64").trim())?;
    zkgroup::deserialize(&bytes).map_err(|_| anyhow!("Signal's server parameters do not parse"))
}

/// A group as its members see it.
pub struct State {
    pub revision: u32,
    pub title: String,
    /// Seconds a message lives; 0 is forever.
    pub timer: u32,
    pub members: Vec<(Aci, Option<[u8; 32]>)>,
}

#[derive(Deserialize)]
struct Credentials {
    credentials: Vec<Credential>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Credential {
    credential: String,
    redemption_time: u64,
}

/// Today's credential from the chat server, kept for the day.
async fn credential(shared: &Shared) -> Result<(u64, Vec<u8>)> {
    let today = crate::store::now() / 1000 / DAY * DAY;
    if let Some((day, credential)) = shared.group_credential.borrow().clone()
        && day == today
    {
        return Ok((day, credential));
    }
    let wire = shared.wire.borrow().clone().context("not connected")?;
    let path = format!(
        "/v1/certificate/auth/group?redemptionStartSeconds={today}&redemptionEndSeconds={}&v101=true&zkcCredential=true",
        today + DAY
    );
    let response = net::request(&wire.chat, ::http::Method::GET, &path, None, None).await?;
    let answer: Credentials =
        serde_json::from_slice(response.body.as_deref().unwrap_or_default()).context("group credentials")?;
    let credential = answer
        .credentials
        .iter()
        .find(|credential| credential.redemption_time == today)
        .context("no group credential for today")?;
    let credential = (today, BASE64.decode(&credential.credential)?);
    *shared.group_credential.borrow_mut() = Some(credential.clone());
    Ok(credential)
}

/// The name and password the group server takes for one group.
async fn authorization(shared: &Shared, secret: &GroupSecretParams) -> Result<(String, String)> {
    let account = shared.account.borrow().clone().context("not linked")?;
    let (pni, _) = account.pni.context("an account with no phone-number identity cannot read groups yet")?;
    let (day, credential) = credential(shared).await?;
    let params = server_params()?;
    let credential = AuthCredentialWithPniResponse::new(&credential)
        .map_err(|_| anyhow!("the group credential does not parse"))?
        .receive(&params, account.aci, pni, zkgroup::Timestamp::from_epoch_seconds(day))
        .map_err(|_| anyhow!("the group credential is not Signal's"))?;
    let mut randomness = [0u8; 32];
    rand::rng().fill(&mut randomness);
    let presentation = credential.present(&params, secret, randomness);
    let hex = |bytes: &[u8]| bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    Ok(http::basic(&hex(&zkgroup::serialize(&secret.get_public_params())), &hex(&zkgroup::serialize(&presentation))))
}

/// Fetch a group's state and decrypt it.
pub async fn fetch(shared: &Shared, master_key: &[u8]) -> Result<State> {
    let master_key: [u8; 32] = master_key.try_into().ok().context("a group key that is not 32 bytes")?;
    let secret = GroupSecretParams::derive_from_master_key(GroupMasterKey::new(master_key));
    let headers = vec![authorization(shared, &secret).await?, ("Content-Type".into(), "application/x-protobuf".into())];
    let body = http::send("GET", format!("{SERVER}/v2/groups/"), headers, None).await?.ok("group")?;
    let group = GroupResponse::decode(body.as_slice()).context("group")?.group.context("an answer with no group in it")?;
    decrypt(&secret, &group)
}

fn blob(secret: &GroupSecretParams, sealed: &[u8]) -> Result<Option<group_attribute_blob::Content>> {
    if sealed.is_empty() {
        return Ok(None);
    }
    let plain = secret.decrypt_blob_with_padding(sealed).map_err(|_| anyhow!("a group field that does not decrypt"))?;
    Ok(GroupAttributeBlob::decode(plain.as_slice()).context("group field")?.content)
}

pub fn decrypt(secret: &GroupSecretParams, group: &Group) -> Result<State> {
    let title = match blob(secret, &group.title)? {
        Some(group_attribute_blob::Content::Title(title)) => title.trim().to_owned(),
        _ => String::new(),
    };
    let timer = match blob(secret, &group.disappearing_messages_timer)? {
        Some(group_attribute_blob::Content::DisappearingMessagesDuration(seconds)) => seconds,
        _ => 0,
    };
    let mut members = Vec::new();
    for member in &group.members {
        let id: UuidCiphertext = zkgroup::deserialize(&member.user_id).map_err(|_| anyhow!("a member id that does not parse"))?;
        let ServiceId::Aci(aci) = secret.decrypt_service_id(id).map_err(|_| anyhow!("a member id that does not decrypt"))?
        else {
            bail!("a member that is a phone-number identity");
        };
        // The key lets us read their name; a member without one is still a member.
        let key = zkgroup::deserialize::<ProfileKeyCiphertext>(&member.profile_key)
            .ok()
            .and_then(|key| secret.decrypt_profile_key(key, aci).ok())
            .map(|key| key.get_bytes());
        members.push((aci, key));
    }
    Ok(State { revision: group.version, title, timer, members })
}
