//! A device's name as the server keeps it: encrypted to the account's
//! identity key, so only the account's devices read it (`DeviceName.proto`).
//! As Signal-Desktop's `encryptDeviceName`.

use anyhow::{Context, Result};
use libsignal_protocol::{KeyPair, PublicKey};
use prost::Message;
use signal_crypto::{Aes256Ctr32, CryptographicMac};

use crate::proto::DeviceName;

pub fn encrypt(name: &str, identity: &PublicKey) -> Result<Vec<u8>> {
    let ephemeral = KeyPair::generate(&mut rand::rng());
    let secret = ephemeral.private_key.calculate_agreement(identity).context("agreement")?;
    let synthetic_iv = &hmac(&hmac(&secret, b"auth")?, name.as_bytes())?[..16];
    let key = hmac(&hmac(&secret, b"cipher")?, synthetic_iv)?;
    let mut ciphertext = name.as_bytes().to_vec();
    Aes256Ctr32::from_key(&key, &[0; 12], 0)
        .map_err(|e| anyhow::anyhow!("aes: {e}"))?
        .process(&mut ciphertext);
    let wrapped = DeviceName {
        ephemeral_public: Some(ephemeral.public_key.serialize().into()),
        synthetic_iv: Some(synthetic_iv.to_vec()),
        ciphertext: Some(ciphertext),
    };
    Ok(wrapped.encode_to_vec())
}

fn hmac(key: &[u8], input: &[u8]) -> Result<Vec<u8>> {
    let mut mac = CryptographicMac::new("HmacSha256", key).map_err(|e| anyhow::anyhow!("hmac: {e}"))?;
    mac.update(input);
    Ok(mac.finalize())
}
