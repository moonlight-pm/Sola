//! People, groups, conversations and messages: what the engine writes as
//! Signal delivers it, and what the window reads to draw.

use std::collections::HashMap;

use anyhow::{Context, Result};
use libsignal_core::Aci;
use rusqlite::{Connection, OptionalExtension, params};

use super::Store;

pub type Id = i64;

/// How far a message we sent has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Sending = 0,
    Sent = 1,
    Delivered = 2,
    Read = 3,
    Failed = 4,
}

impl Status {
    fn from(value: i64) -> Self {
        match value {
            1 => Status::Sent,
            2 => Status::Delivered,
            3 => Status::Read,
            4 => Status::Failed,
            _ => Status::Sending,
        }
    }
}

/// A message someone wrote, or a line the client wrote about the
/// conversation (a timer change, a call, a group change).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text = 0,
    Note = 1,
}

#[derive(Clone, Debug)]
pub struct Recipient {
    pub id: Id,
    pub aci: Option<Aci>,
    pub number: Option<String>,
    pub profile_key: Option<Vec<u8>>,
    pub contact_name: Option<String>,
    pub profile_name: Option<String>,
    pub profile_at: u64,
    pub blocked: bool,
}

impl Recipient {
    /// The name to show: the phone's contact, else the profile, else the number.
    pub fn name(&self) -> String {
        [&self.contact_name, &self.profile_name, &self.number]
            .into_iter()
            .flatten()
            .find(|name| !name.is_empty())
            .cloned()
            .unwrap_or_else(|| "Unknown".into())
    }
}

/// The same choice in SQL, for a joined `recipients` row named `alias`.
fn name_sql(alias: &str) -> String {
    format!("coalesce(nullif({alias}.contact_name, ''), nullif({alias}.profile_name, ''), {alias}.number, 'Unknown')")
}

#[derive(Clone, Debug)]
pub struct Group {
    pub master_key: Vec<u8>,
    /// -1 until its state has been fetched.
    pub revision: i64,
    pub members: Vec<Id>,
}

#[derive(Clone, Debug)]
pub struct Conversation {
    pub recipient: Option<Id>,
    pub group: Option<Id>,
    /// Seconds a message lives once read; 0 is forever.
    pub expire_timer: u32,
    pub expire_timer_version: u32,
}

/// A message to keep.
#[derive(Clone, Debug, Default)]
pub struct NewMessage {
    pub conversation: Id,
    pub sender: Id,
    pub sent_at: u64,
    pub received_at: u64,
    pub note: bool,
    pub body: String,
    pub quote: Option<Quote>,
    pub expires_in: u32,
    /// When its timer started, if it has.
    pub expire_started: Option<u64>,
    pub outgoing: bool,
    pub status: Option<Status>,
    pub unread: bool,
    pub attachments: Vec<NewAttachment>,
}

#[derive(Clone, Debug)]
pub struct Quote {
    pub sent_at: u64,
    pub author: Id,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct NewAttachment {
    pub content_type: String,
    pub file_name: Option<String>,
    pub size: u64,
    pub width: u32,
    pub height: u32,
    pub flags: u32,
    /// The `AttachmentPointer` it came as; None for a file of ours.
    pub pointer: Option<Vec<u8>>,
    /// The file, once it is here.
    pub path: Option<String>,
}

/// Where an attachment's file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetch {
    Waiting = 0,
    Fetching = 1,
    Here = 2,
    Failed = 3,
}

#[derive(Clone, Debug)]
pub struct Attachment {
    pub id: Id,
    pub message: Id,
    pub content_type: String,
    pub file_name: Option<String>,
    pub size: u64,
    pub width: u32,
    pub height: u32,
    pub pointer: Option<Vec<u8>>,
    pub path: Option<String>,
    pub state: Fetch,
}

/// One row of the conversation list.
#[derive(Clone, Debug)]
pub struct Chat {
    pub id: Id,
    pub recipient: Option<Id>,
    pub group: bool,
    pub title: String,
    pub last_activity: u64,
    pub unread: u32,
    pub muted: bool,
    pub expire_timer: u32,
    pub draft: String,
    /// The last message, as one line.
    pub preview: String,
    pub preview_outgoing: bool,
}

/// One message as the thread shows it.
#[derive(Clone, Debug)]
pub struct Bubble {
    pub id: Id,
    pub sender: Id,
    pub sender_name: String,
    pub sent_at: u64,
    pub received_at: u64,
    pub kind: Kind,
    pub body: String,
    pub quote: Option<(String, String)>,
    pub expires_in: u32,
    pub outgoing: bool,
    pub status: Status,
    pub edited: bool,
    pub deleted: bool,
    /// Each emoji with how many reacted with it, and whether we did.
    pub reactions: Vec<(String, u32, bool)>,
    pub attachments: Vec<Attachment>,
}

/// Something still to send.
#[derive(Clone, Debug)]
pub struct Outgoing {
    pub id: Id,
    pub conversation: Option<Id>,
    pub recipient: Option<Id>,
    pub message: Option<Id>,
    pub timestamp: u64,
    /// A `Content`, unpadded. None: built from `message` when it is sent.
    pub content: Option<Vec<u8>>,
    pub urgent: bool,
}

fn recipient_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Recipient> {
    Ok(Recipient {
        id: row.get(0)?,
        aci: row.get::<_, Option<String>>(1)?.and_then(|aci| Aci::parse_from_service_id_string(&aci)),
        number: row.get(2)?,
        profile_key: row.get(3)?,
        contact_name: row.get(4)?,
        profile_name: row.get(5)?,
        profile_at: row.get::<_, i64>(6)? as u64,
        blocked: row.get(7)?,
    })
}

const RECIPIENT: &str =
    "SELECT id, aci, number, profile_key, contact_name, profile_name, profile_at, blocked FROM recipients";

fn attachment_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: row.get(0)?,
        message: row.get(1)?,
        content_type: row.get(2)?,
        file_name: row.get(3)?,
        size: row.get::<_, i64>(4)? as u64,
        width: row.get(5)?,
        height: row.get(6)?,
        pointer: row.get(7)?,
        path: row.get(8)?,
        state: match row.get::<_, i64>(9)? {
            1 => Fetch::Fetching,
            2 => Fetch::Here,
            3 => Fetch::Failed,
            _ => Fetch::Waiting,
        },
    })
}

const ATTACHMENT: &str =
    "SELECT id, message, content_type, file_name, size, width, height, pointer, path, state FROM attachments";

fn touch(db: &Connection, conversation: Id, at: u64) -> rusqlite::Result<()> {
    db.execute(
        "UPDATE conversations SET last_activity = max(last_activity, ?2), archived = 0 WHERE id = ?1",
        params![conversation, at as i64],
    )?;
    Ok(())
}

impl Store {
    // People.

    /// The row for an account, made if it is new.
    pub fn recipient_for(&self, aci: Aci) -> Result<Id> {
        let aci = aci.service_id_string();
        let db = self.db();
        db.execute("INSERT OR IGNORE INTO recipients (aci) VALUES (?1)", [&aci])?;
        Ok(db.query_row("SELECT id FROM recipients WHERE aci = ?1", [&aci], |row| row.get(0))?)
    }

    pub fn recipient(&self, id: Id) -> Result<Recipient> {
        self.db().query_row(&format!("{RECIPIENT} WHERE id = ?1"), [id], recipient_row).context("recipient")
    }

    pub fn recipients(&self) -> Result<Vec<Recipient>> {
        let db = self.db();
        let mut query = db.prepare(&format!("{RECIPIENT} ORDER BY id"))?;
        let rows = query.query_map([], recipient_row)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Keep someone's profile key. True if it is new or changed: their
    /// profile is then worth fetching again.
    pub fn set_profile_key(&self, id: Id, key: &[u8]) -> Result<bool> {
        let changed = self.db().execute(
            "UPDATE recipients SET profile_key = ?2, profile_at = 0 WHERE id = ?1 AND profile_key IS NOT ?2",
            params![id, key],
        )?;
        Ok(changed > 0)
    }

    pub fn set_profile_name(&self, id: Id, name: Option<&str>, at: u64) -> Result<()> {
        self.db().execute(
            "UPDATE recipients SET profile_name = coalesce(?2, profile_name), profile_at = ?3 WHERE id = ?1",
            params![id, name, at as i64],
        )?;
        Ok(())
    }

    /// What the phone's address book says of someone.
    pub fn set_contact(&self, id: Id, number: Option<&str>, name: Option<&str>) -> Result<()> {
        self.db().execute(
            "UPDATE recipients SET number = coalesce(?2, number), contact_name = ?3 WHERE id = ?1",
            params![id, number, name],
        )?;
        Ok(())
    }

    /// A profile name from somewhere other than the profile itself, kept
    /// only until the profile has been read.
    pub fn set_profile_name_if_none(&self, id: Id, name: &str) -> Result<()> {
        self.db().execute(
            "UPDATE recipients SET profile_name = ?2 WHERE id = ?1 AND coalesce(profile_name, '') = ''",
            params![id, name],
        )?;
        Ok(())
    }

    pub fn set_blocked_one(&self, id: Id, blocked: bool) -> Result<()> {
        self.db().execute("UPDATE recipients SET blocked = ?2 WHERE id = ?1", params![id, blocked])?;
        Ok(())
    }

    pub fn set_blocked(&self, blocked: &[Id]) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        tx.execute("UPDATE recipients SET blocked = 0", [])?;
        for id in blocked {
            tx.execute("UPDATE recipients SET blocked = 1 WHERE id = ?1", [id])?;
        }
        tx.commit()?;
        Ok(())
    }

    // Groups.

    /// The row for a group, made if it is new.
    pub fn group_for(&self, master_key: &[u8], identifier: &[u8]) -> Result<Id> {
        let db = self.db();
        db.execute("INSERT OR IGNORE INTO groups (master_key, identifier) VALUES (?1, ?2)", params![master_key, identifier])?;
        Ok(db.query_row("SELECT id FROM groups WHERE master_key = ?1", [master_key], |row| row.get(0))?)
    }

    pub fn group_by_identifier(&self, identifier: &[u8]) -> Result<Option<Id>> {
        Ok(self.db().query_row("SELECT id FROM groups WHERE identifier = ?1", [identifier], |row| row.get(0)).optional()?)
    }

    pub fn group(&self, id: Id) -> Result<Group> {
        let db = self.db();
        let (master_key, revision) = db
            .query_row("SELECT master_key, revision FROM groups WHERE id = ?1", [id], |row| Ok((row.get(0)?, row.get(1)?)))
            .context("group")?;
        let mut query = db.prepare("SELECT recipient FROM group_members WHERE grp = ?1 ORDER BY recipient")?;
        let members = query.query_map([id], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(Group { master_key, revision, members })
    }

    /// A group's state as the server has it.
    pub fn set_group(&self, id: Id, revision: u32, title: &str, members: &[Id]) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        tx.execute("UPDATE groups SET revision = ?2, title = ?3 WHERE id = ?1", params![id, revision, title])?;
        tx.execute("DELETE FROM group_members WHERE grp = ?1", [id])?;
        for member in members {
            tx.execute("INSERT OR IGNORE INTO group_members (grp, recipient) VALUES (?1, ?2)", params![id, member])?;
        }
        tx.commit()?;
        Ok(())
    }

    // Conversations.

    pub fn conversation_with(&self, recipient: Id) -> Result<Id> {
        let db = self.db();
        db.execute("INSERT OR IGNORE INTO conversations (recipient) VALUES (?1)", [recipient])?;
        Ok(db.query_row("SELECT id FROM conversations WHERE recipient = ?1", [recipient], |row| row.get(0))?)
    }

    pub fn conversation_in(&self, group: Id) -> Result<Id> {
        let db = self.db();
        db.execute("INSERT OR IGNORE INTO conversations (grp) VALUES (?1)", [group])?;
        Ok(db.query_row("SELECT id FROM conversations WHERE grp = ?1", [group], |row| row.get(0))?)
    }

    pub fn conversation(&self, id: Id) -> Result<Conversation> {
        self.db()
            .query_row(
                "SELECT recipient, grp, expire_timer, expire_timer_version FROM conversations WHERE id = ?1",
                [id],
                |row| {
                    Ok(Conversation {
                        recipient: row.get(0)?,
                        group: row.get(1)?,
                        expire_timer: row.get(2)?,
                        expire_timer_version: row.get(3)?,
                    })
                },
            )
            .context("conversation")
    }

    /// True if the timer changed.
    pub fn set_timer(&self, conversation: Id, seconds: u32, version: u32) -> Result<bool> {
        let changed = self.db().execute(
            "UPDATE conversations SET expire_timer = ?2, expire_timer_version = max(expire_timer_version, ?3)
             WHERE id = ?1 AND expire_timer <> ?2",
            params![conversation, seconds, version],
        )?;
        Ok(changed > 0)
    }

    /// Muted until a time (the far future for always), and whether the
    /// conversation is put away.
    pub fn set_muted(&self, conversation: Id, until: u64, archived: bool) -> Result<()> {
        self.db().execute(
            "UPDATE conversations SET muted_until = ?2, archived = ?3 WHERE id = ?1",
            params![conversation, until.min(i64::MAX as u64) as i64, archived],
        )?;
        Ok(())
    }

    pub fn set_draft(&self, conversation: Id, draft: &str) -> Result<()> {
        self.db().execute("UPDATE conversations SET draft = ?2 WHERE id = ?1", params![conversation, draft])?;
        Ok(())
    }

    /// Show a conversation in the list though nothing has been said in it.
    pub fn open_conversation(&self, conversation: Id, at: u64) -> Result<()> {
        Ok(touch(&self.db(), conversation, at)?)
    }

    pub fn delete_conversation(&self, conversation: Id) -> Result<()> {
        let db = self.db();
        db.execute("DELETE FROM messages WHERE conversation = ?1", [conversation])?;
        db.execute("UPDATE conversations SET last_activity = 0, draft = '' WHERE id = ?1", [conversation])?;
        Ok(())
    }

    // Messages.

    /// Keep a message. None if it is already kept (the server delivers
    /// again what was not acknowledged).
    pub fn add_message(&self, new: &NewMessage) -> Result<Option<Id>> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let expires_at = new.expire_started.filter(|_| new.expires_in > 0).map(|at| (at + new.expires_in as u64 * 1000) as i64);
        let quote = new.quote.as_ref();
        let added = tx.execute(
            "INSERT OR IGNORE INTO messages (conversation, sender, sent_at, received_at, kind, body, quote_sent_at,
                quote_author, quote_text, expires_in, expires_at, outgoing, status, unread)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                new.conversation,
                new.sender,
                new.sent_at as i64,
                new.received_at as i64,
                if new.note { Kind::Note } else { Kind::Text } as i64,
                new.body,
                quote.map(|q| q.sent_at as i64),
                quote.map(|q| q.author),
                quote.map(|q| &q.text),
                new.expires_in,
                expires_at,
                new.outgoing,
                new.status.unwrap_or(Status::Sent) as i64,
                new.unread,
            ],
        )?;
        if added == 0 {
            return Ok(None);
        }
        let id = tx.last_insert_rowid();
        for (position, a) in new.attachments.iter().enumerate() {
            tx.execute(
                "INSERT INTO attachments (message, position, content_type, file_name, size, width, height, flags,
                    pointer, path, state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    id,
                    position as i64,
                    a.content_type,
                    a.file_name,
                    a.size as i64,
                    a.width,
                    a.height,
                    a.flags,
                    a.pointer,
                    a.path,
                    if a.path.is_some() { Fetch::Here } else { Fetch::Waiting } as i64,
                ],
            )?;
        }
        touch(&tx, new.conversation, new.received_at)?;
        tx.commit()?;
        Ok(Some(id))
    }

    /// The message `sender` sent at `sent_at` (or an edit of it), and its
    /// conversation.
    pub fn find_message(&self, sender: Id, sent_at: u64) -> Result<Option<(Id, Id)>> {
        Ok(self
            .db()
            .query_row(
                "SELECT id, conversation FROM messages
                 WHERE sender = ?1 AND (sent_at = ?2 OR id IN (SELECT message FROM revisions WHERE sent_at = ?2))",
                params![sender, sent_at as i64],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }

    /// The conversation a message is in, who sent it, and its latest timestamp.
    pub fn message_address(&self, message: Id) -> Result<(Id, Id, u64)> {
        self.db()
            .query_row(
                "SELECT conversation, sender,
                        coalesce((SELECT max(sent_at) FROM revisions WHERE message = messages.id), sent_at)
                 FROM messages WHERE id = ?1",
                [message],
                |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? as u64)),
            )
            .context("message")
    }

    /// The text and timer of a message of ours that is waiting to go out.
    pub fn message_to_send(&self, message: Id) -> Result<(String, Option<Quote>, u32)> {
        self.db()
            .query_row(
                "SELECT body, quote_sent_at, quote_author, quote_text, expires_in FROM messages WHERE id = ?1",
                [message],
                |row| {
                    let quote = match (row.get::<_, Option<i64>>(1)?, row.get::<_, Option<Id>>(2)?) {
                        (Some(sent_at), Some(author)) => Some(Quote {
                            sent_at: sent_at as u64,
                            author,
                            text: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                        }),
                        _ => None,
                    };
                    Ok((row.get(0)?, quote, row.get(4)?))
                },
            )
            .context("message")
    }

    /// Someone's reaction to a message; `None` takes it away.
    pub fn set_reaction(&self, message: Id, author: Id, emoji: Option<&str>, sent_at: u64) -> Result<()> {
        let db = self.db();
        match emoji {
            Some(emoji) => db.execute(
                "INSERT OR REPLACE INTO reactions (message, author, emoji, sent_at) VALUES (?1, ?2, ?3, ?4)",
                params![message, author, emoji, sent_at as i64],
            )?,
            None => db.execute("DELETE FROM reactions WHERE message = ?1 AND author = ?2", params![message, author])?,
        };
        Ok(())
    }

    pub fn reaction_of(&self, message: Id, author: Id) -> Result<Option<String>> {
        Ok(self
            .db()
            .query_row("SELECT emoji FROM reactions WHERE message = ?1 AND author = ?2", params![message, author], |row| {
                row.get(0)
            })
            .optional()?)
    }

    /// What a reply shows of the message it answers: when it was first
    /// sent, who by, and its text.
    pub fn quoted(&self, message: Id) -> Result<Quote> {
        self.db()
            .query_row("SELECT sent_at, sender, body FROM messages WHERE id = ?1", [message], |row| {
                Ok(Quote { sent_at: row.get::<_, i64>(0)? as u64, author: row.get(1)?, text: row.get(2)? })
            })
            .context("message")
    }

    /// A message that failed is to be tried again.
    pub fn retry(&self, message: Id) -> Result<()> {
        self.db().execute("UPDATE messages SET status = 0 WHERE id = ?1 AND outgoing = 1 AND status = 4", [message])?;
        Ok(())
    }

    /// A message taken back by its sender: its place stays, its content goes.
    /// The files of its attachments are returned to be removed.
    pub fn retract(&self, message: Id) -> Result<Vec<String>> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let files = attachment_files(&tx, "message = ?1", message)?;
        tx.execute("UPDATE messages SET deleted = 1, body = '', quote_sent_at = NULL, quote_author = NULL, quote_text = NULL WHERE id = ?1", [message])?;
        tx.execute("DELETE FROM attachments WHERE message = ?1", [message])?;
        tx.execute("DELETE FROM reactions WHERE message = ?1", [message])?;
        tx.commit()?;
        Ok(files)
    }

    /// Remove a message from this device.
    pub fn delete_message(&self, message: Id) -> Result<Vec<String>> {
        let db = self.db();
        let files = attachment_files(&db, "message = ?1", message)?;
        db.execute("DELETE FROM messages WHERE id = ?1", [message])?;
        Ok(files)
    }

    /// A message's text replaced by an edit sent at `sent_at`.
    pub fn edit_message(&self, message: Id, sent_at: u64, body: &str) -> Result<()> {
        let db = self.db();
        db.execute("UPDATE messages SET body = ?2, edited = 1 WHERE id = ?1", params![message, body])?;
        db.execute("INSERT OR IGNORE INTO revisions (message, sent_at) VALUES (?1, ?2)", params![message, sent_at as i64])?;
        Ok(())
    }

    /// A message of ours got further (it never goes back, and a failure
    /// does not undo a delivery).
    pub fn raise_status(&self, message: Id, status: Status) -> Result<bool> {
        let changed = self.db().execute(
            "UPDATE messages SET status = ?2 WHERE id = ?1 AND outgoing = 1 AND status < ?2 AND (?2 < 4 OR status = 0)",
            params![message, status as i64],
        )?;
        Ok(changed > 0)
    }

    /// Start the timers of messages of ours that have none running yet.
    pub fn start_timer(&self, message: Id, at: u64) -> Result<()> {
        self.db().execute(
            "UPDATE messages SET expires_at = ?2 + expires_in * 1000
             WHERE id = ?1 AND expires_in > 0 AND expires_at IS NULL",
            params![message, at as i64],
        )?;
        Ok(())
    }

    /// Mark what is unread in a conversation as read, starting the timers of
    /// what disappears. Answers with each message's sender and timestamp, for
    /// the receipts.
    pub fn read_conversation(&self, conversation: Id, at: u64) -> Result<Vec<(Id, u64)>> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let read = {
            let mut query = tx.prepare("SELECT sender, sent_at FROM messages WHERE conversation = ?1 AND unread = 1")?;
            query
                .query_map([conversation], |row| Ok((row.get(0)?, row.get::<_, i64>(1)? as u64)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        tx.execute(
            "UPDATE messages SET unread = 0, expires_at = CASE WHEN expires_in > 0 AND expires_at IS NULL
                 THEN ?2 + expires_in * 1000 ELSE expires_at END
             WHERE conversation = ?1 AND unread = 1",
            params![conversation, at as i64],
        )?;
        tx.commit()?;
        Ok(read)
    }

    /// Another of our devices read a message.
    pub fn read_message(&self, message: Id, at: u64) -> Result<()> {
        self.db().execute(
            "UPDATE messages SET unread = 0, expires_at = CASE WHEN expires_in > 0 AND expires_at IS NULL
                 THEN ?2 + expires_in * 1000 ELSE expires_at END
             WHERE id = ?1",
            params![message, at as i64],
        )?;
        Ok(())
    }

    /// Remove what has disappeared by `now`. Answers with the conversations
    /// that lost messages and the files to remove.
    pub fn expire(&self, now: u64) -> Result<(Vec<Id>, Vec<String>)> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let conversations = {
            let mut query = tx.prepare("SELECT DISTINCT conversation FROM messages WHERE expires_at <= ?1")?;
            query.query_map([now as i64], |row| row.get(0))?.collect::<rusqlite::Result<Vec<Id>>>()?
        };
        let files = attachment_files(&tx, "message IN (SELECT id FROM messages WHERE expires_at <= ?1)", now as i64)?;
        tx.execute("DELETE FROM messages WHERE expires_at <= ?1", [now as i64])?;
        tx.commit()?;
        Ok((conversations, files))
    }

    /// When the next message disappears.
    pub fn next_expiry(&self) -> Result<Option<u64>> {
        let at: Option<i64> = self.db().query_row("SELECT min(expires_at) FROM messages", [], |row| row.get(0))?;
        Ok(at.map(|at| at as u64))
    }

    // Attachments.

    pub fn attachment(&self, id: Id) -> Result<Attachment> {
        self.db().query_row(&format!("{ATTACHMENT} WHERE id = ?1"), [id], attachment_row).context("attachment")
    }

    pub fn attachments_of(&self, message: Id) -> Result<Vec<Attachment>> {
        let db = self.db();
        let mut query = db.prepare(&format!("{ATTACHMENT} WHERE message = ?1 ORDER BY position"))?;
        let rows = query.query_map([message], attachment_row)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn set_attachment(&self, id: Id, state: Fetch, path: Option<&str>) -> Result<()> {
        self.db().execute(
            "UPDATE attachments SET state = ?2, path = coalesce(?3, path) WHERE id = ?1",
            params![id, state as i64, path],
        )?;
        Ok(())
    }

    /// An attachment of ours once uploaded: the pointer the recipients get.
    pub fn set_attachment_pointer(&self, id: Id, pointer: &[u8]) -> Result<()> {
        self.db().execute("UPDATE attachments SET pointer = ?2 WHERE id = ?1", params![id, pointer])?;
        Ok(())
    }

    /// Attachments that were being fetched when the process ended.
    pub fn reset_fetching(&self) -> Result<()> {
        self.db().execute("UPDATE attachments SET state = 0 WHERE state = 1", [])?;
        Ok(())
    }

    // The outbox.

    pub fn enqueue(&self, out: &Outgoing) -> Result<Id> {
        let db = self.db();
        db.execute(
            "INSERT INTO outbox (conversation, recipient, message, timestamp, content, urgent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![out.conversation, out.recipient, out.message, out.timestamp as i64, out.content, out.urgent],
        )?;
        Ok(db.last_insert_rowid())
    }

    /// The oldest thing still to send.
    pub fn next_outgoing(&self, after: Id) -> Result<Option<Outgoing>> {
        Ok(self
            .db()
            .query_row(
                "SELECT id, conversation, recipient, message, timestamp, content, urgent FROM outbox
                 WHERE id > ?1 ORDER BY id LIMIT 1",
                [after],
                |row| {
                    Ok(Outgoing {
                        id: row.get(0)?,
                        conversation: row.get(1)?,
                        recipient: row.get(2)?,
                        message: row.get(3)?,
                        timestamp: row.get::<_, i64>(4)? as u64,
                        content: row.get(5)?,
                        urgent: row.get(6)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn sent(&self, outgoing: Id) -> Result<()> {
        self.db().execute("DELETE FROM outbox WHERE id = ?1", [outgoing])?;
        Ok(())
    }

    // Settings.

    pub fn setting(&self, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.db().query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0)).optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &[u8]) -> Result<()> {
        self.db().execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)", params![key, value])?;
        Ok(())
    }

    // What the window reads.

    pub fn chats(&self) -> Result<Vec<Chat>> {
        let db = self.db();
        let name = name_sql("r");
        let mut query = db.prepare(&format!(
            "SELECT c.id, c.recipient, c.grp IS NOT NULL,
                    coalesce(nullif(g.title, ''), CASE WHEN c.grp IS NOT NULL THEN 'Group' END, {name}), c.last_activity,
                    (SELECT count(*) FROM messages m WHERE m.conversation = c.id AND m.unread = 1),
                    c.muted_until, c.expire_timer, c.draft,
                    last.body, last.outgoing, last.deleted,
                    (SELECT count(*) FROM attachments a WHERE a.message = last.id)
             FROM conversations c
             LEFT JOIN groups g ON g.id = c.grp
             LEFT JOIN recipients r ON r.id = c.recipient
             LEFT JOIN messages last ON last.id =
                 (SELECT id FROM messages WHERE conversation = c.id ORDER BY received_at DESC, id DESC LIMIT 1)
             WHERE c.last_activity > 0 AND c.archived = 0
             ORDER BY c.last_activity DESC"
        ))?;
        let now = super::now();
        let rows = query
            .query_map([], |row| {
                let body: Option<String> = row.get(9)?;
                let deleted: Option<bool> = row.get(11)?;
                let attachments: i64 = row.get(12)?;
                let preview = match (body, deleted) {
                    (_, Some(true)) => "This message was deleted".to_owned(),
                    (Some(body), _) if !body.is_empty() => body.lines().next().unwrap_or_default().to_owned(),
                    _ if attachments > 0 => "Attachment".to_owned(),
                    _ => String::new(),
                };
                Ok(Chat {
                    id: row.get(0)?,
                    recipient: row.get(1)?,
                    group: row.get(2)?,
                    title: row.get(3)?,
                    last_activity: row.get::<_, i64>(4)? as u64,
                    unread: row.get(5)?,
                    muted: row.get::<_, i64>(6)? as u64 > now,
                    expire_timer: row.get(7)?,
                    draft: row.get(8)?,
                    preview,
                    preview_outgoing: row.get::<_, Option<bool>>(10)?.unwrap_or(false),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn unread_total(&self) -> Result<u32> {
        Ok(self.db().query_row(
            "SELECT count(*) FROM messages m JOIN conversations c ON c.id = m.conversation
             WHERE m.unread = 1 AND c.muted_until <= ?1",
            [super::now() as i64],
            |row| row.get(0),
        )?)
    }

    /// The last `limit` messages of a conversation, oldest first. `me` is
    /// our own row, to say which reactions are ours.
    pub fn thread(&self, conversation: Id, limit: u32, me: Id) -> Result<Vec<Bubble>> {
        let db = self.db();
        let name = name_sql("r");
        let quoted = name_sql("q");
        let mut query = db.prepare(&format!(
            "SELECT m.id, m.sender, {name}, m.sent_at, m.received_at, m.kind, m.body, {quoted}, m.quote_text,
                    m.quote_author IS NOT NULL, m.expires_in, m.outgoing, m.status, m.edited, m.deleted
             FROM messages m
             JOIN recipients r ON r.id = m.sender
             LEFT JOIN recipients q ON q.id = m.quote_author
             WHERE m.conversation = ?1
             ORDER BY m.received_at DESC, m.id DESC LIMIT ?2"
        ))?;
        let mut bubbles: Vec<Bubble> = query
            .query_map(params![conversation, limit], |row| {
                Ok(Bubble {
                    id: row.get(0)?,
                    sender: row.get(1)?,
                    sender_name: row.get(2)?,
                    sent_at: row.get::<_, i64>(3)? as u64,
                    received_at: row.get::<_, i64>(4)? as u64,
                    kind: if row.get::<_, i64>(5)? == Kind::Note as i64 { Kind::Note } else { Kind::Text },
                    body: row.get(6)?,
                    quote: row
                        .get::<_, bool>(9)?
                        .then(|| Ok::<_, rusqlite::Error>((row.get(7)?, row.get::<_, Option<String>>(8)?.unwrap_or_default())))
                        .transpose()?,
                    expires_in: row.get(10)?,
                    outgoing: row.get(11)?,
                    status: Status::from(row.get(12)?),
                    edited: row.get(13)?,
                    deleted: row.get(14)?,
                    reactions: Vec::new(),
                    attachments: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        bubbles.reverse();
        let Some(first) = bubbles.first().map(|b| b.id) else { return Ok(bubbles) };
        let index: HashMap<Id, usize> = bubbles.iter().enumerate().map(|(i, b)| (b.id, i)).collect();

        let mut query = db.prepare(
            "SELECT message, emoji, count(*), max(author = ?3) FROM reactions
             WHERE message IN (SELECT id FROM messages WHERE conversation = ?1 AND id >= ?2)
             GROUP BY message, emoji ORDER BY min(sent_at)",
        )?;
        let reactions = query.query_map(params![conversation, oldest_id(&bubbles, first), me], |row| {
            Ok((row.get::<_, Id>(0)?, row.get::<_, String>(1)?, row.get::<_, u32>(2)?, row.get::<_, bool>(3)?))
        })?;
        for reaction in reactions {
            let (message, emoji, count, mine) = reaction?;
            if let Some(&i) = index.get(&message) {
                bubbles[i].reactions.push((emoji, count, mine));
            }
        }

        let mut query = db.prepare(&format!(
            "{ATTACHMENT} WHERE message IN (SELECT id FROM messages WHERE conversation = ?1 AND id >= ?2)
             ORDER BY message, position"
        ))?;
        for attachment in query.query_map(params![conversation, oldest_id(&bubbles, first)], attachment_row)? {
            let attachment = attachment?;
            if let Some(&i) = index.get(&attachment.message) {
                bubbles[i].attachments.push(attachment);
            }
        }
        Ok(bubbles)
    }
}

/// The lowest id among what is shown: messages sort by when they arrived,
/// so it need not be the first.
fn oldest_id(bubbles: &[Bubble], first: Id) -> Id {
    bubbles.iter().map(|b| b.id).min().unwrap_or(first)
}

fn attachment_files(db: &Connection, condition: &str, arg: i64) -> rusqlite::Result<Vec<String>> {
    let mut query = db.prepare(&format!("SELECT path FROM attachments WHERE path IS NOT NULL AND {condition}"))?;
    query.query_map([arg], |row| row.get(0))?.collect()
}
