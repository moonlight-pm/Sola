//! Rule matching for move rules and smart mailboxes.

use sola_bus::topics::MailRule;

/// Returns true if every condition in the rule matches the given message fields.
/// An empty conditions list never matches.
pub fn rule_matches(rule: &MailRule, from: &str, subject: &str, to: &str) -> bool {
    if rule.conditions.is_empty() {
        return false;
    }
    rule.conditions.iter().all(|cond| {
        let field_value = match cond.field.as_str() {
            "from" => from,
            "subject" => subject,
            "to" => to,
            _ => return false,
        };
        let value = cond.value.to_lowercase();
        match cond.match_type.as_str() {
            "domain" => {
                if let Some(at_pos) = field_value.rfind('@') {
                    let after_at = &field_value[at_pos + 1..];
                    let domain = after_at.trim_end_matches('>').to_lowercase();
                    domain == value
                } else {
                    false
                }
            }
            "address" => {
                if let Some(start) = field_value.find('<') {
                    let after = &field_value[start + 1..];
                    if let Some(end) = after.find('>') {
                        after[..end].trim().to_lowercase() == value
                    } else {
                        false
                    }
                } else {
                    field_value.trim().to_lowercase() == value
                }
            }
            "contains" => field_value.to_lowercase().contains(&value),
            "equals" => field_value.to_lowercase() == value,
            _ => false,
        }
    })
}

/// Smart mailbox display names from config rules.
pub fn smart_mailbox_names(rules: &[MailRule]) -> Vec<String> {
    rules
        .iter()
        .filter(|r| r.action == "smart_mailbox")
        .map(|r| r.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_bus::topics::MailRuleCondition;

    fn rule(field: &str, match_type: &str, value: &str) -> MailRule {
        MailRule {
            name: "t".into(),
            action: "smart_mailbox".into(),
            dest: None,
            conditions: vec![MailRuleCondition {
                field: field.into(),
                match_type: match_type.into(),
                value: value.into(),
            }],
        }
    }

    #[test]
    fn domain_match_matches_any_address_in_domain() {
        let rule = rule("from", "domain", "github.com");
        assert!(rule_matches(&rule, "noreply@github.com", "", ""));
        assert!(rule_matches(&rule, "Bot <bot@github.com>", "", ""));
        assert!(!rule_matches(&rule, "someone@example.com", "", ""));
    }

    #[test]
    fn address_match_requires_exact_address() {
        let rule = rule("from", "address", "a@b.com");
        assert!(rule_matches(&rule, "a@b.com", "", ""));
        assert!(rule_matches(&rule, "A <a@b.com>", "", ""));
        assert!(!rule_matches(&rule, "a@b.co", "", ""));
    }

    #[test]
    fn contains_match_substring_case_insensitive() {
        let rule = rule("subject", "contains", "invoice");
        assert!(rule_matches(&rule, "", "Your INVOICE #1", ""));
        assert!(!rule_matches(&rule, "", "Receipt", ""));
    }

    #[test]
    fn equals_match_full_string() {
        let rule = rule("subject", "equals", "ping");
        assert!(rule_matches(&rule, "", "ping", ""));
        assert!(!rule_matches(&rule, "", "ping!", ""));
    }

    #[test]
    fn all_conditions_must_match() {
        let mut r = rule("from", "domain", "example.com");
        r.conditions.push(MailRuleCondition {
            field: "subject".into(),
            match_type: "contains".into(),
            value: "alert".into(),
        });
        assert!(rule_matches(&r, "x@example.com", "alert: down", ""));
        assert!(!rule_matches(&r, "x@example.com", "news", ""));
    }
}
