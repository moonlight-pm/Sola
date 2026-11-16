//! Sending: a text to one account, and the transcript of it to our own
//! other devices, so the phone shows what this device sent.
//!
//! As Signal-Desktop's `OutgoingMessage`: one ciphertext for each device of
//! the recipient we hold a session with. The server answers with the devices
//! that are missing, gone or registered again; those are put right and the
//! send is tried once more.
//!
//! Not sealed yet: a sealed send needs the recipient's profile key, and
//! nothing keeps those yet. The server sees who sent it, as it does for a
//! first message from any client.

use std::time::SystemTime;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use libsignal_core::{DeviceId, ServiceId};
use libsignal_net::chat::ChatConnection;
use libsignal_net_chat::api::messages::{
    AuthenticatedChatApi, SingleOutboundUnsealedMessage, UnsealedSendFailure,
};
use libsignal_net_chat::api::{Auth, RequestError};
use libsignal_protocol::{
    CiphertextMessage, IdentityKey, PreKeyBundle, ProtocolAddress, PublicKey, Timestamp, kem, message_encrypt,
    process_prekey_bundle,
};
use prost::Message;
use rand::Rng;
use serde::Deserialize;

use crate::account::{Account, Identity};
use crate::net;
use crate::proto::{self, Content, DataMessage, SyncMessage, sync_message};
use crate::store::{ProtocolStore, Store};

/// How often one send is tried while the server corrects our list of the
/// recipient's devices.
const ATTEMPTS: usize = 3;

/// Padded plaintext is a multiple of this, less one (Signal-Desktop's
/// `PADDING_BLOCK`).
const PADDING_BLOCK: usize = 80;

/// The server's base64, with or without its padding.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

type Outbound = SingleOutboundUnsealedMessage<CiphertextMessage>;

pub struct Sender<'a> {
    account: &'a Account,
    store: ProtocolStore,
    chat: &'a ChatConnection,
}

impl<'a> Sender<'a> {
    /// `chat` is a signed-in connection.
    pub fn new(store: &Store, account: &'a Account, chat: &'a ChatConnection) -> Self {
        Self { store: store.protocol(Identity::Aci, account.aci_keys.clone()), account, chat }
    }

    /// Send `text` to `to`. To our own account it is a Note to Self: only
    /// the transcript goes out.
    pub async fn text(&self, to: ServiceId, text: &str) -> Result<()> {
        let timestamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_millis() as u64;
        let us = ServiceId::from(self.account.aci);
        let message = DataMessage {
            body: Some(text.to_owned()),
            timestamp: Some(timestamp),
            profile_key: Some(self.account.profile_key.clone()),
            ..Default::default()
        };
        if to != us {
            let content = Content {
                content: Some(proto::content::Content::DataMessage(message.clone())),
                ..Default::default()
            };
            self.deliver(to, &content, timestamp, true).await?;
        }
        self.deliver(us, &transcript(to, to != us, message, timestamp), timestamp, false)
            .await
            .context("the transcript for our other devices")
    }

    /// Encrypt `content` for each device of `to` and hand it to the server.
    async fn deliver(&self, to: ServiceId, content: &Content, timestamp: u64, urgent: bool) -> Result<()> {
        let name = to.service_id_string();
        let ours = to == ServiceId::from(self.account.aci);
        let plaintext = pad(content.encode_to_vec());
        if open_sessions(&self.store, self.account, &name)?.is_empty() {
            self.start_sessions(to, None).await?;
        }
        let chat: &Auth<ChatConnection> = self.chat.into();
        let timestamp = Timestamp::from_epoch_millis(timestamp);
        for _ in 0..ATTEMPTS {
            let messages = encrypt(&self.store, self.account, &name, &plaintext).await?;
            if messages.is_empty() {
                bail!("no device of {name} to send to");
            }
            let sent = if ours {
                chat.send_sync_message(timestamp, &messages, urgent)
                    .await
                    .map_err(|e| e.flat_map_other(|e| RequestError::Other(UnsealedSendFailure::MismatchedDevices(e))))
            } else {
                chat.send_message(to, timestamp, &messages, false, urgent).await
            };
            let wrong = match sent {
                Ok(()) => return Ok(()),
                Err(RequestError::Other(UnsealedSendFailure::MismatchedDevices(wrong))) => wrong,
                Err(RequestError::Other(UnsealedSendFailure::ServiceIdNotFound)) => {
                    bail!("{name} is not a Signal account")
                }
                Err(e) => bail!("sending: {e}"),
            };
            tracing::info!(
                missing = wrong.missing_devices.len(),
                gone = wrong.extra_devices.len(),
                registered_again = wrong.stale_devices.len(),
                "the server corrected the recipient's devices"
            );
            for device in wrong.extra_devices.iter().chain(&wrong.stale_devices) {
                let address = ProtocolAddress::new(name.clone(), *device);
                self.store.clone().archive_session(&address).await.map_err(|e| anyhow!("{address}: {e}"))?;
            }
            let fresh: Vec<DeviceId> = wrong.missing_devices.into_iter().chain(wrong.stale_devices).collect();
            if !fresh.is_empty() {
                self.start_sessions(to, Some(&fresh)).await?;
            }
        }
        bail!("the server and this device did not come to agree on {name}'s devices")
    }

    /// Fetch `to`'s pre-keys and start a session with each of `devices`
    /// (`None`: every device).
    async fn start_sessions(&self, to: ServiceId, devices: Option<&[DeviceId]>) -> Result<()> {
        let path = format!("/v2/keys/{}/*", to.service_id_string());
        let response = net::request(self.chat, http::Method::GET, &path, None, None).await?;
        let keys: Keys = serde_json::from_slice(response.body.as_deref().unwrap_or_default()).context("pre-keys")?;
        start_sessions(&self.store, self.account, to, keys, devices).await
    }
}

/// What the server answers to `GET /v2/keys/<account>/<device>`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Keys {
    identity_key: String,
    devices: Vec<DeviceKeys>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceKeys {
    device_id: u32,
    registration_id: u32,
    signed_pre_key: SignedKey,
    /// None when the device's one-time keys have run out.
    pre_key: Option<OneTimeKey>,
    pq_pre_key: SignedKey,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignedKey {
    key_id: u32,
    public_key: String,
    signature: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OneTimeKey {
    key_id: u32,
    public_key: String,
}

/// Start a session with each device in `keys` that is wanted. libsignal
/// checks each signed key against the account's identity key, and the store
/// refuses an identity key that differs from the one it knows.
async fn start_sessions(
    store: &ProtocolStore,
    account: &Account,
    to: ServiceId,
    keys: Keys,
    wanted: Option<&[DeviceId]>,
) -> Result<()> {
    fn e(e: impl std::fmt::Display) -> anyhow::Error {
        anyhow!("pre-keys: {e}")
    }
    let name = to.service_id_string();
    let ours = to == ServiceId::from(account.aci);
    let identity = IdentityKey::decode(&BASE64.decode(&keys.identity_key)?).map_err(e)?;
    if ours && &identity != account.aci_keys.key_pair.identity_key() {
        bail!("the server's identity key for our own account is not ours");
    }
    let local = ProtocolAddress::new(account.aci.service_id_string(), account.device_id);
    for device in keys.devices {
        let id = DeviceId::try_from(device.device_id).map_err(|_| anyhow!("pre-keys: a device id out of range"))?;
        if ours && id == account.device_id || wanted.is_some_and(|wanted| !wanted.contains(&id)) {
            continue;
        }
        let one_time = match &device.pre_key {
            Some(key) => Some((key.key_id.into(), PublicKey::deserialize(&BASE64.decode(&key.public_key)?).map_err(e)?)),
            None => None,
        };
        let bundle = PreKeyBundle::new(
            device.registration_id,
            id,
            one_time,
            device.signed_pre_key.key_id.into(),
            PublicKey::deserialize(&BASE64.decode(&device.signed_pre_key.public_key)?).map_err(e)?,
            BASE64.decode(&device.signed_pre_key.signature)?,
            device.pq_pre_key.key_id.into(),
            kem::PublicKey::deserialize(&BASE64.decode(&device.pq_pre_key.public_key)?).map_err(e)?,
            BASE64.decode(&device.pq_pre_key.signature)?,
            identity,
        )
        .map_err(e)?;
        let remote = ProtocolAddress::new(name.clone(), id);
        process_prekey_bundle(
            &remote,
            &local,
            &mut store.clone(),
            &mut store.clone(),
            &bundle,
            SystemTime::now(),
            &mut rand::rng(),
        )
        .await
        .map_err(|e| anyhow!("a session with {remote}: {e}"))?;
    }
    Ok(())
}

/// The devices of `name` to encrypt for: those we hold a session with, less
/// this one when the account is our own.
fn open_sessions(store: &ProtocolStore, account: &Account, name: &str) -> Result<Vec<(DeviceId, u32)>> {
    let ours = name == account.aci.service_id_string();
    let mut open = store.open_sessions(name).map_err(|e| anyhow!("sessions: {e}"))?;
    open.retain(|(device, _)| !(ours && *device == account.device_id));
    Ok(open)
}

/// One ciphertext of `plaintext` for each device of `name`.
async fn encrypt(store: &ProtocolStore, account: &Account, name: &str, plaintext: &[u8]) -> Result<Vec<Outbound>> {
    let local = ProtocolAddress::new(account.aci.service_id_string(), account.device_id);
    let mut messages = Vec::new();
    for (device_id, registration_id) in open_sessions(store, account, name)? {
        let remote = ProtocolAddress::new(name.to_owned(), device_id);
        let contents = message_encrypt(
            plaintext,
            &remote,
            &local,
            &mut store.clone(),
            &mut store.clone(),
            SystemTime::now(),
            &mut rand::rng(),
        )
        .await
        .map_err(|e| anyhow!("for {remote}: {e}"))?;
        messages.push(Outbound { device_id, registration_id, contents });
    }
    Ok(messages)
}

/// What our other devices are told of a message this one sent.
fn transcript(to: ServiceId, delivered: bool, message: DataMessage, timestamp: u64) -> Content {
    let to = to.service_id_string();
    let status = sync_message::sent::UnidentifiedDeliveryStatus {
        destination_service_id: Some(to.clone()),
        unidentified: Some(false),
        ..Default::default()
    };
    let sent = sync_message::Sent {
        destination_service_id: Some(to),
        timestamp: Some(timestamp),
        message: Some(message),
        unidentified_status: if delivered { vec![status] } else { Vec::new() },
        ..Default::default()
    };
    // A sync message carries 1 to 512 random bytes, so its size says less.
    let mut padding = vec![0; rand::rng().random_range(1..=512)];
    rand::rng().fill(padding.as_mut_slice());
    Content {
        content: Some(proto::content::Content::SyncMessage(SyncMessage {
            content: Some(sync_message::Content::Sent(sent)),
            padding: Some(padding),
            ..Default::default()
        })),
        ..Default::default()
    }
}

/// A 0x80 marker, then zeros up to the block (Signal-Desktop's `padMessage`).
fn pad(mut content: Vec<u8>) -> Vec<u8> {
    let padded = (content.len() + 2).div_ceil(PADDING_BLOCK) * PADDING_BLOCK - 1;
    content.push(0x80);
    content.resize(padded, 0);
    content
}

#[cfg(test)]
mod tests {
    use base64::engine::general_purpose::STANDARD;
    use libsignal_core::Aci;
    use libsignal_protocol::{
        CiphertextMessageType, GenericSignedPreKey, IdentityKeyPair, InMemSignalProtocolStore, KyberPreKeyStore,
        SignedPreKeyStore, message_decrypt,
    };
    use serde_json::json;

    use super::*;
    use crate::account::IdentityKeys;
    use crate::link::first_pre_keys;

    #[test]
    fn padding_ends_one_short_of_a_block() {
        assert_eq!(pad(vec![]).len(), 79);
        assert_eq!(pad(vec![7; 78]).len(), 79);
        assert_eq!(pad(vec![7; 79]).len(), 159);
        assert_eq!(pad(vec![7, 7])[..4], [7, 7, 0x80, 0]);
    }

    fn account(aci: u128, device: u8) -> Account {
        Account {
            aci: Aci::from(uuid::Uuid::from_u128(aci)),
            device_id: DeviceId::new(device).unwrap(),
            password: "secret".into(),
            aci_keys: IdentityKeys { key_pair: IdentityKeyPair::generate(&mut rand::rng()), registration_id: 7 },
            pni: None,
            number: None,
            profile_key: vec![1; 32],
            account_entropy_pool: None,
            media_root_backup_key: None,
            ephemeral_backup_key: None,
            read_receipts: true,
        }
    }

    /// A device with the pre-keys it would have registered, and what the
    /// server would answer about it.
    async fn device(keys: &IdentityKeyPair, id: u8, registration_id: u32) -> (InMemSignalProtocolStore, serde_json::Value) {
        let mut store = InMemSignalProtocolStore::new(*keys, registration_id).unwrap();
        let (_, signed, kyber) = first_pre_keys(Identity::Aci, keys).unwrap();
        store.save_signed_pre_key(signed.id().unwrap(), &signed).await.unwrap();
        store.save_kyber_pre_key(kyber.id().unwrap(), &kyber).await.unwrap();
        let answer = json!({
            "deviceId": id,
            "registrationId": registration_id,
            "signedPreKey": {
                "keyId": u32::from(signed.id().unwrap()),
                "publicKey": STANDARD.encode(signed.public_key().unwrap().serialize()),
                "signature": STANDARD.encode(signed.signature().unwrap()),
            },
            "pqPreKey": {
                "keyId": u32::from(kyber.id().unwrap()),
                "publicKey": STANDARD.encode(kyber.public_key().unwrap().serialize()),
                "signature": STANDARD.encode(kyber.signature().unwrap()),
            },
        });
        (store, answer)
    }

    async fn read(device: &mut InMemSignalProtocolStore, message: &Outbound, from: &ProtocolAddress, to: &ProtocolAddress) -> Content {
        assert_eq!(message.contents.message_type(), CiphertextMessageType::PreKey);
        let plaintext = message_decrypt(
            &message.contents,
            from,
            to,
            &mut device.session_store,
            &mut device.identity_store,
            &mut device.pre_key_store,
            &device.signed_pre_key_store,
            &mut device.kyber_pre_key_store,
            &mut rand::rng(),
        )
        .await
        .unwrap();
        assert_eq!(plaintext.len() % PADDING_BLOCK, PADDING_BLOCK - 1);
        let end = plaintext.iter().rposition(|&b| b != 0).unwrap();
        assert_eq!(plaintext[end], 0x80);
        Content::decode(&plaintext[..end]).unwrap()
    }

    #[tokio::test]
    async fn a_text_reaches_each_device_and_the_transcript_skips_this_one() {
        let dir = std::env::temp_dir().join(format!("sola-signal-test-{}", uuid::Uuid::new_v4()));
        let us = account(0xb0b, 5);
        let store = Store::open(&dir.join("signal.db")).unwrap().protocol(Identity::Aci, us.aci_keys.clone());
        let from = ProtocolAddress::new(us.aci.service_id_string(), us.device_id);

        // Alice has two devices; the keys of both come in one answer.
        let alice = ServiceId::from(Aci::from(uuid::Uuid::from_u128(0xa11ce)));
        let alice_keys = IdentityKeyPair::generate(&mut rand::rng());
        let (mut phone, phone_keys) = device(&alice_keys, 1, 11).await;
        let (mut desktop, desktop_keys) = device(&alice_keys, 2, 22).await;
        let answer = json!({
            "identityKey": STANDARD.encode(alice_keys.identity_key().serialize()),
            "devices": [phone_keys, desktop_keys],
        });
        start_sessions(&store, &us, alice, serde_json::from_value(answer).unwrap(), None).await.unwrap();

        let hello = Content {
            content: Some(proto::content::Content::DataMessage(DataMessage {
                body: Some("hello".into()),
                ..Default::default()
            })),
            ..Default::default()
        };
        let name = alice.service_id_string();
        let messages = encrypt(&store, &us, &name, &pad(hello.encode_to_vec())).await.unwrap();
        assert_eq!(messages.iter().map(|m| (u8::from(m.device_id), m.registration_id)).collect::<Vec<_>>(), [(1, 11), (2, 22)]);
        for (device, message) in [&mut phone, &mut desktop].into_iter().zip(&messages) {
            let to = ProtocolAddress::new(name.clone(), message.device_id);
            assert_eq!(read(device, message, &from, &to).await, hello);
        }

        // Our own account: the phone and this device. Only the phone is written to.
        let (mut our_phone, our_phone_keys) = device(&us.aci_keys.key_pair, 1, 33).await;
        let (_, this_device_keys) = device(&us.aci_keys.key_pair, 5, 7).await;
        let ours = json!({
            "identityKey": STANDARD.encode(us.aci_keys.key_pair.identity_key().serialize()),
            "devices": [our_phone_keys, this_device_keys],
        });
        let our_id = ServiceId::from(us.aci);
        start_sessions(&store, &us, our_id, serde_json::from_value(ours).unwrap(), None).await.unwrap();
        let sync = transcript(alice, true, DataMessage { body: Some("hello".into()), ..Default::default() }, 1234);
        let messages = encrypt(&store, &us, &our_id.service_id_string(), &pad(sync.encode_to_vec())).await.unwrap();
        assert_eq!(messages.len(), 1);
        let to = ProtocolAddress::new(our_id.service_id_string(), messages[0].device_id);
        let Some(proto::content::Content::SyncMessage(read)) = read(&mut our_phone, &messages[0], &from, &to).await.content
        else {
            panic!("not a sync message");
        };
        let Some(sync_message::Content::Sent(sent)) = read.content else { panic!("not a transcript") };
        assert_eq!(sent.destination_service_id, Some(name));
        assert_eq!(sent.message.unwrap().body.as_deref(), Some("hello"));

        // Someone else's key in the answer for our own account is refused.
        let wrong = json!({ "identityKey": STANDARD.encode(alice_keys.identity_key().serialize()), "devices": [] });
        assert!(start_sessions(&store, &us, our_id, serde_json::from_value(wrong).unwrap(), None).await.is_err());

        let _ = std::fs::remove_dir_all(dir);
    }
}
