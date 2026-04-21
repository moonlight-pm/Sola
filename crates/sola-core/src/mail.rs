//! Mail config shape shared by `sola-mail` (producer) and `sola-settings`
//! (editor). The `JsonConfig` impl for `MailConfig` lives in `sola-app`,
//! since that trait is defined there.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailRule {
    pub name: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dest: Option<String>,
    pub conditions: Vec<MailRuleCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailRuleCondition {
    pub field: String,
    #[serde(rename = "match")]
    pub match_type: String,
    pub value: String,
}
