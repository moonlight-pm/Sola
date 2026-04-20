use std::path::PathBuf;

use serde::Deserialize;

use crate::rules::{MailRule, MailRuleCondition};

#[derive(Debug, Clone, serde::Serialize)]
pub struct MailConfig {
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
    pub password: String,
    pub rules: Vec<MailRule>,
}

#[derive(Deserialize)]
struct TomlConfig {
    account: Option<AccountSection>,
    #[serde(default)]
    rule: Vec<RuleEntry>,
    /// Catch-all for unknown sections (e.g. legacy [wicket]) so they don't cause parse errors.
    #[serde(flatten)]
    _extra: std::collections::HashMap<String, toml::Value>,
}

#[derive(Deserialize)]
struct AccountSection {
    email: Option<String>,
    imap_host: Option<String>,
    imap_port: Option<u16>,
    smtp_host: Option<String>,
    smtp_port: Option<u16>,
    username: Option<String>,
    password: Option<String>,
}

#[derive(Deserialize)]
struct RuleEntry {
    name: String,
    action: String,
    dest: Option<String>,
    #[serde(default)]
    conditions: Vec<ConditionEntry>,
}

#[derive(Deserialize)]
struct ConditionEntry {
    field: String,
    #[serde(rename = "match")]
    match_type: String,
    value: String,
}

impl Default for MailConfig {
    fn default() -> Self {
        Self {
            email: String::new(),
            imap_host: String::new(),
            imap_port: 993,
            smtp_host: String::new(),
            smtp_port: 587,
            username: String::new(),
            password: String::new(),
            rules: Vec::new(),
        }
    }
}

impl MailConfig {
    /// Returns the path to the mail.toml config file.
    pub fn config_path() -> PathBuf {
        let config_dir = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
                PathBuf::from(home).join(".config")
            });
        config_dir.join("sola").join("mail.toml")
    }

    pub fn load() -> anyhow::Result<Self> {
        let path = Self::config_path();
        match std::fs::read_to_string(&path) {
            Ok(content) => Self::parse(&content),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(anyhow::anyhow!("Failed to read {}: {e}", path.display())),
        }
    }

    fn parse(content: &str) -> anyhow::Result<Self> {
        let config: TomlConfig = toml::from_str(content)
            .map_err(|e| anyhow::anyhow!("Failed to parse mail.toml: {e}"))?;

        let acct = config.account.unwrap_or(AccountSection {
            email: None,
            imap_host: None,
            imap_port: None,
            smtp_host: None,
            smtp_port: None,
            username: None,
            password: None,
        });

        let imap_host = acct.imap_host.unwrap_or_default();
        let username = acct.username.unwrap_or_default();
        let password = acct.password.unwrap_or_default();

        // Parse new [[rule]] entries
        let rules: Vec<MailRule> = config
            .rule
            .into_iter()
            .map(|r| MailRule {
                name: r.name,
                action: r.action,
                dest: r.dest,
                conditions: r
                    .conditions
                    .into_iter()
                    .map(|c| MailRuleCondition {
                        field: c.field,
                        match_type: c.match_type,
                        value: c.value,
                    })
                    .collect(),
            })
            .collect();

        Ok(Self {
            email: acct.email.unwrap_or_default(),
            imap_host,
            imap_port: acct.imap_port.unwrap_or(993),
            smtp_host: acct.smtp_host.unwrap_or_default(),
            smtp_port: acct.smtp_port.unwrap_or(587),
            username,
            password,
            rules,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_config() {
        let content = r#"
[account]
email = "user@example.com"
imap_host = "mail.example.com"
imap_port = 993
smtp_host = "mail.example.com"
smtp_port = 587
username = "user@example.com"
password = "secret"
"#;
        let config = MailConfig::parse(content).unwrap();
        assert_eq!(config.email, "user@example.com");
        assert_eq!(config.imap_host, "mail.example.com");
        assert_eq!(config.imap_port, 993);
        assert_eq!(config.smtp_host, "mail.example.com");
        assert_eq!(config.smtp_port, 587);
        assert_eq!(config.username, "user@example.com");
        assert_eq!(config.password, "secret");
        assert!(config.rules.is_empty());
    }

    #[test]
    fn parse_legacy_wicket_config() {
        let content = r#"
[account]
email = "user@example.com"
imap_host = "mail.example.com"
imap_port = 993
smtp_host = "mail.example.com"
smtp_port = 587
username = "user@example.com"
password = "secret"

[wicket]
server = "niarada.co"
pat = "tok_abc123"
"#;
        // Legacy [wicket] section should be silently ignored, not cause an error
        let config = MailConfig::parse(content).unwrap();
        assert_eq!(config.email, "user@example.com");
        assert!(config.rules.is_empty());
    }

    #[test]
    fn parse_partial_config() {
        let content = r#"
[account]
email = "user@example.com"
"#;
        // Partial configs should parse successfully with empty strings for missing fields
        let config = MailConfig::parse(content).unwrap();
        assert_eq!(config.email, "user@example.com");
        assert_eq!(config.imap_host, "");
        assert_eq!(config.username, "");
        assert_eq!(config.password, "");
        assert_eq!(config.imap_port, 993);
        assert_eq!(config.smtp_port, 587);
    }

    #[test]
    fn parse_empty_config() {
        let content = "";
        let config = MailConfig::parse(content).unwrap();
        assert_eq!(config.email, "");
        assert_eq!(config.imap_host, "");
        assert_eq!(config.imap_port, 993);
        assert_eq!(config.smtp_host, "");
        assert_eq!(config.smtp_port, 587);
        assert_eq!(config.username, "");
        assert_eq!(config.password, "");
        assert!(config.rules.is_empty());
    }

    #[test]
    fn parse_rule_config() {
        let content = r#"
[account]
email = "user@example.com"
imap_host = "mail.example.com"
username = "user@example.com"
password = "secret"

[[rule]]
name = "GitHub"
action = "smart_mailbox"
conditions = [
  { field = "from", match = "domain", value = "github.com" }
]

[[rule]]
name = "Move newsletters"
action = "move"
dest = "Newsletters"
conditions = [
  { field = "from", match = "contains", value = "newsletter" }
]
"#;
        let config = MailConfig::parse(content).unwrap();
        assert_eq!(config.rules.len(), 2);
        assert_eq!(config.rules[0].name, "GitHub");
        assert_eq!(config.rules[0].action, "smart_mailbox");
        assert!(config.rules[0].dest.is_none());
        assert_eq!(config.rules[0].conditions.len(), 1);
        assert_eq!(config.rules[0].conditions[0].field, "from");
        assert_eq!(config.rules[0].conditions[0].match_type, "domain");
        assert_eq!(config.rules[0].conditions[0].value, "github.com");
        assert_eq!(config.rules[1].name, "Move newsletters");
        assert_eq!(config.rules[1].action, "move");
        assert_eq!(config.rules[1].dest.as_deref(), Some("Newsletters"));
    }

}
