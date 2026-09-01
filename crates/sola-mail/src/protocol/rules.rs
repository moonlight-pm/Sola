//! Rule matching for move rules and smart mailboxes.

use sola_bus::topics::MailRule;

/// Address inside `Name <user@host>` (or the trimmed field if there is
/// no angle-bracket form). IMAP envelopes almost always carry a display
/// name, so matchers that only compare the raw From/To string miss
/// `no-reply@example.com` against `Bot <no-reply@example.com>`.
pub fn extract_address(field_value: &str) -> &str {
    if let Some(start) = field_value.find('<') {
        let after = &field_value[start + 1..];
        if let Some(end) = after.find('>') {
            return after[..end].trim();
        }
    }
    field_value.trim()
}

fn is_addr_field(field: &str) -> bool {
    matches!(field, "from" | "to")
}

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
        let value = cond.value.trim().to_lowercase();
        if value.is_empty() {
            return false;
        }
        match cond.match_type.as_str() {
            "domain" => {
                let addr = if is_addr_field(&cond.field) {
                    extract_address(field_value)
                } else {
                    field_value
                };
                if let Some(at_pos) = addr.rfind('@') {
                    addr[at_pos + 1..].trim().to_lowercase() == value
                } else {
                    false
                }
            }
            "address" => extract_address(field_value).to_lowercase() == value,
            "contains" => field_value.to_lowercase().contains(&value),
            "equals" => {
                let fv = field_value.trim().to_lowercase();
                if fv == value {
                    return true;
                }
                // From/To "is" an address: treat display-name envelopes as
                // the address, so a saved `equals no-reply@x.com` still
                // matches `List <no-reply@x.com>`.
                is_addr_field(&cond.field) && extract_address(field_value).to_lowercase() == value
            }
            _ => false,
        }
    })
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

    #[test]
    fn equals_on_from_matches_display_name_envelope() {
        // Settings "is" + a bare address used to fail against IMAP
        // `Name <addr>` From lines (the live Illuno spam rule).
        let rule = rule("from", "equals", "no-reply@illuno.com");
        assert!(rule_matches(
            &rule,
            "Illuno <no-reply@illuno.com>",
            "weekly digest",
            ""
        ));
        assert!(rule_matches(&rule, "no-reply@illuno.com", "", ""));
        assert!(!rule_matches(&rule, "other@illuno.com", "", ""));
        assert!(!rule_matches(
            &rule,
            "Someone <someone@elsewhere.com>",
            "",
            ""
        ));
    }

    #[test]
    fn extract_address_from_angle_brackets() {
        assert_eq!(extract_address("Bot <bot@github.com>"), "bot@github.com");
        assert_eq!(extract_address("a@b.com"), "a@b.com");
        assert_eq!(extract_address("  a@b.com  "), "a@b.com");
    }

    #[test]
    fn empty_condition_value_never_matches() {
        let rule = rule("from", "equals", "  ");
        assert!(!rule_matches(&rule, "a@b.com", "", ""));
    }
}
