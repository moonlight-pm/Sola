//! Receiving: the signed-in socket, each envelope the server delivers, and
//! what is inside once decrypted.
//!
//! The checks are Signal-Desktop's (`MessageReceiver`) and signalmeow's
//! (`receiving_decrypt.go`): a sealed envelope only for our ACI, its sender
//! certificate signed by Signal and not expired, never from this device; a
//! sync message only from our own account.

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use libsignal_core::{DeviceId, ServiceId};
use libsignal_net::chat::server_requests::ServerEvent;
use libsignal_protocol::{
    CiphertextMessage, CiphertextMessageType, PlaintextContent, PreKeySignalMessage, ProtocolAddress, PublicKey,
    SenderKeyDistributionMessage, SignalMessage, Timestamp, group_decrypt, message_decrypt,
    process_sender_key_distribution_message, sealed_sender_decrypt_to_usmc,
};
use prost::Message;
use tokio::sync::mpsc;

use crate::account::{Account, Identity};
use crate::keys;
use crate::net::{self, Net};
use crate::proto::{self, Content, Envelope, envelope};
use crate::store::{ProtocolStore, Store};

/// The keys Signal's server signs sender certificates under (Signal-Desktop
/// `config/production.json`, `serverTrustRoots`).
const TRUST_ROOTS: &[&str] =
    &["BXu6QIKVz5MA8gstzfOgRQGqyLqOwNKHL6INkv3IHWMF", "BUkY0I+9+oPgDCn4+Ac6Iu813yvqkDr/ga8DzLxFxuk6"];

/// A decrypted envelope: who sent it and what it carries.
pub struct Received {
    pub sender: ServiceId,
    pub sender_device: DeviceId,
    pub timestamp: u64,
    pub content: Content,
}

pub struct Receiver {
    account: Account,
    aci: ProtocolStore,
    pni: Option<ProtocolStore>,
    trust_roots: Vec<PublicKey>,
}

impl Receiver {
    pub fn new(store: &Store, account: Account) -> Result<Self> {
        let trust_roots = TRUST_ROOTS
            .iter()
            .map(|root| PublicKey::deserialize(&BASE64.decode(root)?).map_err(|e| anyhow!("trust root: {e}")))
            .collect::<Result<_>>()?;
        Ok(Self {
            aci: store.protocol(Identity::Aci, account.aci_keys.clone()),
            pni: account.pni.as_ref().map(|(_, keys)| store.protocol(Identity::Pni, keys.clone())),
            account,
            trust_roots,
        })
    }

    /// Sign in and hand each message to `on_message` until the socket closes.
    pub async fn run(&self, net: &Net, mut on_message: impl FnMut(Received)) -> Result<()> {
        let (events, mut event) = mpsc::unbounded_channel();
        let chat = net
            .connect(
                net::CHAT_WEBSOCKET_PATH,
                Some(net::sign_in(&self.account)),
                Box::new(move |e| match ServerEvent::try_from(e) {
                    Ok(e) => drop(events.send(e)),
                    Err(e) => tracing::warn!("chat socket: {e}"),
                }),
                "chat",
            )
            .await
            .context("signing in")?;

        keys::replenish(&chat, &self.aci, Identity::Aci, &self.account.aci_keys).await?;
        if let (Some(store), Some((_, keys))) = (&self.pni, &self.account.pni) {
            keys::replenish(&chat, store, Identity::Pni, keys).await?;
        }

        while let Some(event) = event.recv().await {
            match event {
                ServerEvent::IncomingMessage { envelope, send_ack, .. } => {
                    match self.open(&envelope).await {
                        Ok(Some(received)) => on_message(received),
                        Ok(None) => {}
                        // Nothing asks the sender to send it again yet.
                        Err(e) => tracing::warn!("an envelope was not read: {e:#}"),
                    }
                    send_ack(http::StatusCode::OK).map_err(|e| anyhow!("ack: {e}"))?;
                }
                ServerEvent::QueueEmpty => tracing::info!("caught up with the server's queue"),
                ServerEvent::ServerTimestamp(_) => {}
                ServerEvent::Alerts(alerts) => {
                    if !alerts.is_empty() {
                        tracing::warn!(?alerts, "the server has alerts for this device");
                    }
                }
                ServerEvent::Stopped(cause) => bail!("the chat socket closed: {cause:?}"),
            }
        }
        Ok(())
    }

    /// Decrypt one envelope. `None`: it carries nothing to read (a receipt
    /// the server made).
    async fn open(&self, bytes: &[u8]) -> Result<Option<Received>> {
        let envelope = Envelope::decode(bytes).context("envelope")?;
        let timestamp = envelope.client_timestamp.unwrap_or_default();
        let kind = envelope.r#type();
        let our_aci = ServiceId::from(self.account.aci);
        let destination = service_id(envelope.destination_service_id_binary.as_deref(), envelope.destination_service_id.as_deref())
            .unwrap_or(our_aci);
        let store = if destination == our_aci {
            &self.aci
        } else {
            match (&self.account.pni, &self.pni) {
                (Some((pni, _)), Some(store)) if destination == ServiceId::from(*pni) => store,
                _ => bail!("an envelope for {}, which is not us", destination.service_id_string()),
            }
        };
        let local = ProtocolAddress::new(destination.service_id_string(), self.account.device_id);
        let content = envelope.content.as_deref().unwrap_or_default();

        let (sender, sender_device, plaintext) = match kind {
            envelope::Type::ServerDeliveryReceipt => return Ok(None),
            envelope::Type::UnidentifiedSender => {
                if destination != our_aci {
                    bail!("a sealed envelope that is not for our ACI");
                }
                self.open_sealed(store, &local, content, envelope.server_timestamp.unwrap_or_default()).await?
            }
            envelope::Type::DoubleRatchet | envelope::Type::PrekeyMessage | envelope::Type::PlaintextContent => {
                let sender = service_id(envelope.source_service_id_binary.as_deref(), envelope.source_service_id.as_deref())
                    .context("an envelope with no sender")?;
                let device = envelope.source_device_id.and_then(|d| u8::try_from(d).ok()).and_then(|d| DeviceId::new(d).ok())
                    .context("an envelope with no sender device")?;
                let kind = match kind {
                    envelope::Type::DoubleRatchet => CiphertextMessageType::Whisper,
                    envelope::Type::PrekeyMessage => CiphertextMessageType::PreKey,
                    _ => CiphertextMessageType::Plaintext,
                };
                let remote = ProtocolAddress::new(sender.service_id_string(), device);
                (sender, device, self.decrypt(store, kind, content, &remote, &local).await?)
            }
            envelope::Type::Unknown => bail!("an envelope of unknown type"),
        };

        let content = Content::decode(unpad(&plaintext)?).context("content")?;
        if let Some(distribution) = &content.sender_key_distribution_message {
            let message = SenderKeyDistributionMessage::try_from(distribution.as_slice())
                .map_err(|e| anyhow!("sender key distribution: {e}"))?;
            let remote = ProtocolAddress::new(sender.service_id_string(), sender_device);
            process_sender_key_distribution_message(&remote, &message, &mut self.aci.clone())
                .await
                .map_err(|e| anyhow!("sender key distribution: {e}"))?;
        }
        if matches!(content.content, Some(proto::content::Content::SyncMessage(_))) && sender != our_aci {
            bail!("a sync message from {}, which is not our account", sender.service_id_string());
        }
        Ok(Some(Received { sender, sender_device, timestamp, content }))
    }

    async fn open_sealed(
        &self,
        store: &ProtocolStore,
        local: &ProtocolAddress,
        sealed: &[u8],
        server_timestamp: u64,
    ) -> Result<(ServiceId, DeviceId, Vec<u8>)> {
        let e = |e| anyhow!("sealed envelope: {e}");
        let message = sealed_sender_decrypt_to_usmc(sealed, store).await.map_err(e)?;
        let certificate = message.sender().map_err(e)?;
        let roots: Vec<&PublicKey> = self.trust_roots.iter().collect();
        if !certificate
            .validate_with_trust_roots(&roots, Timestamp::from_epoch_millis(server_timestamp))
            .map_err(e)?
        {
            bail!("a sealed envelope whose sender certificate is not valid");
        }
        let sender = ServiceId::parse_from_service_id_string(certificate.sender_uuid().map_err(e)?)
            .context("the certificate's sender")?;
        let device = certificate.sender_device_id().map_err(e)?;
        if sender == ServiceId::from(self.account.aci) && device == self.account.device_id {
            bail!("a sealed envelope from this device");
        }
        let remote = ProtocolAddress::new(sender.service_id_string(), device);
        let plaintext =
            self.decrypt(store, message.msg_type().map_err(e)?, message.contents().map_err(e)?, &remote, local).await?;
        Ok((sender, device, plaintext))
    }

    async fn decrypt(
        &self,
        store: &ProtocolStore,
        kind: CiphertextMessageType,
        ciphertext: &[u8],
        remote: &ProtocolAddress,
        local: &ProtocolAddress,
    ) -> Result<Vec<u8>> {
        let e = |e| anyhow!("from {remote}: {e}");
        let message = match kind {
            CiphertextMessageType::Whisper => CiphertextMessage::SignalMessage(SignalMessage::try_from(ciphertext).map_err(e)?),
            CiphertextMessageType::PreKey => {
                CiphertextMessage::PreKeySignalMessage(PreKeySignalMessage::try_from(ciphertext).map_err(e)?)
            }
            CiphertextMessageType::SenderKey => {
                return group_decrypt(ciphertext, &mut self.aci.clone(), remote).await.map_err(e);
            }
            // Not encrypted: only ever a notice that decryption failed. The
            // body is a padded `Content`, after a marker byte.
            CiphertextMessageType::Plaintext => {
                return Ok(PlaintextContent::try_from(ciphertext).map_err(e)?.body().to_vec());
            }
        };
        message_decrypt(
            &message,
            remote,
            local,
            &mut store.clone(),
            &mut store.clone(),
            &mut store.clone(),
            store,
            &mut store.clone(),
            &mut rand::rng(),
        )
        .await
        .map_err(e)
    }
}

fn service_id(binary: Option<&[u8]>, text: Option<&str>) -> Option<ServiceId> {
    match (binary, text) {
        (Some(bytes), _) if !bytes.is_empty() => ServiceId::parse_from_service_id_binary(bytes),
        (_, Some(text)) if !text.is_empty() => ServiceId::parse_from_service_id_string(text),
        _ => None,
    }
}

/// Take the padding off: zeros back to a 0x80 marker.
fn unpad(padded: &[u8]) -> Result<&[u8]> {
    match padded.iter().rposition(|&b| b != 0) {
        Some(end) if padded[end] == 0x80 => Ok(&padded[..end]),
        _ => bail!("the content's padding is not valid"),
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use libsignal_core::{Aci, DeviceId};
    use libsignal_protocol::{
        GenericSignedPreKey, IdentityKeyPair, InMemSignalProtocolStore, PreKeyBundle, message_encrypt,
        process_prekey_bundle,
    };

    use super::*;
    use crate::account::IdentityKeys;
    use crate::link::{Linked, first_pre_keys};
    use crate::proto::{DataMessage, SyncMessage};

    #[test]
    fn padding_comes_off_at_the_marker() {
        assert_eq!(unpad(&[1, 2, 0x80, 0, 0]).unwrap(), &[1, 2]);
        assert_eq!(unpad(&[0x80]).unwrap(), &[] as &[u8]);
        assert!(unpad(&[1, 2, 0, 0]).is_err());
        assert!(unpad(&[]).is_err());
    }

    /// A linked device in a store of its own, and the pre-key bundle the
    /// server would hand someone who writes to it.
    fn device(dir: &std::path::Path) -> (Receiver, PreKeyBundle) {
        let store = Store::open(&dir.join("signal.db")).unwrap();
        let keys = IdentityKeys { key_pair: IdentityKeyPair::generate(&mut rand::rng()), registration_id: 7 };
        let (identity, signed, kyber) = first_pre_keys(Identity::Aci, &keys.key_pair).unwrap();
        let bundle = PreKeyBundle::new(
            keys.registration_id,
            DeviceId::new(2).unwrap(),
            None,
            signed.id().unwrap(),
            signed.public_key().unwrap(),
            signed.signature().unwrap(),
            kyber.id().unwrap(),
            kyber.public_key().unwrap(),
            kyber.signature().unwrap(),
            *keys.key_pair.identity_key(),
        )
        .unwrap();
        let account = Account {
            aci: Aci::from(uuid::Uuid::from_u128(0xb0b)),
            device_id: DeviceId::new(2).unwrap(),
            password: "secret".into(),
            aci_keys: keys,
            pni: None,
            number: None,
            profile_key: vec![1; 32],
            account_entropy_pool: None,
            media_root_backup_key: None,
            ephemeral_backup_key: None,
            read_receipts: true,
        };
        store.save_link(&Linked { account, pre_keys: vec![(identity, signed, kyber)] }).unwrap();
        // Read back, so the test covers what a later run starts from.
        let account = store.account().unwrap().unwrap();
        (Receiver::new(&store, account).unwrap(), bundle)
    }

    /// `content` from a stranger, as the server would deliver it to `to`.
    async fn envelope_from(
        stranger: &mut InMemSignalProtocolStore,
        from: Aci,
        to: &Receiver,
        content: Content,
    ) -> Vec<u8> {
        let remote = ProtocolAddress::new(to.account.aci.service_id_string(), to.account.device_id);
        let local = ProtocolAddress::new(from.service_id_string(), DeviceId::new(1).unwrap());
        let mut padded = content.encode_to_vec();
        padded.push(0x80);
        let message = message_encrypt(
            &padded,
            &remote,
            &local,
            &mut stranger.session_store,
            &mut stranger.identity_store,
            SystemTime::now(),
            &mut rand::rng(),
        )
        .await
        .unwrap();
        assert_eq!(message.message_type(), CiphertextMessageType::PreKey);
        Envelope {
            r#type: Some(envelope::Type::PrekeyMessage as i32),
            source_service_id: Some(from.service_id_string()),
            source_device_id: Some(1),
            destination_service_id: Some(to.account.aci.service_id_string()),
            client_timestamp: Some(1234),
            content: Some(message.serialize().to_vec()),
            ..Default::default()
        }
        .encode_to_vec()
    }

    #[tokio::test]
    async fn a_first_message_opens_once_and_a_strangers_sync_is_refused() {
        let dir = std::env::temp_dir().join(format!("sola-signal-test-{}", uuid::Uuid::new_v4()));
        let (receiver, bundle) = device(&dir);

        let alice = Aci::from(uuid::Uuid::from_u128(0xa11ce));
        let mut alice_store =
            InMemSignalProtocolStore::new(IdentityKeyPair::generate(&mut rand::rng()), 9).unwrap();
        process_prekey_bundle(
            &ProtocolAddress::new(receiver.account.aci.service_id_string(), receiver.account.device_id),
            &ProtocolAddress::new(alice.service_id_string(), DeviceId::new(1).unwrap()),
            &mut alice_store.session_store,
            &mut alice_store.identity_store,
            &bundle,
            SystemTime::now(),
            &mut rand::rng(),
        )
        .await
        .unwrap();

        let hello = Content {
            content: Some(proto::content::Content::DataMessage(DataMessage {
                body: Some("hello".into()),
                ..Default::default()
            })),
            ..Default::default()
        };
        let envelope = envelope_from(&mut alice_store, alice, &receiver, hello).await;
        let received = receiver.open(&envelope).await.unwrap().unwrap();
        assert_eq!(received.sender, ServiceId::from(alice));
        assert_eq!(received.timestamp, 1234);
        let Some(proto::content::Content::DataMessage(message)) = received.content.content else {
            panic!("not a data message");
        };
        assert_eq!(message.body.as_deref(), Some("hello"));

        // The same envelope again is a replay.
        assert!(receiver.open(&envelope).await.is_err());

        let sync = Content {
            content: Some(proto::content::Content::SyncMessage(SyncMessage::default())),
            ..Default::default()
        };
        let envelope = envelope_from(&mut alice_store, alice, &receiver, sync).await;
        let refused = receiver.open(&envelope).await.err().expect("a stranger's sync message is refused");
        assert!(refused.to_string().contains("not our account"), "{refused:#}");

        let _ = std::fs::remove_dir_all(dir);
    }
}
