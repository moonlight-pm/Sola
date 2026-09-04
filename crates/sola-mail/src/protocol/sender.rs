//! SMTP send via lettre.

use lettre::message::Mailbox;
use lettre::message::header::ContentType;
use lettre::message::{Attachment, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use tracing::debug;

use super::account::Account;
use super::types::MailAttachment;

/// Send an email via SMTP STARTTLS. Returns raw RFC822 for IMAP append.
pub fn send_mail(
    account: &Account,
    from: &str,
    to: &str,
    cc: Option<&str>,
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
    attachments: &[MailAttachment],
) -> anyhow::Result<Vec<u8>> {
    let email = build_message(from, to, cc, subject, body, in_reply_to, attachments)?;

    let creds = Credentials::new(account.username.clone(), account.password.clone());

    let mailer = SmtpTransport::starttls_relay(&account.smtp_host)
        .map_err(|e| anyhow::anyhow!("SMTP relay setup failed: {e}"))?
        .port(account.smtp_port)
        .credentials(creds)
        .build();

    mailer
        .send(&email)
        .map_err(|e| anyhow::anyhow!("SMTP send failed: {e}"))?;

    debug!("Email sent to {to}");
    Ok(email.formatted())
}

/// Build the RFC822 message (no network). Used by send and tests.
pub(crate) fn build_message(
    from: &str,
    to: &str,
    cc: Option<&str>,
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
    attachments: &[MailAttachment],
) -> anyhow::Result<Message> {
    let from: Mailbox = from
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid from address: {e}"))?;

    let mut builder = Message::builder().from(from).subject(subject);

    for addr in to.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let mailbox: Mailbox = addr
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid to address '{addr}': {e}"))?;
        builder = builder.to(mailbox);
    }

    if let Some(cc_str) = cc {
        for addr in cc_str
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            let mailbox: Mailbox = addr
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid cc address '{addr}': {e}"))?;
            builder = builder.cc(mailbox);
        }
    }

    if let Some(msg_id) = in_reply_to {
        builder = builder.in_reply_to(msg_id.to_string());
    }

    if attachments.is_empty() {
        return builder
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_string())
            .map_err(|e| anyhow::anyhow!("Failed to build email: {e}"));
    }

    let mut mp = MultiPart::mixed().singlepart(SinglePart::plain(body.to_string()));
    for att in attachments {
        let ct = att
            .mime
            .parse::<ContentType>()
            .unwrap_or_else(|_| ContentType::parse("application/octet-stream").expect("octet"));
        mp = mp.singlepart(Attachment::new(att.filename.clone()).body(att.bytes.to_vec(), ct));
    }
    builder
        .multipart(mp)
        .map_err(|e| anyhow::anyhow!("Failed to build email: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::attachments::collect_attachments;
    use mail_parser::MessageParser;
    use std::sync::Arc;

    #[test]
    fn multipart_roundtrip_keeps_pdf() {
        let att = MailAttachment {
            filename: "invoice.pdf".into(),
            mime: "application/pdf".into(),
            size: 4,
            bytes: Arc::from([1u8, 2, 3, 4]),
        };
        let msg = build_message(
            "me@example.com",
            "you@example.com",
            None,
            "invoice",
            "please find attached",
            None,
            &[att],
        )
        .expect("build");
        let raw = msg.formatted();
        let parsed = MessageParser::default().parse(&raw).expect("parse");
        let atts = collect_attachments(&parsed);
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].filename, "invoice.pdf");
        assert_eq!(&*atts[0].bytes, &[1, 2, 3, 4]);
    }

    #[test]
    fn plain_send_has_no_attachments() {
        let msg = build_message(
            "me@example.com",
            "you@example.com",
            None,
            "hi",
            "hello",
            None,
            &[],
        )
        .expect("build");
        let raw = msg.formatted();
        let parsed = MessageParser::default().parse(&raw).expect("parse");
        assert!(collect_attachments(&parsed).is_empty());
        assert!(parsed.body_text(0).is_some());
    }
}
