//! A received message as one line of text, for the spike's terminal.

use libsignal_core::ServiceId;

use crate::proto::content::Content as Kind;
use crate::proto::{DataMessage, sync_message};
use crate::receive::Received;

pub fn line(received: &Received) -> String {
    let from = format!("{}.{}", received.sender.service_id_string(), received.sender_device);
    let what = match &received.content.content {
        Some(Kind::DataMessage(message)) => data(message),
        Some(Kind::SyncMessage(sync)) => match &sync.content {
            Some(sync_message::Content::Sent(sent)) => {
                let to = ServiceId::parse_from_service_id_binary(sent.destination_service_id_binary())
                    .map(|id| id.service_id_string())
                    .or_else(|| sent.destination_service_id.clone())
                    .unwrap_or_else(|| "a group or ourselves".into());
                match &sent.message {
                    Some(message) => format!("we sent to {to}: {}", data(message)),
                    None => format!("we sent to {to}: something that is not a message"),
                }
            }
            other => format!("sync: {}", variant(other)),
        },
        Some(Kind::ReceiptMessage(receipt)) => {
            format!("receipt: {:?} for {} message(s)", receipt.r#type(), receipt.timestamp.len())
        }
        Some(Kind::TypingMessage(typing)) => format!("typing {:?}", typing.action()),
        other => variant(other),
    };
    format!("[{}] {from}: {what}", received.timestamp)
}

fn data(message: &DataMessage) -> String {
    let mut parts = Vec::new();
    if let Some(body) = &message.body {
        parts.push(format!("{body:?}"));
    }
    if !message.attachments.is_empty() {
        parts.push(format!("{} attachment(s)", message.attachments.len()));
    }
    if let Some(reaction) = &message.reaction {
        parts.push(format!("reaction {}", reaction.emoji()));
    }
    if message.group_v2.is_some() {
        parts.push("in a group".into());
    }
    if parts.is_empty() {
        parts.push("a message with none of the parts shown here".into());
    }
    parts.join(", ")
}

/// The name of an enum's variant, from its `Debug`.
fn variant(value: &impl std::fmt::Debug) -> String {
    let text = format!("{value:?}");
    let name = text.strip_prefix("Some(").unwrap_or(&text);
    name.split(['(', ' ', '{']).next().unwrap_or(name).to_string()
}
