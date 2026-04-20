use lettre::message::Mailbox;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use tracing::debug;

use crate::config::MailConfig;

/// Send an email via SMTP STARTTLS.
pub fn send_mail(
    config: &MailConfig,
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

    // Parse To addresses (comma-separated)
    for addr in to.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let mailbox: Mailbox = addr
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid to address '{addr}': {e}"))?;
        builder = builder.to(mailbox);
    }

    // Parse CC addresses
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

    // Set In-Reply-To header for threading
    if let Some(msg_id) = in_reply_to {
        builder = builder.in_reply_to(msg_id.to_string());
    }

    let email = builder
        .body(body.to_string())
        .map_err(|e| anyhow::anyhow!("Failed to build email: {e}"))?;

    let creds = Credentials::new(config.username.clone(), config.password.clone());

    let mailer = SmtpTransport::starttls_relay(&config.smtp_host)
        .map_err(|e| anyhow::anyhow!("SMTP relay setup failed: {e}"))?
        .port(config.smtp_port)
        .credentials(creds)
        .build();

    mailer
        .send(&email)
        .map_err(|e| anyhow::anyhow!("SMTP send failed: {e}"))?;

    debug!("Email sent to {to}");

    // Return the raw RFC822 message for IMAP append
    Ok(email.formatted())
}
