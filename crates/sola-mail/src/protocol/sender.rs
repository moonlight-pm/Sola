//! SMTP send via lettre.

use lettre::message::Mailbox;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use tracing::debug;

use super::account::Account;

/// Send an email via SMTP STARTTLS. Returns raw RFC822 for IMAP append.
pub fn send_mail(
    account: &Account,
    from: &str,
    to: &str,
    cc: Option<&str>,
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
) -> anyhow::Result<Vec<u8>> {
    let from: Mailbox = from
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid from address: {e}"))?;

    let mut builder = Message::builder()
        .from(from)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN);

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

    let email = builder
        .body(body.to_string())
        .map_err(|e| anyhow::anyhow!("Failed to build email: {e}"))?;

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
