//! The store the Signal protocol reads and writes: sessions, the other
//! side's identity keys, our pre-keys, sender keys. One per identity of ours
//! (ACI, PNI), over the same file.
//!
//! Trust is on first use. A changed key is taken when it arrives on a
//! message (the conversation should say so; nothing does yet) and refused
//! when sending until then.

use anyhow::anyhow;
use async_trait::async_trait;
use libsignal_protocol::{
    CiphertextMessageType, Direction, GenericSignedPreKey, IdentityChange, IdentityKey, IdentityKeyPair,
    IdentityKeyStore, KyberPreKeyId, KyberPreKeyRecord, KyberPreKeyStore, PreKeyId, PreKeyRecord, PreKeyStore,
    ProtocolAddress, PublicKey, SenderKeyRecord, SenderKeyStore, SessionRecord, SessionStore, SignalProtocolError,
    SignedPreKeyId, SignedPreKeyRecord, SignedPreKeyStore,
};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use super::Store;
use crate::account::{Identity, IdentityKeys};

type Result<T> = std::result::Result<T, SignalProtocolError>;

/// Cheap to clone: libsignal takes the session store and the identity store
/// as two `&mut`, and both are this.
#[derive(Clone)]
pub struct ProtocolStore {
    store: Store,
    identity: Identity,
    keys: IdentityKeys,
}

impl Store {
    pub fn protocol(&self, identity: Identity, keys: IdentityKeys) -> ProtocolStore {
        ProtocolStore { store: self.clone(), identity, keys }
    }
}

impl ProtocolStore {
    fn blob(&self, what: &'static str, sql: &str, args: impl rusqlite::Params) -> Result<Option<Vec<u8>>> {
        self.store.db().query_row(sql, args, |row| row.get(0)).optional().map_err(failed(what))
    }

    fn write(&self, what: &'static str, sql: &str, args: impl rusqlite::Params) -> Result<usize> {
        self.store.db().execute(sql, args).map_err(failed(what))
    }

    pub fn save_pre_key_record(&self, record: &PreKeyRecord) -> anyhow::Result<()> {
        let e = |e| anyhow!("pre-key: {e}");
        self.write(
            "save_pre_key",
            "INSERT OR REPLACE INTO pre_keys (identity, id, record) VALUES (?1, ?2, ?3)",
            params![self.identity.as_str(), u32::from(record.id().map_err(e)?), record.serialize().map_err(e)?],
        )
        .map_err(e)?;
        Ok(())
    }

    pub fn save_kyber_record(&self, record: &KyberPreKeyRecord) -> anyhow::Result<()> {
        super::account::save_kyber(&self.store.db(), self.identity, record, false)
    }
}

/// A database failure, as the error libsignal passes through.
fn failed(what: &'static str) -> impl Fn(rusqlite::Error) -> SignalProtocolError {
    #[derive(Debug)]
    struct StoreError(String);
    impl std::fmt::Display for StoreError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }
    impl std::error::Error for StoreError {}
    move |e| SignalProtocolError::ApplicationCallbackError(what, Box::new(StoreError(e.to_string())))
}

#[async_trait(?Send)]
impl IdentityKeyStore for ProtocolStore {
    async fn get_identity_key_pair(&self) -> Result<IdentityKeyPair> {
        Ok(self.keys.key_pair)
    }

    async fn get_local_registration_id(&self) -> Result<u32> {
        Ok(self.keys.registration_id)
    }

    async fn save_identity(&mut self, address: &ProtocolAddress, identity: &IdentityKey) -> Result<IdentityChange> {
        let known = self.get_identity(address).await?;
        if known.as_ref() == Some(identity) {
            return Ok(IdentityChange::NewOrUnchanged);
        }
        self.write(
            "save_identity",
            "INSERT OR REPLACE INTO identities (address, key) VALUES (?1, ?2)",
            params![address.name(), identity.serialize().to_vec()],
        )?;
        if known.is_some() {
            tracing::warn!(address = address.name(), "identity key changed");
        }
        Ok(IdentityChange::from_changed(known.is_some()))
    }

    async fn is_trusted_identity(
        &self,
        address: &ProtocolAddress,
        identity: &IdentityKey,
        direction: Direction,
    ) -> Result<bool> {
        Ok(match (self.get_identity(address).await?, direction) {
            (None, _) | (Some(_), Direction::Receiving) => true,
            (Some(known), Direction::Sending) => &known == identity,
        })
    }

    async fn get_identity(&self, address: &ProtocolAddress) -> Result<Option<IdentityKey>> {
        self.blob("get_identity", "SELECT key FROM identities WHERE address = ?1", [address.name()])?
            .map(|key| IdentityKey::decode(&key))
            .transpose()
    }
}

#[async_trait(?Send)]
impl SessionStore for ProtocolStore {
    async fn load_session(&self, address: &ProtocolAddress) -> Result<Option<SessionRecord>> {
        self.blob(
            "load_session",
            "SELECT record FROM sessions WHERE identity = ?1 AND address = ?2 AND device = ?3",
            params![self.identity.as_str(), address.name(), u8::from(address.device_id())],
        )?
        .map(|record| SessionRecord::deserialize(&record))
        .transpose()
    }

    async fn store_session(&mut self, address: &ProtocolAddress, record: &SessionRecord) -> Result<()> {
        self.write(
            "store_session",
            "INSERT OR REPLACE INTO sessions (identity, address, device, record) VALUES (?1, ?2, ?3, ?4)",
            params![self.identity.as_str(), address.name(), u8::from(address.device_id()), record.serialize()?],
        )?;
        Ok(())
    }
}

#[async_trait(?Send)]
impl PreKeyStore for ProtocolStore {
    async fn get_pre_key(&self, id: PreKeyId) -> Result<PreKeyRecord> {
        let record = self
            .blob(
                "get_pre_key",
                "SELECT record FROM pre_keys WHERE identity = ?1 AND id = ?2",
                params![self.identity.as_str(), u32::from(id)],
            )?
            .ok_or(SignalProtocolError::InvalidPreKeyId)?;
        PreKeyRecord::deserialize(&record)
    }

    async fn save_pre_key(&mut self, id: PreKeyId, record: &PreKeyRecord) -> Result<()> {
        self.write(
            "save_pre_key",
            "INSERT OR REPLACE INTO pre_keys (identity, id, record) VALUES (?1, ?2, ?3)",
            params![self.identity.as_str(), u32::from(id), record.serialize()?],
        )?;
        Ok(())
    }

    async fn remove_pre_key(&mut self, id: PreKeyId) -> Result<()> {
        self.write(
            "remove_pre_key",
            "DELETE FROM pre_keys WHERE identity = ?1 AND id = ?2",
            params![self.identity.as_str(), u32::from(id)],
        )?;
        Ok(())
    }
}

#[async_trait(?Send)]
impl SignedPreKeyStore for ProtocolStore {
    async fn get_signed_pre_key(&self, id: SignedPreKeyId) -> Result<SignedPreKeyRecord> {
        let record = self
            .blob(
                "get_signed_pre_key",
                "SELECT record FROM signed_pre_keys WHERE identity = ?1 AND id = ?2",
                params![self.identity.as_str(), u32::from(id)],
            )?
            .ok_or(SignalProtocolError::InvalidSignedPreKeyId)?;
        SignedPreKeyRecord::deserialize(&record)
    }

    async fn save_signed_pre_key(&mut self, id: SignedPreKeyId, record: &SignedPreKeyRecord) -> Result<()> {
        self.write(
            "save_signed_pre_key",
            "INSERT OR REPLACE INTO signed_pre_keys (identity, id, record) VALUES (?1, ?2, ?3)",
            params![self.identity.as_str(), u32::from(id), record.serialize()?],
        )?;
        Ok(())
    }
}

#[async_trait(?Send)]
impl KyberPreKeyStore for ProtocolStore {
    async fn get_kyber_pre_key(&self, id: KyberPreKeyId) -> Result<KyberPreKeyRecord> {
        let record = self
            .blob(
                "get_kyber_pre_key",
                "SELECT record FROM kyber_pre_keys WHERE identity = ?1 AND id = ?2",
                params![self.identity.as_str(), u32::from(id)],
            )?
            .ok_or(SignalProtocolError::InvalidKyberPreKeyId)?;
        KyberPreKeyRecord::deserialize(&record)
    }

    async fn save_kyber_pre_key(&mut self, id: KyberPreKeyId, record: &KyberPreKeyRecord) -> Result<()> {
        self.write(
            "save_kyber_pre_key",
            "INSERT OR REPLACE INTO kyber_pre_keys (identity, id, record, last_resort) VALUES (?1, ?2, ?3, 0)",
            params![self.identity.as_str(), u32::from(id), record.serialize()?],
        )?;
        Ok(())
    }

    /// A one-time key is used once and goes. The last-resort key stays, so a
    /// message that repeats a base key against it is a replay.
    async fn mark_kyber_pre_key_used(
        &mut self,
        id: KyberPreKeyId,
        signed_id: SignedPreKeyId,
        base_key: &PublicKey,
    ) -> Result<()> {
        let identity = self.identity.as_str();
        let removed = self.write(
            "mark_kyber_pre_key_used",
            "DELETE FROM kyber_pre_keys WHERE identity = ?1 AND id = ?2 AND last_resort = 0",
            params![identity, u32::from(id)],
        )?;
        if removed > 0 {
            return Ok(());
        }
        let fresh = self.write(
            "mark_kyber_pre_key_used",
            "INSERT OR IGNORE INTO kyber_base_keys (identity, kyber_id, signed_id, base_key) VALUES (?1, ?2, ?3, ?4)",
            params![identity, u32::from(id), u32::from(signed_id), base_key.serialize().to_vec()],
        )?;
        if fresh == 0 {
            return Err(SignalProtocolError::InvalidMessage(CiphertextMessageType::PreKey, "reused base key".into()));
        }
        Ok(())
    }
}

#[async_trait(?Send)]
impl SenderKeyStore for ProtocolStore {
    async fn store_sender_key(
        &mut self,
        sender: &ProtocolAddress,
        distribution_id: Uuid,
        record: &SenderKeyRecord,
    ) -> Result<()> {
        self.write(
            "store_sender_key",
            "INSERT OR REPLACE INTO sender_keys (address, device, distribution_id, record) VALUES (?1, ?2, ?3, ?4)",
            params![sender.name(), u8::from(sender.device_id()), distribution_id.to_string(), record.serialize()?],
        )?;
        Ok(())
    }

    async fn load_sender_key(
        &mut self,
        sender: &ProtocolAddress,
        distribution_id: Uuid,
    ) -> Result<Option<SenderKeyRecord>> {
        self.blob(
            "load_sender_key",
            "SELECT record FROM sender_keys WHERE address = ?1 AND device = ?2 AND distribution_id = ?3",
            params![sender.name(), u8::from(sender.device_id()), distribution_id.to_string()],
        )?
        .map(|record| SenderKeyRecord::deserialize(&record))
        .transpose()
    }
}
