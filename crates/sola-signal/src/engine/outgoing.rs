//! What we say, as Signal carries it: a message of ours written to the
//! store and the outbox, and the `Content` built from it when it goes.

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use prost::Message;

use super::Shared;
use crate::proto::{self, AttachmentPointer, Content, DataMessage, GroupContextV2, data_message};
use crate::send::Failure;
use crate::store::chats::{Id, NewAttachment, NewMessage, Outgoing, Status};
use crate::store::{Store, now};

/// What every message to a conversation carries: its time, our profile key
/// (so the other side can show our name), the conversation's timer and,
/// in a group, which group.
pub fn frame(shared: &Shared, conversation: Id, timestamp: u64) -> Result<DataMessage> {
    let account = shared.account.borrow().clone().context("not linked")?;
    let conversation = shared.store.conversation(conversation)?;
    let group = match conversation.group {
        Some(group) => {
            let group = shared.store.group(group)?;
            Some(GroupContextV2 {
                master_key: Some(group.master_key),
                revision: Some(group.revision.max(0) as u32),
                group_change: None,
            })
        }
        None => None,
    };
    Ok(DataMessage {
        timestamp: Some(timestamp),
        profile_key: Some(account.profile_key),
        expire_timer: (conversation.expire_timer > 0).then_some(conversation.expire_timer),
        expire_timer_version: Some(conversation.expire_timer_version),
        group_v2: group,
        ..Default::default()
    })
}

/// Put something in the outbox for a conversation.
pub fn enqueue(
    shared: &Shared,
    conversation: Id,
    message: Option<Id>,
    timestamp: u64,
    content: proto::content::Content,
) -> Result<()> {
    let content = Content { content: Some(content), ..Default::default() };
    shared.store.enqueue(&Outgoing {
        id: 0,
        conversation: Some(conversation),
        recipient: None,
        message,
        timestamp,
        content: Some(content.encode_to_vec()),
        urgent: true,
    })?;
    Ok(())
}

/// Keep a message of ours and queue it to go.
pub fn compose(shared: &Shared, conversation: Id, body: &str, quote: Option<Id>, files: &[PathBuf]) -> Result<()> {
    let store = &shared.store;
    let timestamp = now();
    let quote = quote.map(|quoted| store.quoted(quoted)).transpose()?;
    let attachments = files.iter().map(|file| keep(file)).collect::<Result<Vec<_>>>()?;
    let new = NewMessage {
        conversation,
        sender: shared.me.get(),
        sent_at: timestamp,
        received_at: timestamp,
        body: body.to_owned(),
        quote,
        expires_in: store.conversation(conversation)?.expire_timer,
        outgoing: true,
        status: Some(Status::Sending),
        attachments,
        ..Default::default()
    };
    let message = store.add_message(&new)?.context("two messages in one millisecond")?;
    store.enqueue(&Outgoing {
        id: 0,
        conversation: Some(conversation),
        recipient: None,
        message: Some(message),
        timestamp,
        content: None,
        urgent: true,
    })?;
    Ok(())
}

/// A file to send, copied beside the store so the message keeps it.
fn keep(file: &PathBuf) -> Result<NewAttachment> {
    let name = file.file_name().and_then(|name| name.to_str()).unwrap_or("file").to_owned();
    let extension = file.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    let dir = Store::files_dir();
    std::fs::create_dir_all(&dir)?;
    let mut kept = dir.join(uuid::Uuid::new_v4().to_string());
    if !extension.is_empty() {
        kept.set_extension(&extension);
    }
    let size = std::fs::copy(file, &kept).with_context(|| file.display().to_string())?;
    let content_type = content_type(&extension);
    let (width, height) = match content_type.starts_with("image/") {
        true => image::image_dimensions(&kept).unwrap_or_default(),
        false => (0, 0),
    };
    Ok(NewAttachment {
        content_type: content_type.to_owned(),
        file_name: Some(name),
        size,
        width,
        height,
        flags: 0,
        pointer: None,
        path: Some(kept.to_string_lossy().into_owned()),
    })
}

pub fn content_type(extension: &str) -> &'static str {
    match extension {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "svg" => "image/svg+xml",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "m4a" | "aac" => "audio/aac",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "txt" | "md" | "log" => "text/plain",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

/// The extension a file of this type is kept under.
pub fn extension(content_type: &str) -> &'static str {
    match content_type.split(';').next().unwrap_or_default().trim() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/heic" => "heic",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        "video/webm" => "webm",
        "audio/mpeg" => "mp3",
        "audio/aac" | "audio/mp4" => "m4a",
        "audio/ogg" => "ogg",
        "audio/wav" => "wav",
        "application/pdf" => "pdf",
        "application/zip" => "zip",
        "text/plain" => "txt",
        _ => "bin",
    }
}

/// The `Content` of a stored message of ours, its files uploaded first.
pub async fn message(shared: &Shared, conversation: Id, message: Id, timestamp: u64) -> Result<Content, Failure> {
    let never = Failure::Never;
    let store = &shared.store;
    let (body, quote, expires_in) = store.message_to_send(message).map_err(never)?;
    let mut said = frame(shared, conversation, timestamp).map_err(never)?;
    said.body = (!body.is_empty()).then_some(body);
    said.expire_timer = (expires_in > 0).then_some(expires_in);
    if let Some(quote) = quote {
        let author = store.recipient(quote.author).map_err(never)?.aci.context("a quote of no one").map_err(never)?;
        said.quote = Some(data_message::Quote {
            id: Some(quote.sent_at),
            author_aci: Some(author.service_id_string()),
            text: Some(quote.text),
            r#type: Some(data_message::quote::Type::Normal as i32),
            ..Default::default()
        });
    }
    for attachment in store.attachments_of(message).map_err(never)? {
        let pointer = match &attachment.pointer {
            Some(pointer) => AttachmentPointer::decode(pointer.as_slice()).map_err(|e| never(anyhow!("attachment: {e}")))?,
            None => {
                let pointer = super::files::upload(shared, &attachment).await?;
                store.set_attachment_pointer(attachment.id, &pointer.encode_to_vec()).map_err(never)?;
                pointer
            }
        };
        said.attachments.push(pointer);
    }
    Ok(Content { content: Some(proto::content::Content::DataMessage(said)), ..Default::default() })
}
