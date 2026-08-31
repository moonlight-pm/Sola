//! Local account view over bus [`MailConfig`].
//!
//! Protocol code (IMAP/SMTP/IDLE) needs a plain password string and does
//! not depend on bus encryption. Construct once at the worker boundary.

use sola_bus::topics::{MailConfig, MailRule};

/// Credentials + rules used by the protocol layer.
#[derive(Debug, Clone)]
pub struct Account {
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
    pub password: String,
    pub rules: Vec<MailRule>,
}

impl Account {
    pub fn from_config(cfg: &MailConfig) -> Self {
        Self {
            email: cfg.email.clone(),
            imap_host: cfg.imap_host.clone(),
            imap_port: cfg.imap_port,
            smtp_host: cfg.smtp_host.clone(),
            smtp_port: cfg.smtp_port,
            username: cfg.username.clone(),
            password: cfg.password.0.clone(),
            rules: cfg.rules.clone(),
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.imap_host.is_empty() && !self.username.is_empty() && !self.password.is_empty()
    }
}
