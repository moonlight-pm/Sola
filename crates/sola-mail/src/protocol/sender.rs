//! Outgoing mail: RFC 5322 build (lettre) + SMTP submit.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use lettre::address::Envelope;
use lettre::message::header::{self, ContentType, HeaderName, HeaderValue};
use lettre::message::{Attachment, Mailbox, Mailboxes, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use tracing::debug;

use super::account::Account;
use super::types::{MailAttachment, MailId};

const SMTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Implicit-TLS submission port (everything else negotiates STARTTLS).
const SMTPS_PORT: u16 = 465;

/// A message the user wrote: sent, or saved to Drafts.
#[derive(Debug, Clone, Default)]
pub struct Outgoing {
    pub from: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub body: String,
    /// Message-ID being replied to (with or without angle brackets).
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub attachments: Vec<MailAttachment>,
    /// Draft this message supersedes (deleted after a successful send/save).
    pub replaces: Option<MailId>,
}

/// Submit over SMTP. Returns the raw RFC 5322 bytes for the Sent copy.
pub fn send_mail(account: &Account, msg: &Outgoing) -> anyhow::Result<Vec<u8>> {
    let email = build_message(msg, false)?;
    let relay = if account.smtp_port == SMTPS_PORT {
        SmtpTransport::relay(&account.smtp_host)
    } else {
        SmtpTransport::starttls_relay(&account.smtp_host)
    }
    .map_err(|e| anyhow::anyhow!("can't set up SMTP to {}: {e}", account.smtp_host))?;
    let mailer = relay
        .port(account.smtp_port)
        .credentials(Credentials::new(
            account.username.clone(),
            account.password.clone(),
        ))
        .timeout(Some(SMTP_TIMEOUT))
        .build();
    mailer.send(&email).map_err(|e| anyhow::anyhow!("{e}"))?;
    debug!(to = msg.to.as_str(), "mail sent");
    Ok(email.formatted())
}

/// Build the message (no network). Drafts keep whatever the user typed in
/// To/Cc even when it does not parse yet, and need no recipient.
pub fn build_message(msg: &Outgoing, draft: bool) -> anyhow::Result<Message> {
    let from: Mailbox = msg
        .from
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("“{}” is not a valid From address", msg.from.trim()))?;
    let mut builder = Message::builder()
        .from(from.clone())
        .subject(msg.subject.clone())
        .message_id(Some(new_message_id(&from)));

    for (name, raw, required) in [
        (HeaderName::new_from_ascii_str("To"), msg.to.trim(), !draft),
        (HeaderName::new_from_ascii_str("Cc"), msg.cc.trim(), false),
    ] {
        if raw.is_empty() {
            if required {
                anyhow::bail!("add at least one recipient");
            }
            continue;
        }
        match raw.parse::<Mailboxes>() {
            Ok(list) if &*name == "To" => builder = builder.mailbox(header::To::from(list)),
            Ok(list) => builder = builder.mailbox(header::Cc::from(list)),
            Err(_) if draft => builder = builder.raw_header(HeaderValue::new(name, raw.to_string())),
            Err(_) => anyhow::bail!("can't read the {} addresses “{raw}”", &*name),
        }
    }
    if draft {
        builder = builder.envelope(
            Envelope::new(Some(from.email.clone()), vec![from.email.clone()])
                .map_err(|e| anyhow::anyhow!("draft envelope: {e}"))?,
        );
    }

    if let Some(parent) = msg.in_reply_to.as_deref().map(angle).filter(|s| s.len() > 2) {
        let mut refs: Vec<String> = msg.references.iter().map(|r| angle(r)).collect();
        if !refs.contains(&parent) {
            refs.push(parent.clone());
        }
        builder = builder.in_reply_to(parent).references(refs.join(" "));
    }

    let built = if msg.attachments.is_empty() {
        builder
            .header(ContentType::TEXT_PLAIN)
            .body(msg.body.clone())
    } else {
        let mut mp = MultiPart::mixed().singlepart(SinglePart::plain(msg.body.clone()));
        for att in &msg.attachments {
            let ct = att
                .mime
                .parse::<ContentType>()
                .unwrap_or_else(|_| ContentType::parse("application/octet-stream").expect("octet"));
            mp = mp.singlepart(Attachment::new(att.filename.clone()).body(att.bytes.to_vec(), ct));
        }
        builder.multipart(mp)
    };
    built.map_err(|e| anyhow::anyhow!("can't build the message: {e}"))
}

/// `<id>` — In-Reply-To / References need the angle brackets.
fn angle(id: &str) -> String {
    let t = id.trim().trim_start_matches('<').trim_end_matches('>');
    format!("<{t}>")
}

/// `<time.pid.n@from-domain>`: unique enough, and no local hostname leak.
fn new_message_id(from: &Mailbox) -> String {
    static N: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    format!(
        "<{nanos:x}.{:x}.{n}@{}>",
        std::process::id(),
        from.email.domain()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::attachments::collect_attachments;
    use mail_parser::MessageParser;
    use std::sync::Arc;

    fn outgoing(to: &str) -> Outgoing {
        Outgoing {
            from: "me@example.com".into(),
            to: to.into(),
            subject: "hi".into(),
            body: "hello".into(),
            ..Outgoing::default()
        }
    }

    fn roundtrip(msg: &Outgoing, draft: bool) -> Vec<u8> {
        build_message(msg, draft).expect("build").formatted()
    }

    #[test]
    fn multipart_roundtrip_keeps_pdf() {
        let mut msg = outgoing("you@example.com");
        msg.attachments.push(MailAttachment {
            filename: "invoice.pdf".into(),
            mime: "application/pdf".into(),
            size: 4,
            bytes: Arc::from([1u8, 2, 3, 4]),
        });
        let raw = roundtrip(&msg, false);
        let parsed = MessageParser::default().parse(&raw).expect("parse");
        let atts = collect_attachments(&parsed);
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].filename, "invoice.pdf");
        assert_eq!(&*atts[0].bytes, &[1, 2, 3, 4]);
    }

    #[test]
    fn plain_send_has_message_id_and_no_attachments() {
        let raw = roundtrip(&outgoing("you@example.com"), false);
        let parsed = MessageParser::default().parse(&raw).expect("parse");
        assert!(collect_attachments(&parsed).is_empty());
        assert!(parsed.body_text(0).is_some());
        assert!(parsed.message_id().is_some_and(|id| id.ends_with("@example.com")));
    }

    #[test]
    fn quoted_comma_names_parse_as_one_recipient() {
        let raw = roundtrip(&outgoing(r#""Doe, Jane" <jane@example.com>, bob@example.com"#), false);
        let parsed = MessageParser::default().parse(&raw).expect("parse");
        assert_eq!(parsed.to().map(|a| a.iter().count()), Some(2));
    }

    #[test]
    fn reply_threads_with_brackets() {
        let mut msg = outgoing("you@example.com");
        msg.in_reply_to = Some("parent@x".into());
        msg.references = vec!["<root@x>".into()];
        let raw = String::from_utf8(roundtrip(&msg, false)).expect("utf8");
        assert!(raw.contains("In-Reply-To: <parent@x>"), "{raw}");
        assert!(raw.contains("References: <root@x> <parent@x>"), "{raw}");
    }

    #[test]
    fn send_needs_a_recipient_but_a_draft_does_not() {
        assert!(build_message(&outgoing(""), false).is_err());
        assert!(build_message(&outgoing(""), true).is_ok());
        let raw = String::from_utf8(roundtrip(&outgoing("half typed@"), true)).expect("utf8");
        assert!(raw.contains("To: half typed@"), "{raw}");
        assert!(build_message(&outgoing("half typed@"), false).is_err());
    }
}
