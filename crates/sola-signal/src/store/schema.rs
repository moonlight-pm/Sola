//! The store's tables. `user_version` counts the steps applied.

use anyhow::Result;
use rusqlite::Connection;

const STEPS: &[&str] = &[
    // 1: the account and its pre-keys.
    "CREATE TABLE account (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        aci TEXT NOT NULL,
        pni TEXT,
        number TEXT,
        device_id INTEGER NOT NULL,
        password TEXT NOT NULL,
        aci_identity BLOB NOT NULL,
        aci_registration_id INTEGER NOT NULL,
        pni_identity BLOB,
        pni_registration_id INTEGER,
        profile_key BLOB NOT NULL,
        account_entropy_pool TEXT,
        media_root_backup_key BLOB,
        ephemeral_backup_key BLOB,
        read_receipts INTEGER NOT NULL
    ) STRICT;
    CREATE TABLE signed_pre_keys (
        identity TEXT NOT NULL,
        id INTEGER NOT NULL,
        record BLOB NOT NULL,
        PRIMARY KEY (identity, id)
    ) STRICT;
    CREATE TABLE kyber_pre_keys (
        identity TEXT NOT NULL,
        id INTEGER NOT NULL,
        record BLOB NOT NULL,
        last_resort INTEGER NOT NULL,
        PRIMARY KEY (identity, id)
    ) STRICT;",
    // 2: what the Signal protocol keeps. `identity` is ours (aci or pni);
    // `address` is the other side's service id.
    "CREATE TABLE identities (
        address TEXT PRIMARY KEY,
        key BLOB NOT NULL
    ) STRICT;
    CREATE TABLE sessions (
        identity TEXT NOT NULL,
        address TEXT NOT NULL,
        device INTEGER NOT NULL,
        record BLOB NOT NULL,
        PRIMARY KEY (identity, address, device)
    ) STRICT;
    CREATE TABLE pre_keys (
        identity TEXT NOT NULL,
        id INTEGER NOT NULL,
        record BLOB NOT NULL,
        PRIMARY KEY (identity, id)
    ) STRICT;
    CREATE TABLE kyber_base_keys (
        identity TEXT NOT NULL,
        kyber_id INTEGER NOT NULL,
        signed_id INTEGER NOT NULL,
        base_key BLOB NOT NULL,
        PRIMARY KEY (identity, kyber_id, signed_id, base_key)
    ) STRICT;
    CREATE TABLE sender_keys (
        address TEXT NOT NULL,
        device INTEGER NOT NULL,
        distribution_id TEXT NOT NULL,
        record BLOB NOT NULL,
        PRIMARY KEY (address, device, distribution_id)
    ) STRICT;",
];

pub fn migrate(db: &Connection) -> Result<()> {
    let done: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    for (index, step) in STEPS.iter().enumerate().skip(done as usize) {
        db.execute_batch(&format!("BEGIN; {step}; PRAGMA user_version = {}; COMMIT;", index + 1))?;
    }
    Ok(())
}
