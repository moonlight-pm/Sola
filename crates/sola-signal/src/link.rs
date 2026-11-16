//! Linking: this device joins the phone's account.
//!
//! As Signal-Desktop does it (`Provisioner`, `AccountManager.#createAccount`,
//! `linkDevice`): open the provisioning socket, show the address the server
//! gives and a fresh public key as a QR code, take the account's keys the
//! phone encrypts to that key, then register this device with pre-keys of
//! its own.

use std::time::SystemTime;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use libsignal_core::{Aci, DeviceId, Pni};
use libsignal_net::chat::server_requests::ProvisioningEvent;
use libsignal_net::chat::ws::ListenerEvent;
use libsignal_protocol::{
    GenericSignedPreKey, IdentityKey, IdentityKeyPair, KeyPair, KyberPreKeyRecord, PrivateKey, SignedPreKeyRecord,
    Timestamp, kem,
};
use libsignal_provisioning::RecvProvisionCiphertextState;
use prost::Message;
use rand::Rng;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;

use crate::account::{Account, Identity, IdentityKeys};
use crate::net::{self, Net};
use crate::proto::ProvisionMessage;

/// What a link makes: the account, and the first pre-keys the server now
/// holds for this device, to be kept.
pub struct Linked {
    pub account: Account,
    pub pre_keys: Vec<(Identity, SignedPreKeyRecord, KyberPreKeyRecord)>,
}

/// Link as `device_name`. `show` gets the `sgnl://linkdevice` URL to draw as a
/// QR code; the rest waits for the phone to scan it.
pub async fn link(net: &Net, device_name: &str, show: impl FnOnce(&str)) -> Result<Linked> {
    let cipher = RecvProvisionCiphertextState::generate(&mut rand::rng());
    let (events, mut event) = mpsc::unbounded_channel();
    let socket = net
        .connect(
            net::CHAT_PROVISIONING_PATH,
            None,
            // As libsignal's bridge listens: alerts are not for this socket,
            // and an event it cannot read is logged, not fatal.
            Box::new(move |e| match e {
                ListenerEvent::ReceivedAlerts(_) => {}
                e => match ProvisioningEvent::try_from(e) {
                    Ok(e) => drop(events.send(e)),
                    Err(e) => tracing::warn!("provisioning socket: {e}"),
                },
            }),
            "provisioning",
        )
        .await
        .context("opening the provisioning socket")?;

    let mut shown = Some(show);
    let message = loop {
        let event = event.recv().await.context("the provisioning socket closed")?;
        match event {
            ProvisioningEvent::ServerTimestamp(_) => {}
            ProvisioningEvent::ReceivedAddress { address, send_ack } => {
                let show = shown.take().context("the server gave a second address")?;
                send_ack(http::StatusCode::OK).map_err(|e| anyhow!("ack: {e}"))?;
                show(&link_url(&address, &cipher.public_key()));
            }
            ProvisioningEvent::ReceivedEnvelope { envelope, send_ack } => {
                let plain = cipher
                    .decrypt_envelope(&envelope)
                    .map_err(|e| anyhow!("the phone's message did not decrypt: {e}"))?;
                send_ack(http::StatusCode::OK).map_err(|e| anyhow!("ack: {e}"))?;
                break ProvisionMessage::decode(plain.as_slice()).context("the phone's message")?;
            }
            ProvisioningEvent::Stopped(cause) => bail!("the provisioning socket closed: {cause:?}"),
        }
    };
    socket.disconnect().await;
    register(net, device_name, message).await
}

fn link_url(address: &str, public_key: &[u8]) -> String {
    // `nopni2`: an account with no phone number may link. No `backup5` yet:
    // that asks the phone to offer its history, which nothing here reads.
    format!(
        "sgnl://linkdevice?uuid={}&pub_key={}&capabilities=nopni2",
        percent(address),
        percent(&BASE64.encode(public_key)),
    )
}

/// Percent-encode a query value (what `URLSearchParams` leaves bare stays bare).
fn percent(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

async fn register(net: &Net, device_name: &str, message: ProvisionMessage) -> Result<Linked> {
    let code = message.provisioning_code.context("no provisioning code")?;
    let profile_key = message.profile_key.filter(|k| !k.is_empty()).context("no profile key")?;
    let aci_pair = identity_pair(message.aci_identity_key_private.as_deref().context("no ACI key")?)?;
    let aci = match (&message.aci_binary, &message.aci) {
        (Some(bytes), _) if !bytes.is_empty() => uuid::Uuid::from_slice(bytes).ok().map(Aci::from),
        (_, Some(text)) => Aci::parse_from_service_id_string(text),
        _ => None,
    }
    .context("no ACI")?;
    let pni = match (&message.pni_binary, &message.pni) {
        (Some(bytes), _) if !bytes.is_empty() => Some(uuid::Uuid::from_slice(bytes).context("PNI")?),
        // The message carries the PNI bare, without the `PNI:` prefix.
        (_, Some(text)) if !text.is_empty() => Some(uuid::Uuid::parse_str(text).context("PNI")?),
        _ => None,
    }
    .map(Pni::from);
    let pni_pair = match (&pni, message.pni_identity_key_private.as_deref()) {
        (Some(_), Some(private)) if !private.is_empty() => Some(identity_pair(private)?),
        (None, None | Some([])) => None,
        _ => bail!("a PNI and its key come together"),
    };

    let mut rng = rand::rng();
    // Base64 of 16 bytes without its padding, as Desktop makes it.
    let password = BASE64.encode(rng.random::<[u8; 16]>()).trim_end_matches('=').to_string();
    let aci_keys = IdentityKeys { key_pair: aci_pair, registration_id: rng.random_range(1..16383) };
    let pni_keys = pni_pair.map(|key_pair| IdentityKeys { key_pair, registration_id: rng.random_range(1..16383) });

    let mut pre_keys = vec![first_pre_keys(Identity::Aci, &aci_keys.key_pair)?];
    if let Some(keys) = &pni_keys {
        pre_keys.push(first_pre_keys(Identity::Pni, &keys.key_pair)?);
    }

    let mut body = json!({
        "verificationCode": code,
        "accountAttributes": {
            "fetchesMessages": true,
            "name": BASE64.encode(crate::devicename::encrypt(device_name, aci_keys.key_pair.public_key())?),
            "registrationId": aci_keys.registration_id,
            "capabilities": capabilities(pni_keys.is_none()),
        },
    });
    if let Some(keys) = &pni_keys {
        body["accountAttributes"]["pniRegistrationId"] = keys.registration_id.into();
    }
    for (identity, signed, kyber) in &pre_keys {
        let e = |e| anyhow!("pre-key: {e}");
        body[format!("{}SignedPreKey", identity.as_str())] = signed_json(
            signed.id().map_err(e)?.into(),
            &signed.public_key().map_err(e)?.serialize(),
            &signed.signature().map_err(e)?,
        );
        body[format!("{}PqLastResortPreKey", identity.as_str())] = signed_json(
            kyber.id().map_err(e)?.into(),
            &kyber.public_key().map_err(e)?.serialize(),
            &kyber.signature().map_err(e)?,
        );
    }

    // The new device's sign-in, on a socket that is not signed in.
    let basic = BASE64.encode(format!("{}:{password}", aci.service_id_string()));
    let authorization = http::HeaderValue::from_str(&format!("Basic {basic}"))?;
    let chat = net
        .connect(net::CHAT_WEBSOCKET_PATH, None, Box::new(|_| {}), "link")
        .await
        .context("opening the chat socket")?;
    let response = net::send_json(
        &chat,
        http::Method::PUT,
        "/v1/devices/link",
        Some((http::header::AUTHORIZATION, authorization)),
        &body,
    )
    .await
    .map_err(|e| anyhow!("registering the device: {e}"))?;
    chat.disconnect().await;
    if !response.status.is_success() {
        let text = response.body.as_deref().map(String::from_utf8_lossy).unwrap_or_default();
        bail!("the server refused the device: {} {text}", response.status);
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct LinkedDevice {
        device_id: u8,
    }
    let device: LinkedDevice =
        serde_json::from_slice(response.body.as_deref().unwrap_or_default()).context("the server's answer")?;

    let account = Account {
        aci,
        device_id: DeviceId::new(device.device_id).map_err(|_| anyhow!("device id {}", device.device_id))?,
        password,
        aci_keys,
        pni: pni.zip(pni_keys),
        number: message.number,
        profile_key,
        account_entropy_pool: message.account_entropy_pool,
        media_root_backup_key: message.media_root_backup_key,
        ephemeral_backup_key: message.ephemeral_backup_key,
        read_receipts: message.read_receipts.unwrap_or(false),
    };
    Ok(Linked { account, pre_keys })
}

/// What this device says it can do, as Signal-Desktop's `getCapabilities`.
fn capabilities(no_phone_number: bool) -> serde_json::Value {
    json!({
        "attachmentBackfill": false,
        "spqr": true,
        "usernameChangeSyncMessage": true,
        "optionalPhoneNumber": no_phone_number,
        "storage": false,
    })
}

fn identity_pair(private: &[u8]) -> Result<IdentityKeyPair> {
    let private = PrivateKey::deserialize(private).map_err(|e| anyhow!("identity key: {e}"))?;
    let public = private.public_key().map_err(|e| anyhow!("identity key: {e}"))?;
    Ok(IdentityKeyPair::new(IdentityKey::new(public), private))
}

/// The signed pre-key and the last-resort Kyber pre-key a device registers with.
fn first_pre_keys(
    identity: Identity,
    keys: &IdentityKeyPair,
) -> Result<(Identity, SignedPreKeyRecord, KyberPreKeyRecord)> {
    let mut rng = rand::rng();
    let now = Timestamp::from_epoch_millis(
        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_millis() as u64,
    );
    let pair = KeyPair::generate(&mut rng);
    let signature = keys
        .private_key()
        .calculate_signature(&pair.public_key.serialize(), &mut rng)
        .map_err(|e| anyhow!("signing: {e}"))?;
    let signed = SignedPreKeyRecord::new(key_id(&mut rng).into(), now, &pair, &signature);
    let kyber = KyberPreKeyRecord::generate(kem::KeyType::Kyber1024, key_id(&mut rng).into(), keys.private_key())
        .map_err(|e| anyhow!("kyber pre-key: {e}"))?;
    Ok((identity, signed, kyber))
}

/// Key ids start anywhere in 24 bits, as Desktop's do.
fn key_id(rng: &mut impl Rng) -> u32 {
    rng.random_range(1..=0xff_ffff)
}

fn signed_json(id: u32, public_key: &[u8], signature: &[u8]) -> serde_json::Value {
    json!({
        "keyId": id,
        "publicKey": BASE64.encode(public_key),
        "signature": BASE64.encode(signature),
    })
}
