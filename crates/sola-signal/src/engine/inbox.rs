//! What a decrypted message does to the store.
//!
//! Nothing here touches the network: what has to follow (a receipt, a
//! group's state, a profile, a file) is handed back as [`Effects`] for the
//! engine to do.

use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use libsignal_core::{Aci, DeviceId, ServiceId};
use prost::Message;
use zkgroup::groups::{GroupMasterKey, GroupSecretParams};

use crate::proto::{
    self, AttachmentPointer, BodyRange, DataMessage, EditMessage, ReceiptMessage, SyncMessage, TypingMessage,
    addressable_message, body_range, conversation_identifier, data_message, receipt_message, sync_message,
    typing_message,
};
use crate::receive::{Received, service_id};
use crate::store::chats::{Id, NewAttachment, NewMessage, Quote, Status};
use crate::store::{Store, now};

/// Files fetched without being asked for are no larger than this.
const FETCH_UNASKED: u64 = 30 * 1024 * 1024;

/// A message to tell the user of.
pub struct Arrived {
    pub conversation: Id,
    pub sender: Id,
    pub text: String,
}

#[derive(Default)]
pub struct Effects {
    /// Conversations whose row or thread changed.
    pub changed: BTreeSet<Id>,
    /// Names, the block list or the account changed.
    pub people: bool,
    pub arrived: Vec<Arrived>,
    /// Delivery receipts owed: to whom, for which timestamp.
    pub receipts: Vec<(Id, u64)>,
    /// Groups whose state on the server is newer than ours.
    pub groups: Vec<Id>,
    /// People whose profile is worth fetching.
    pub profiles: Vec<Id>,
    /// The phone's contacts, as the pointer to fetch them by.
    pub contacts: Option<AttachmentPointer>,
    /// Attachments to fetch now.
    pub fetch: Vec<Id>,
    /// Someone started or stopped typing: the conversation, who, started.
    pub typing: Option<(Id, Id, bool)>,
    /// A device could not read what we sent at this timestamp.
    pub unread_by: Option<(Id, DeviceId, Vec<u8>)>,
    /// The account's storage record changed: read it again.
    pub storage: bool,
    /// Files of messages that are gone.
    pub remove: Vec<String>,
}

pub struct Inbox {
    pub store: Store,
    /// Our own row among the recipients.
    pub me: Id,
}

impl Inbox {
    pub fn take(&self, received: Received) -> Result<Effects> {
        let mut fx = Effects::default();
        let ServiceId::Aci(aci) = received.sender else { bail!("a message from a phone-number identity") };
        let sender = self.store.recipient_for(aci)?;
        use proto::content::Content as C;
        match received.content.content {
            Some(C::DataMessage(message)) => {
                self.data(sender, None, message, received.timestamp, received.server_timestamp, &mut fx)?
            }
            Some(C::EditMessage(edit)) => self.edit(sender, edit, &mut fx)?,
            // `receive` has checked that it is from our own account.
            Some(C::SyncMessage(sync)) => self.sync(sync, received.server_timestamp, &mut fx)?,
            Some(C::ReceiptMessage(receipt)) => self.receipt(receipt, &mut fx)?,
            Some(C::TypingMessage(typing)) => self.typing(sender, typing, &mut fx)?,
            Some(C::CallMessage(call)) if call.offer.is_some() => {
                let conversation = self.store.conversation_with(sender)?;
                let name = self.store.recipient(sender)?.name();
                self.note(conversation, sender, &format!("{name} called. Calls are answered on your phone."), received.server_timestamp, true, &mut fx)?;
            }
            Some(C::DecryptionErrorMessage(notice)) => fx.unread_by = Some((sender, received.sender_device, notice)),
            _ => {}
        }
        Ok(fx)
    }

    /// A message someone wrote. `sent` is set when it is our own, told to
    /// us by another of our devices. `placed` is where it goes in its
    /// thread: the server's time for it, ours and theirs alike, so messages
    /// that waited on the server come out in the order they were said.
    fn data(
        &self,
        author: Id,
        sent: Option<&sync_message::Sent>,
        message: DataMessage,
        timestamp: u64,
        placed: u64,
        fx: &mut Effects,
    ) -> Result<()> {
        if message.timestamp.is_some_and(|inner| inner != timestamp) {
            bail!("a message whose timestamp is not its envelope's");
        }
        let ours = sent.is_some();

        if let (Some(key), false) = (&message.profile_key, ours)
            && self.store.set_profile_key(author, key)?
        {
            fx.profiles.push(author);
        }

        let conversation = match (&message.group_v2, sent) {
            (Some(group), _) => {
                let master_key = group.master_key.as_deref().context("a group message with no group key")?;
                let group_id = self.store.group_for(master_key, &group_identifier(master_key)?)?;
                if self.store.group(group_id)?.revision < group.revision.unwrap_or(0) as i64 {
                    fx.groups.push(group_id);
                }
                self.store.conversation_in(group_id)?
            }
            (None, Some(sent)) => {
                let to = service_id(sent.destination_service_id_binary.as_deref(), sent.destination_service_id.as_deref())
                    .context("a transcript with no destination")?;
                let ServiceId::Aci(to) = to else { bail!("a transcript to a phone-number identity") };
                self.store.conversation_with(self.store.recipient_for(to)?)?
            }
            (None, None) => {
                if self.store.recipient(author)?.blocked {
                    return Ok(());
                }
                self.store.conversation_with(author)?
            }
        };

        if let Some(reaction) = &message.reaction {
            let target = aci(reaction.target_author_aci_binary.as_deref(), reaction.target_author_aci.as_deref())
                .context("a reaction to no one's message")?;
            let target = self.store.recipient_for(target)?;
            if let Some((id, conversation)) = self.store.find_message(target, reaction.target_sent_timestamp())? {
                let emoji = (!reaction.remove()).then(|| reaction.emoji());
                self.store.set_reaction(id, author, emoji, timestamp)?;
                fx.changed.insert(conversation);
            }
            return Ok(());
        }
        if let Some(delete) = &message.delete {
            if let Some((id, conversation)) = self.store.find_message(author, delete.target_sent_timestamp())? {
                fx.remove.extend(self.store.retract(id)?);
                fx.changed.insert(conversation);
            }
            return Ok(());
        }
        if let Some(delete) = &message.admin_delete {
            let target = aci(delete.target_author_aci_binary.as_deref(), None).context("an admin's delete of no one's message")?;
            let target = self.store.recipient_for(target)?;
            if let Some((id, found_in)) = self.store.find_message(target, delete.target_sent_timestamp())?
                && found_in == conversation
            {
                fx.remove.extend(self.store.retract(id)?);
                fx.changed.insert(conversation);
            }
            return Ok(());
        }

        let flags = message.flags();
        let timer = message.expire_timer();
        let timer_version = message.expire_timer_version.unwrap_or(1);
        if flags & data_message::Flags::ExpirationTimerUpdate as u32 != 0 {
            if self.store.set_timer(conversation, timer, timer_version)? {
                let who = if ours { "You".to_owned() } else { self.store.recipient(author)?.name() };
                let text = match timer {
                    0 => format!("{who} turned off disappearing messages."),
                    _ => format!("{who} set messages to disappear after {}.", duration(timer)),
                };
                self.note(conversation, author, &text, placed, false, fx)?;
            }
            return Ok(());
        }
        if flags & data_message::Flags::ProfileKeyUpdate as u32 != 0 {
            return Ok(());
        }
        // A message carries its conversation's timer; one that differs means
        // we missed the change.
        if self.store.conversation(conversation)?.expire_timer != timer {
            self.store.set_timer(conversation, timer, timer_version)?;
        }

        let mut body = mentions(message.body(), &message.body_ranges, |aci| {
            self.store.recipient_for(aci).and_then(|id| self.store.recipient(id)).map(|r| r.name()).ok()
        });
        let mut attachments: Vec<NewAttachment> = Vec::new();
        if message.is_view_once() {
            body = "A view-once message. Open it on your phone.".into();
        } else {
            attachments.extend(message.attachments.iter().map(attachment));
            if let Some(pointer) = message.sticker.as_ref().and_then(|sticker| sticker.data.as_ref()) {
                attachments.push(attachment(pointer));
            }
        }
        for contact in &message.contact {
            let name = contact.name.as_ref().map(|n| [n.given_name(), n.family_name()].join(" ")).unwrap_or_default();
            let numbers: Vec<&str> = contact.number.iter().map(|n| n.value()).collect();
            body = [body.as_str(), "Contact:", name.trim(), &numbers.join(", ")].join(" ").trim().to_owned();
        }
        if let Some(poll) = &message.poll_create {
            body = format!("Poll: {}\n{}", poll.question(), poll.options.iter().map(|o| format!("• {o}")).collect::<Vec<_>>().join("\n"));
        }
        if body.is_empty() && attachments.is_empty() {
            // Something this client has no words for (a payment, a badge,
            // a pin, a vote): nothing to keep.
            return Ok(());
        }
        let quote = match &message.quote {
            Some(quote) => aci(quote.author_aci_binary.as_deref(), quote.author_aci.as_deref())
                .map(|author| self.store.recipient_for(author))
                .transpose()?
                .map(|author| Quote { sent_at: quote.id(), author, text: quote.text().to_owned() }),
            None => None,
        };

        let fetch: Vec<bool> = attachments.iter().map(fetched_unasked).collect();
        let new = NewMessage {
            conversation,
            sender: author,
            sent_at: timestamp,
            received_at: placed,
            body,
            quote,
            expires_in: timer,
            expire_started: sent.and_then(|sent| sent.expiration_start_timestamp).or(ours.then_some(timestamp)),
            outgoing: ours,
            status: Some(Status::Sent),
            unread: !ours,
            attachments,
            ..Default::default()
        };
        let Some(id) = self.store.add_message(&new)? else { return Ok(()) };
        fx.changed.insert(conversation);
        for (attachment, fetch) in self.store.attachments_of(id)?.iter().zip(fetch) {
            if fetch {
                fx.fetch.push(attachment.id);
            }
        }
        if !ours {
            fx.receipts.push((author, timestamp));
            let text = if new.body.is_empty() { "Attachment".to_owned() } else { new.body.clone() };
            fx.arrived.push(Arrived { conversation, sender: author, text });
        }
        Ok(())
    }

    fn edit(&self, author: Id, edit: EditMessage, fx: &mut Effects) -> Result<()> {
        let Some(message) = &edit.data_message else { return Ok(()) };
        let Some((id, conversation)) = self.store.find_message(author, edit.target_sent_timestamp())? else {
            return Ok(());
        };
        let body = mentions(message.body(), &message.body_ranges, |aci| {
            self.store.recipient_for(aci).and_then(|id| self.store.recipient(id)).map(|r| r.name()).ok()
        });
        self.store.edit_message(id, message.timestamp(), &body)?;
        fx.changed.insert(conversation);
        Ok(())
    }

    /// What another of our devices tells this one.
    fn sync(&self, sync: SyncMessage, placed: u64, fx: &mut Effects) -> Result<()> {
        let now = now();
        for read in sync.read.iter().map(|r| (&r.sender_aci_binary, &r.sender_aci, r.timestamp())).chain(
            sync.viewed.iter().map(|v| (&v.sender_aci_binary, &v.sender_aci, v.timestamp())),
        ) {
            let (binary, text, timestamp) = read;
            let Some(sender) = aci(binary.as_deref(), text.as_deref()) else { continue };
            if let Some((id, conversation)) = self.store.find_message(self.store.recipient_for(sender)?, timestamp)? {
                self.store.read_message(id, now)?;
                fx.changed.insert(conversation);
            }
        }
        use sync_message::Content as S;
        match sync.content {
            Some(S::Sent(mut sent)) => {
                let timestamp = sent.timestamp();
                if let Some(message) = sent.message.take() {
                    self.data(self.me, Some(&sent), message, timestamp, placed, fx)?;
                } else if let Some(edit) = sent.edit_message.take() {
                    self.edit(self.me, edit, fx)?;
                }
            }
            Some(S::Contacts(contacts)) => fx.contacts = contacts.blob,
            Some(S::Blocked(blocked)) => {
                let acis = blocked
                    .acis
                    .iter()
                    .filter_map(|aci| Aci::parse_from_service_id_string(aci))
                    .chain(blocked.acis_binary.iter().filter_map(|bytes| aci(Some(bytes), None)))
                    .chain(blocked.blocked_acis.iter().filter_map(|b| aci(b.aci_binary.as_deref(), None)));
                let ids = acis.map(|aci| self.store.recipient_for(aci)).collect::<Result<Vec<_>>>()?;
                self.store.set_blocked(&ids)?;
                fx.people = true;
            }
            Some(S::Configuration(configuration)) => {
                if let Some(on) = configuration.read_receipts {
                    self.store.set_read_receipts(on)?;
                    fx.people = true;
                }
            }
            Some(S::Keys(keys)) => {
                self.store.set_account_keys(keys.account_entropy_pool.as_deref(), keys.media_root_backup_key.as_deref())?;
                fx.people = true;
                fx.storage = true;
            }
            Some(S::FetchLatest(latest)) => match latest.r#type() {
                sync_message::fetch_latest::Type::StorageManifest => fx.storage = true,
                sync_message::fetch_latest::Type::LocalProfile => fx.profiles.push(self.me),
                _ => {}
            },
            Some(S::DeleteForMe(delete)) => self.delete_for_me(delete, fx)?,
            _ => {}
        }
        Ok(())
    }

    /// What was deleted on another of our devices goes here too.
    fn delete_for_me(&self, delete: sync_message::DeleteForMe, fx: &mut Effects) -> Result<()> {
        for deletes in &delete.message_deletes {
            for message in &deletes.messages {
                let author = match &message.author {
                    Some(addressable_message::Author::AuthorServiceId(text)) => aci(None, Some(text)),
                    Some(addressable_message::Author::AuthorServiceIdBinary(bytes)) => aci(Some(bytes), None),
                    _ => None,
                };
                let Some(author) = author else { continue };
                if let Some((id, conversation)) = self.store.find_message(self.store.recipient_for(author)?, message.sent_timestamp())? {
                    fx.remove.extend(self.store.delete_message(id)?);
                    fx.changed.insert(conversation);
                }
            }
        }
        let whole = delete.conversation_deletes.iter().map(|d| &d.conversation);
        let local = delete.local_only_conversation_deletes.iter().map(|d| &d.conversation);
        for identifier in whole.chain(local).flatten() {
            use conversation_identifier::Identifier as I;
            let conversation = match &identifier.identifier {
                Some(I::ThreadGroupId(group)) => {
                    self.store.group_by_identifier(group)?.map(|group| self.store.conversation_in(group)).transpose()?
                }
                Some(I::ThreadServiceId(text)) => self.direct(aci(None, Some(text)))?,
                Some(I::ThreadServiceIdBinary(bytes)) => self.direct(aci(Some(bytes), None))?,
                _ => None,
            };
            if let Some(conversation) = conversation {
                self.store.delete_conversation(conversation)?;
                fx.changed.insert(conversation);
            }
        }
        Ok(())
    }

    fn direct(&self, aci: Option<Aci>) -> Result<Option<Id>> {
        aci.map(|aci| self.store.conversation_with(self.store.recipient_for(aci)?)).transpose()
    }

    /// Our messages got further.
    fn receipt(&self, receipt: ReceiptMessage, fx: &mut Effects) -> Result<()> {
        let status = match receipt.r#type() {
            receipt_message::Type::Delivery => Status::Delivered,
            receipt_message::Type::Read | receipt_message::Type::Viewed => Status::Read,
        };
        for timestamp in &receipt.timestamp {
            if let Some((id, conversation)) = self.store.find_message(self.me, *timestamp)?
                && self.store.raise_status(id, status)?
            {
                fx.changed.insert(conversation);
            }
        }
        Ok(())
    }

    fn typing(&self, sender: Id, typing: TypingMessage, fx: &mut Effects) -> Result<()> {
        let conversation = match &typing.group_id {
            Some(group) => match self.store.group_by_identifier(group)? {
                Some(group) => self.store.conversation_in(group)?,
                None => return Ok(()),
            },
            None => self.store.conversation_with(sender)?,
        };
        fx.typing = Some((conversation, sender, typing.action() == typing_message::Action::Started));
        Ok(())
    }

    /// A line of the client's own in a conversation.
    pub fn note(&self, conversation: Id, about: Id, text: &str, placed: u64, unread: bool, fx: &mut Effects) -> Result<()> {
        let new = NewMessage {
            conversation,
            sender: about,
            sent_at: now(),
            received_at: placed,
            note: true,
            body: text.to_owned(),
            unread,
            ..Default::default()
        };
        self.store.add_message(&new)?;
        fx.changed.insert(conversation);
        Ok(())
    }
}

/// What a group is known by outside its members: typing messages and
/// deletes name it so.
pub fn group_identifier(master_key: &[u8]) -> Result<[u8; 32]> {
    let master_key: [u8; 32] = master_key.try_into().ok().context("a group key that is not 32 bytes")?;
    Ok(GroupSecretParams::derive_from_master_key(GroupMasterKey::new(master_key)).get_group_identifier())
}

pub fn aci(binary: Option<&[u8]>, text: Option<&str>) -> Option<Aci> {
    match service_id(binary, text)? {
        ServiceId::Aci(aci) => Some(aci),
        ServiceId::Pni(_) => None,
    }
}

fn attachment(pointer: &AttachmentPointer) -> NewAttachment {
    NewAttachment {
        content_type: pointer.content_type().to_owned(),
        file_name: pointer.file_name.clone(),
        size: pointer.size() as u64,
        width: pointer.width(),
        height: pointer.height(),
        flags: pointer.flags(),
        pointer: Some(pointer.encode_to_vec()),
        path: None,
    }
}

/// Pictures and sound come by themselves; the rest when asked for.
fn fetched_unasked(attachment: &NewAttachment) -> bool {
    let kind = attachment.content_type.as_str();
    (kind.starts_with("image/") || kind.starts_with("audio/")) && attachment.size <= FETCH_UNASKED
}

/// The body with each mention's placeholder replaced by `@name`. Ranges
/// count UTF-16 units, as the clients that wrote them do.
fn mentions(body: &str, ranges: &[BodyRange], name: impl Fn(Aci) -> Option<String>) -> String {
    let mut found: Vec<(usize, usize, Aci)> = ranges
        .iter()
        .filter_map(|range| {
            let who = match range.associated_value.as_ref()? {
                body_range::AssociatedValue::MentionAci(text) => aci(None, Some(text)),
                body_range::AssociatedValue::MentionAciBinary(bytes) => aci(Some(bytes), None),
                body_range::AssociatedValue::Style(_) => None,
            }?;
            Some((range.start() as usize, range.length() as usize, who))
        })
        .collect();
    if found.is_empty() {
        return body.to_owned();
    }
    found.sort_by_key(|(start, ..)| std::cmp::Reverse(*start));
    let mut units: Vec<u16> = body.encode_utf16().collect();
    for (start, length, who) in found {
        if start + length > units.len() {
            continue;
        }
        let mention = format!("@{}", name(who).unwrap_or_else(|| "someone".into()));
        units.splice(start..start + length, mention.encode_utf16());
    }
    String::from_utf16_lossy(&units)
}

/// A timer as words: "1 week", "30 seconds".
pub fn duration(seconds: u32) -> String {
    const UNITS: &[(u32, &str)] = &[(604_800, "week"), (86_400, "day"), (3600, "hour"), (60, "minute"), (1, "second")];
    for (size, unit) in UNITS {
        if seconds >= *size && seconds % size == 0 {
            let count = seconds / size;
            return format!("{count} {unit}{}", if count == 1 { "" } else { "s" });
        }
    }
    format!("{seconds} seconds")
}

#[cfg(test)]
mod tests {
    use libsignal_core::DeviceId;

    use super::*;
    use crate::proto::Content;
    use crate::store::chats::Kind;

    struct Desk {
        inbox: Inbox,
        dir: std::path::PathBuf,
        us: Aci,
        alice: Aci,
    }

    impl Drop for Desk {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn desk() -> Desk {
        let dir = std::env::temp_dir().join(format!("sola-signal-test-{}", uuid::Uuid::new_v4()));
        let store = Store::open(&dir.join("signal.db")).unwrap();
        let us = Aci::from(uuid::Uuid::from_u128(0xb0b));
        let me = store.recipient_for(us).unwrap();
        Desk { inbox: Inbox { store, me }, dir, us, alice: Aci::from(uuid::Uuid::from_u128(0xa11ce)) }
    }

    impl Desk {
        fn from(&self, sender: Aci, timestamp: u64, content: proto::content::Content) -> Effects {
            let received = Received {
                sender: sender.into(),
                sender_device: DeviceId::new(1).unwrap(),
                timestamp,
                server_timestamp: timestamp + 1,
                content: Content { content: Some(content), ..Default::default() },
            };
            self.inbox.take(received).unwrap()
        }

        fn says(&self, sender: Aci, timestamp: u64, message: DataMessage) -> Effects {
            self.from(sender, timestamp, proto::content::Content::DataMessage(message))
        }

        fn thread(&self, conversation: Id) -> Vec<crate::store::chats::Bubble> {
            self.inbox.store.thread(conversation, 50, self.inbox.me).unwrap()
        }
    }

    fn text(body: &str, timestamp: u64) -> DataMessage {
        DataMessage { body: Some(body.into()), timestamp: Some(timestamp), ..Default::default() }
    }

    #[test]
    fn a_message_is_kept_once_reacted_to_edited_and_taken_back() {
        let desk = desk();
        let fx = desk.says(desk.alice, 100, DataMessage { profile_key: Some(vec![7; 32]), ..text("hello", 100) });
        let conversation = *fx.changed.first().unwrap();
        let alice = desk.inbox.store.recipient_for(desk.alice).unwrap();
        assert_eq!(fx.receipts, [(alice, 100)]);
        assert_eq!(fx.profiles, [alice]);
        assert_eq!(fx.arrived.len(), 1);
        // The server delivers it again: kept once, no second receipt.
        let again = desk.says(desk.alice, 100, text("hello", 100));
        assert!(again.receipts.is_empty());
        assert_eq!(desk.thread(conversation).len(), 1);
        assert_eq!(desk.inbox.store.chats().unwrap()[0].unread, 1);

        // We react to it from the phone: a transcript, named by our own account.
        let reaction = data_message::Reaction {
            emoji: Some("👍".into()),
            target_author_aci: Some(desk.alice.service_id_string()),
            target_sent_timestamp: Some(100),
            ..Default::default()
        };
        let sent = sync_message::Sent {
            destination_service_id: Some(desk.alice.service_id_string()),
            timestamp: Some(150),
            message: Some(DataMessage { reaction: Some(reaction), timestamp: Some(150), ..Default::default() }),
            ..Default::default()
        };
        let sync = SyncMessage { content: Some(sync_message::Content::Sent(sent)), ..Default::default() };
        desk.from(desk.us, 150, proto::content::Content::SyncMessage(sync));
        assert_eq!(desk.thread(conversation)[0].reactions, [("👍".to_owned(), 1, true)]);

        // She edits it, then takes it back.
        let edit = EditMessage { target_sent_timestamp: Some(100), data_message: Some(text("hello!", 200)) };
        desk.from(desk.alice, 200, proto::content::Content::EditMessage(edit));
        let shown = desk.thread(conversation);
        assert_eq!((shown[0].body.as_str(), shown[0].edited), ("hello!", true));
        let delete = DataMessage {
            delete: Some(data_message::Delete { target_sent_timestamp: Some(200) }),
            timestamp: Some(300),
            ..Default::default()
        };
        desk.says(desk.alice, 300, delete);
        let shown = desk.thread(conversation);
        assert!(shown[0].deleted && shown[0].body.is_empty() && shown[0].reactions.is_empty());
    }

    #[test]
    fn what_we_sent_elsewhere_is_ours_and_receipts_move_it_on() {
        let desk = desk();
        let sent = sync_message::Sent {
            destination_service_id: Some(desk.alice.service_id_string()),
            timestamp: Some(500),
            message: Some(text("from the phone", 500)),
            ..Default::default()
        };
        let sync = SyncMessage { content: Some(sync_message::Content::Sent(sent)), ..Default::default() };
        let fx = desk.from(desk.us, 500, proto::content::Content::SyncMessage(sync));
        let conversation = *fx.changed.first().unwrap();
        assert!(fx.receipts.is_empty() && fx.arrived.is_empty());
        let shown = desk.thread(conversation);
        assert!(shown[0].outgoing);
        assert_eq!(shown[0].status, Status::Sent);

        let read = ReceiptMessage { r#type: Some(receipt_message::Type::Read as i32), timestamp: vec![500] };
        desk.from(desk.alice, 600, proto::content::Content::ReceiptMessage(read));
        let delivered = ReceiptMessage { r#type: Some(receipt_message::Type::Delivery as i32), timestamp: vec![500] };
        desk.from(desk.alice, 700, proto::content::Content::ReceiptMessage(delivered));
        // A delivery heard after a read does not take the read back.
        assert_eq!(desk.thread(conversation)[0].status, Status::Read);
    }

    #[test]
    fn what_waited_on_the_server_comes_out_in_the_order_it_was_said() {
        let desk = desk();
        let ours = |body: &str, at: u64| {
            let sent = sync_message::Sent {
                destination_service_id: Some(desk.alice.service_id_string()),
                timestamp: Some(at),
                message: Some(text(body, at)),
                ..Default::default()
            };
            let sync = SyncMessage { content: Some(sync_message::Content::Sent(sent)), ..Default::default() };
            desk.from(desk.us, at, proto::content::Content::SyncMessage(sync))
        };
        // A conversation held on the phone, all of it arriving here at once.
        let fx = ours("one", 100);
        desk.says(desk.alice, 200, text("two", 200));
        ours("three", 300);
        desk.says(desk.alice, 400, text("four", 400));
        let conversation = *fx.changed.first().unwrap();
        let said: Vec<String> = desk.thread(conversation).into_iter().map(|bubble| bubble.body).collect();
        assert_eq!(said, ["one", "two", "three", "four"]);
    }

    #[test]
    fn a_timer_change_is_a_note_and_later_messages_disappear() {
        let desk = desk();
        let change = DataMessage {
            flags: Some(data_message::Flags::ExpirationTimerUpdate as u32),
            expire_timer: Some(3600),
            timestamp: Some(100),
            ..Default::default()
        };
        let fx = desk.says(desk.alice, 100, change);
        let conversation = *fx.changed.first().unwrap();
        let shown = desk.thread(conversation);
        assert_eq!(shown[0].kind, Kind::Note);
        assert!(shown[0].body.contains("1 hour"), "{}", shown[0].body);
        assert_eq!(desk.inbox.store.conversation(conversation).unwrap().expire_timer, 3600);

        desk.says(desk.alice, 200, DataMessage { expire_timer: Some(3600), ..text("soon gone", 200) });
        // Its hour starts when it is read, and it is gone when the hour is up.
        assert_eq!(desk.inbox.store.next_expiry().unwrap(), None);
        desk.inbox.store.read_conversation(conversation, 10_000).unwrap();
        assert_eq!(desk.inbox.store.next_expiry().unwrap(), Some(10_000 + 3_600_000));
        desk.inbox.store.expire(10_000 + 3_600_000).unwrap();
        assert_eq!(desk.thread(conversation).len(), 1);
    }

    #[test]
    fn a_mention_reads_as_a_name() {
        let desk = desk();
        let alice = desk.inbox.store.recipient_for(desk.alice).unwrap();
        desk.inbox.store.set_contact(alice, None, Some("Alice")).unwrap();
        let range = BodyRange {
            start: Some(4),
            length: Some(1),
            associated_value: Some(body_range::AssociatedValue::MentionAci(desk.alice.service_id_string())),
        };
        let body = mentions("ask \u{fffc} 🙂", &[range], |aci| (aci == desk.alice).then(|| "Alice".to_owned()));
        assert_eq!(body, "ask @Alice 🙂");
        assert_eq!(duration(604_800), "1 week");
        assert_eq!(duration(90), "90 seconds");
    }
}
