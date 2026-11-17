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
    // 3: people, groups, conversations and what is said in them. A message
    // is known by who sent it and the sender's timestamp; `revisions` holds
    // the timestamps of its edits, which receipts and reactions also name.
    // `outbox` is what has still to go out, in order.
    "CREATE TABLE recipients (
        id INTEGER PRIMARY KEY,
        aci TEXT UNIQUE,
        pni TEXT UNIQUE,
        number TEXT,
        profile_key BLOB,
        contact_name TEXT,
        profile_name TEXT,
        profile_at INTEGER NOT NULL DEFAULT 0,
        blocked INTEGER NOT NULL DEFAULT 0
    ) STRICT;
    CREATE TABLE groups (
        id INTEGER PRIMARY KEY,
        master_key BLOB NOT NULL UNIQUE,
        identifier BLOB NOT NULL UNIQUE,
        revision INTEGER NOT NULL DEFAULT -1,
        title TEXT
    ) STRICT;
    CREATE TABLE group_members (
        grp INTEGER NOT NULL REFERENCES groups ON DELETE CASCADE,
        recipient INTEGER NOT NULL REFERENCES recipients,
        PRIMARY KEY (grp, recipient)
    ) STRICT, WITHOUT ROWID;
    CREATE TABLE conversations (
        id INTEGER PRIMARY KEY,
        recipient INTEGER UNIQUE REFERENCES recipients,
        grp INTEGER UNIQUE REFERENCES groups,
        expire_timer INTEGER NOT NULL DEFAULT 0,
        expire_timer_version INTEGER NOT NULL DEFAULT 1,
        last_activity INTEGER NOT NULL DEFAULT 0,
        archived INTEGER NOT NULL DEFAULT 0,
        muted_until INTEGER NOT NULL DEFAULT 0,
        draft TEXT NOT NULL DEFAULT '',
        CHECK ((recipient IS NULL) <> (grp IS NULL))
    ) STRICT;
    CREATE TABLE messages (
        id INTEGER PRIMARY KEY,
        conversation INTEGER NOT NULL REFERENCES conversations ON DELETE CASCADE,
        sender INTEGER NOT NULL REFERENCES recipients,
        sent_at INTEGER NOT NULL,
        received_at INTEGER NOT NULL,
        kind INTEGER NOT NULL DEFAULT 0,
        body TEXT NOT NULL DEFAULT '',
        quote_sent_at INTEGER,
        quote_author INTEGER REFERENCES recipients,
        quote_text TEXT,
        expires_in INTEGER NOT NULL DEFAULT 0,
        expires_at INTEGER,
        outgoing INTEGER NOT NULL,
        status INTEGER NOT NULL DEFAULT 0,
        unread INTEGER NOT NULL DEFAULT 0,
        edited INTEGER NOT NULL DEFAULT 0,
        deleted INTEGER NOT NULL DEFAULT 0,
        UNIQUE (sender, sent_at, conversation)
    ) STRICT;
    CREATE INDEX messages_thread ON messages (conversation, received_at, id);
    CREATE INDEX messages_unread ON messages (conversation) WHERE unread = 1;
    CREATE INDEX messages_expiring ON messages (expires_at) WHERE expires_at IS NOT NULL;
    CREATE TABLE revisions (
        message INTEGER NOT NULL REFERENCES messages ON DELETE CASCADE,
        sent_at INTEGER NOT NULL,
        PRIMARY KEY (message, sent_at)
    ) STRICT, WITHOUT ROWID;
    CREATE TABLE reactions (
        message INTEGER NOT NULL REFERENCES messages ON DELETE CASCADE,
        author INTEGER NOT NULL REFERENCES recipients,
        emoji TEXT NOT NULL,
        sent_at INTEGER NOT NULL,
        PRIMARY KEY (message, author)
    ) STRICT, WITHOUT ROWID;
    CREATE TABLE attachments (
        id INTEGER PRIMARY KEY,
        message INTEGER NOT NULL REFERENCES messages ON DELETE CASCADE,
        position INTEGER NOT NULL,
        content_type TEXT NOT NULL DEFAULT '',
        file_name TEXT,
        size INTEGER NOT NULL DEFAULT 0,
        width INTEGER NOT NULL DEFAULT 0,
        height INTEGER NOT NULL DEFAULT 0,
        flags INTEGER NOT NULL DEFAULT 0,
        pointer BLOB,
        path TEXT,
        state INTEGER NOT NULL DEFAULT 0
    ) STRICT;
    CREATE INDEX attachments_message ON attachments (message, position);
    CREATE TABLE outbox (
        id INTEGER PRIMARY KEY,
        conversation INTEGER REFERENCES conversations ON DELETE CASCADE,
        recipient INTEGER REFERENCES recipients,
        message INTEGER REFERENCES messages ON DELETE CASCADE,
        timestamp INTEGER NOT NULL,
        content BLOB,
        urgent INTEGER NOT NULL DEFAULT 1
    ) STRICT;
    CREATE TABLE settings (
        key TEXT PRIMARY KEY,
        value BLOB NOT NULL
    ) STRICT, WITHOUT ROWID;",
];

pub fn migrate(db: &Connection) -> Result<()> {
    let done: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    for (index, step) in STEPS.iter().enumerate().skip(done as usize) {
        db.execute_batch(&format!("BEGIN; {step}; PRAGMA user_version = {}; COMMIT;", index + 1))?;
    }
    Ok(())
}
