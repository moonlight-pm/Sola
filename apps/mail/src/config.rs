pub use sola_core::mail::MailConfig;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_config() {
        let content = r#"{
            "email": "user@example.com",
            "imap_host": "mail.example.com",
            "imap_port": 993,
            "smtp_host": "mail.example.com",
            "smtp_port": 587,
            "username": "user@example.com",
            "password": "secret"
        }"#;
        let config: MailConfig = serde_json::from_str(content).unwrap();
        assert_eq!(config.email, "user@example.com");
        assert_eq!(config.imap_host, "mail.example.com");
        assert_eq!(config.imap_port, 993);
        assert_eq!(config.smtp_port, 587);
        assert_eq!(config.password, "secret");
        assert!(config.rules.is_empty());
    }

    #[test]
    fn parse_partial_config_fills_defaults() {
        let content = r#"{ "email": "user@example.com" }"#;
        let config: MailConfig = serde_json::from_str(content).unwrap();
        assert_eq!(config.email, "user@example.com");
        assert_eq!(config.imap_port, 993);
        assert_eq!(config.smtp_port, 587);
        assert_eq!(config.imap_host, "");
    }

    #[test]
    fn parse_empty_object_uses_defaults() {
        let config: MailConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config.imap_port, 993);
        assert_eq!(config.smtp_port, 587);
        assert!(config.rules.is_empty());
    }

    #[test]
    fn parse_ignores_unknown_fields() {
        let content = r#"{
            "email": "user@example.com",
            "wicket": { "server": "niarada.co", "pat": "tok" }
        }"#;
        let config: MailConfig = serde_json::from_str(content).unwrap();
        assert_eq!(config.email, "user@example.com");
    }

    #[test]
    fn parse_rules() {
        let content = r#"{
            "email": "user@example.com",
            "rules": [
                {
                    "name": "GitHub",
                    "action": "smart_mailbox",
                    "conditions": [
                        { "field": "from", "match": "domain", "value": "github.com" }
                    ]
                },
                {
                    "name": "Move newsletters",
                    "action": "move",
                    "dest": "Newsletters",
                    "conditions": [
                        { "field": "from", "match": "contains", "value": "newsletter" }
                    ]
                }
            ]
        }"#;
        let config: MailConfig = serde_json::from_str(content).unwrap();
        assert_eq!(config.rules.len(), 2);
        assert_eq!(config.rules[0].name, "GitHub");
        assert_eq!(config.rules[0].conditions[0].field, "from");
        assert_eq!(config.rules[0].conditions[0].match_type, "domain");
        assert_eq!(config.rules[1].dest.as_deref(), Some("Newsletters"));
    }
}
