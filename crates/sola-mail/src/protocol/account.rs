//! Local account view over bus [`MailConfig`].
//!
//! Protocol code (IMAP/SMTP/IDLE) needs a plain password string and does
//! not depend on bus encryption. Construct once at the worker boundary.

use sola_bus::topics::{MailAccount, MailConfig, MailRule, mail_addr_key};

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
    pub aliases: Vec<String>,
    pub imap_enabled: bool,
    pub smtp_enabled: bool,
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
            aliases: cfg.aliases.clone(),
            imap_enabled: cfg.imap_enabled,
            smtp_enabled: cfg.smtp_enabled,
            rules: cfg.rules.clone(),
        }
    }

    pub fn from_extra(acc: &MailAccount, rules: &[MailRule]) -> Self {
        Self {
            email: acc.email.clone(),
            imap_host: acc.imap_host.clone(),
            imap_port: acc.imap_port,
            smtp_host: acc.smtp_host.clone(),
            smtp_port: acc.smtp_port,
            username: acc.username.clone(),
            password: acc.password.0.clone(),
            aliases: acc.aliases.clone(),
            imap_enabled: acc.imap_enabled,
            smtp_enabled: acc.smtp_enabled,
            rules: rules.to_vec(),
        }
    }

    pub fn id(&self) -> String {
        let key = sola_bus::topics::mail_addr_key(&self.email);
        if !key.is_empty() {
            key
        } else {
            self.username.to_ascii_lowercase()
        }
    }

    /// IMAP-enabled accounts (inbox first, then extras). Deduped by id.
    pub fn imap_accounts(cfg: &MailConfig) -> Vec<Self> {
        let mut out = Vec::new();
        let inbox = Self::from_config(cfg);
        if inbox.is_configured() {
            out.push(inbox);
        }
        for extra in &cfg.accounts {
            let a = Self::from_extra(extra, &cfg.rules);
            if a.is_configured() && !out.iter().any(|o| o.id() == a.id()) {
                out.push(a);
            }
        }
        out
    }

    /// Inbox + extra SMTP identities. Extra accounts are first so a
    /// Gmail From uses Gmail SMTP even if the address is also listed
    /// as an inbox alias.
    pub fn senders_from_config(cfg: &MailConfig) -> Vec<Self> {
        let mut v: Vec<Self> = cfg
            .accounts
            .iter()
            .map(|a| Self::from_extra(a, &cfg.rules))
            .collect();
        v.push(Self::from_config(cfg));
        v
    }

    pub fn is_configured(&self) -> bool {
        self.imap_enabled
            && !self.imap_host.is_empty()
            && !self.username.is_empty()
            && !self.password.is_empty()
    }

    pub fn can_send(&self) -> bool {
        self.smtp_enabled
            && !self.smtp_host.is_empty()
            && !self.username.is_empty()
            && !self.password.is_empty()
    }

    pub fn owns_from(&self, from: &str) -> bool {
        let key = mail_addr_key(from);
        if key.is_empty() {
            return false;
        }
        mail_addr_key(&self.email) == key || self.aliases.iter().any(|a| mail_addr_key(a) == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_core::Encrypted;

    #[test]
    fn smtp_for_prefers_extra_account() {
        let cfg = MailConfig {
            email: "josh@wicket.example".into(),
            smtp_host: "mail.wicket.example".into(),
            smtp_port: 587,
            username: "josh".into(),
            password: Encrypted("inbox".into()),
            aliases: vec!["hello@wicket.example".into()],
            accounts: vec![MailAccount {
                email: "me@gmail.com".into(),
                smtp_host: "smtp.gmail.com".into(),
                smtp_port: 587,
                username: "me@gmail.com".into(),
                password: Encrypted("app-pass".into()),
                ..MailAccount::default()
            }],
            ..MailConfig::default()
        };
        let gmail = smtp_for(&cfg, "Me <me@gmail.com>");
        assert_eq!(gmail.smtp_host, "smtp.gmail.com");
        assert_eq!(gmail.password, "app-pass");
        let alias = smtp_for(&cfg, "hello@wicket.example");
        assert_eq!(alias.smtp_host, "mail.wicket.example");
        let unknown = smtp_for(&cfg, "stranger@example.com");
        assert_eq!(unknown.smtp_host, "mail.wicket.example");
    }
}

/// SMTP account for this From: extra accounts first, then inbox.
pub fn smtp_for(cfg: &MailConfig, from: &str) -> Account {
    Account::senders_from_config(cfg)
        .into_iter()
        .find(|a| a.owns_from(from) && a.can_send())
        .unwrap_or_else(|| Account::from_config(cfg))
}
