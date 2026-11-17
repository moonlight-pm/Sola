//! The account row, and what a link writes with it.

use anyhow::{Context, Result, anyhow};
use libsignal_core::{Aci, DeviceId, Pni};
use libsignal_protocol::{GenericSignedPreKey, IdentityKeyPair, KyberPreKeyRecord, SignedPreKeyRecord};
use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::account::{Account, Identity, IdentityKeys};
use crate::link::Linked;

impl Store {
    /// The account this device is linked to, if it is.
    pub fn account(&self) -> Result<Option<Account>> {
        let db = self.db();
        let row = db
            .query_row(
                "SELECT aci, pni, number, device_id, password, aci_identity, aci_registration_id,
                        pni_identity, pni_registration_id, profile_key, account_entropy_pool,
                        media_root_backup_key, ephemeral_backup_key, read_receipts
                 FROM account",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, u8>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Vec<u8>>(5)?,
                        row.get::<_, u32>(6)?,
                        row.get::<_, Option<Vec<u8>>>(7)?,
                        row.get::<_, Option<u32>>(8)?,
                        row.get::<_, Vec<u8>>(9)?,
                        row.get::<_, Option<String>>(10)?,
                        row.get::<_, Option<Vec<u8>>>(11)?,
                        row.get::<_, Option<Vec<u8>>>(12)?,
                        row.get::<_, bool>(13)?,
                    ))
                },
            )
            .optional()?;
        let Some((aci, pni, number, device_id, password, aci_identity, aci_registration_id, pni_identity, pni_registration_id, profile_key, account_entropy_pool, media_root_backup_key, ephemeral_backup_key, read_receipts)) = row
        else {
            return Ok(None);
        };
        let key_pair = |bytes: &[u8]| IdentityKeyPair::try_from(bytes).map_err(|e| anyhow!("stored identity key: {e}"));
        let pni = match (pni, pni_identity, pni_registration_id) {
            (Some(pni), Some(identity), Some(registration_id)) => Some((
                Pni::parse_from_service_id_string(&pni).context("stored PNI")?,
                IdentityKeys { key_pair: key_pair(&identity)?, registration_id },
            )),
            _ => None,
        };
        Ok(Some(Account {
            aci: Aci::parse_from_service_id_string(&aci).context("stored ACI")?,
            device_id: DeviceId::new(device_id).map_err(|_| anyhow!("stored device id {device_id}"))?,
            password,
            aci_keys: IdentityKeys { key_pair: key_pair(&aci_identity)?, registration_id: aci_registration_id },
            pni,
            number,
            profile_key,
            account_entropy_pool,
            media_root_backup_key,
            ephemeral_backup_key,
            read_receipts,
        }))
    }

    /// Whether the account sends read receipts, as the phone has it set.
    pub fn set_read_receipts(&self, on: bool) -> Result<()> {
        self.db().execute("UPDATE account SET read_receipts = ?1", [on])?;
        Ok(())
    }

    /// The account's root keys, sent again by the phone.
    pub fn set_account_keys(&self, entropy_pool: Option<&str>, media_root_backup_key: Option<&[u8]>) -> Result<()> {
        self.db().execute(
            "UPDATE account SET account_entropy_pool = coalesce(?1, account_entropy_pool),
                                media_root_backup_key = coalesce(?2, media_root_backup_key)",
            params![entropy_pool, media_root_backup_key],
        )?;
        Ok(())
    }

    /// Keep a new link: the account and the pre-keys the server took with it,
    /// together or not at all.
    pub fn save_link(&self, linked: &Linked) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let a = &linked.account;
        let pni = a.pni.as_ref();
        // Linked again: the device is a new one, so what the protocol kept
        // for the old one is void. Another account's conversations go too.
        let before: Option<String> = tx.query_row("SELECT aci FROM account", [], |row| row.get(0)).optional()?;
        if before.is_some() {
            tx.execute_batch(
                "DELETE FROM account; DELETE FROM sessions; DELETE FROM pre_keys; DELETE FROM signed_pre_keys;
                 DELETE FROM kyber_pre_keys; DELETE FROM kyber_base_keys; DELETE FROM sender_keys;
                 DELETE FROM outbox; DELETE FROM settings;",
            )?;
        }
        if before.is_some_and(|aci| aci != a.aci.service_id_string()) {
            tx.execute_batch(
                "DELETE FROM messages; DELETE FROM conversations; DELETE FROM group_members; DELETE FROM groups;
                 DELETE FROM recipients; DELETE FROM identities;",
            )?;
        }
        tx.execute(
            "INSERT INTO account (id, aci, pni, number, device_id, password, aci_identity,
                aci_registration_id, pni_identity, pni_registration_id, profile_key,
                account_entropy_pool, media_root_backup_key, ephemeral_backup_key, read_receipts)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                a.aci.service_id_string(),
                pni.map(|(pni, _)| pni.service_id_string()),
                a.number,
                u8::from(a.device_id),
                a.password,
                a.aci_keys.key_pair.serialize().to_vec(),
                a.aci_keys.registration_id,
                pni.map(|(_, keys)| keys.key_pair.serialize().to_vec()),
                pni.map(|(_, keys)| keys.registration_id),
                a.profile_key,
                a.account_entropy_pool,
                a.media_root_backup_key,
                a.ephemeral_backup_key,
                a.read_receipts,
            ],
        )?;
        for (identity, signed, kyber) in &linked.pre_keys {
            save_signed(&tx, *identity, signed)?;
            save_kyber(&tx, *identity, kyber, true)?;
        }
        tx.commit()?;
        Ok(())
    }
}

pub(super) fn save_signed(db: &rusqlite::Connection, identity: Identity, record: &SignedPreKeyRecord) -> Result<()> {
    let e = |e| anyhow!("signed pre-key: {e}");
    db.execute(
        "INSERT OR REPLACE INTO signed_pre_keys (identity, id, record) VALUES (?1, ?2, ?3)",
        params![identity.as_str(), u32::from(record.id().map_err(e)?), record.serialize().map_err(e)?],
    )?;
    Ok(())
}

pub(super) fn save_kyber(
    db: &rusqlite::Connection,
    identity: Identity,
    record: &KyberPreKeyRecord,
    last_resort: bool,
) -> Result<()> {
    let e = |e| anyhow!("kyber pre-key: {e}");
    db.execute(
        "INSERT OR REPLACE INTO kyber_pre_keys (identity, id, record, last_resort) VALUES (?1, ?2, ?3, ?4)",
        params![identity.as_str(), u32::from(record.id().map_err(e)?), record.serialize().map_err(e)?, last_resort],
    )?;
    Ok(())
}
