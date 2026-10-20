//! Rule matching for move rules and smart mailboxes.

use sola_bus::topics::{MailRule, MailRuleCondition};

const MOVE: &str = "move";
const SMART: &str = "smart_mailbox";

/// A `move` rule with a non-empty destination.
pub fn move_dest(rule: &MailRule) -> Option<&str> {
    if rule.action != MOVE {
        return None;
    }
    rule.dest.as_deref().map(str::trim).filter(|d| !d.is_empty())
}

pub fn is_smart(rule: &MailRule) -> bool {
    rule.action == SMART
}

/// Address inside `Name <user@host>` (or the trimmed field if there is
/// no angle-bracket form). Matchers that only compare the raw From/To
/// string miss `no-reply@example.com` against `Bot <no-reply@example.com>`.
pub fn extract_address(field_value: &str) -> &str {
    if let Some(start) = field_value.find('<') {
        let after = &field_value[start + 1..];
        if let Some(end) = after.find('>') {
            return after[..end].trim();
        }
    }
    field_value.trim()
}

/// Returns true if every condition in the rule matches the given message
/// fields. A `to` condition matches when any recipient does. An empty
/// conditions list never matches.
pub fn rule_matches(rule: &MailRule, from: &str, subject: &str, to: &[String]) -> bool {
    if rule.conditions.is_empty() {
        return false;
    }
    rule.conditions.iter().all(|cond| match cond.field.as_str() {
        "from" => condition_matches(cond, from, true),
        "subject" => condition_matches(cond, subject, false),
        "to" => to.iter().any(|r| condition_matches(cond, r, true)),
        _ => false,
    })
}

fn condition_matches(cond: &MailRuleCondition, field_value: &str, is_addr: bool) -> bool {
    let value = cond.value.trim().to_lowercase();
    if value.is_empty() {
        return false;
    }
    match cond.match_type.as_str() {
        "domain" => {
            let addr = if is_addr {
                extract_address(field_value)
            } else {
                field_value
            };
            addr.rfind('@')
                .is_some_and(|at| addr[at + 1..].trim().to_lowercase() == value)
        }
        "address" => extract_address(field_value).to_lowercase() == value,
        "contains" => field_value.to_lowercase().contains(&value),
        // From/To "is" an address: treat display-name envelopes as the
        // address, so a saved `equals no-reply@x.com` still matches
        // `List <no-reply@x.com>`.
        "equals" => {
            field_value.trim().to_lowercase() == value
                || (is_addr && extract_address(field_value).to_lowercase() == value)
        }
        _ => false,
    }
}

/// IMAP SEARCH criteria for a smart mailbox (conditions are ANDed).
///
/// IMAP SEARCH is substring matching on header fields, so `domain` /
/// `address` are approximated — counts may slightly overcount compared to
/// [`rule_matches`]. No `UNDELETED` prefix: some servers mishandle
/// combined criteria and desync the imap crate's command tags; `\Deleted`
/// rows are dropped when the summaries are parsed.
pub fn imap_search(conditions: &[MailRuleCondition]) -> String {
    conditions
        .iter()
        .filter_map(|c| {
            let safe = c.value.replace('\\', "\\\\").replace('"', "\\\"");
            let key = match c.field.as_str() {
                "from" => "FROM",
                "to" => "TO",
                "subject" => "SUBJECT",
                _ => return None,
            };
            Some(format!("{key} \"{safe}\""))
        })
        .collect::<Vec<_>>()
        .join(" ")
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
        assert!(rule_matches(&rule, "noreply@github.com", "", &[]));
        assert!(rule_matches(&rule, "Bot <bot@github.com>", "", &[]));
        assert!(!rule_matches(&rule, "someone@example.com", "", &[]));
    }

    #[test]
    fn address_match_requires_exact_address() {
        let rule = rule("from", "address", "a@b.com");
        assert!(rule_matches(&rule, "a@b.com", "", &[]));
        assert!(rule_matches(&rule, "A <a@b.com>", "", &[]));
        assert!(!rule_matches(&rule, "a@b.co", "", &[]));
    }

    #[test]
    fn contains_match_substring_case_insensitive() {
        let rule = rule("subject", "contains", "invoice");
        assert!(rule_matches(&rule, "", "Your INVOICE #1", &[]));
        assert!(!rule_matches(&rule, "", "Receipt", &[]));
    }

    #[test]
    fn equals_match_full_string() {
        let rule = rule("subject", "equals", "ping");
        assert!(rule_matches(&rule, "", "ping", &[]));
        assert!(!rule_matches(&rule, "", "ping!", &[]));
    }

    #[test]
    fn all_conditions_must_match() {
        let mut r = rule("from", "domain", "example.com");
        r.conditions.push(MailRuleCondition {
            field: "subject".into(),
            match_type: "contains".into(),
            value: "alert".into(),
        });
        assert!(rule_matches(&r, "x@example.com", "alert: down", &[]));
        assert!(!rule_matches(&r, "x@example.com", "news", &[]));
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
            &[]
        ));
        assert!(rule_matches(&rule, "no-reply@illuno.com", "", &[]));
        assert!(!rule_matches(&rule, "other@illuno.com", "", &[]));
        assert!(!rule_matches(
            &rule,
            "Someone <someone@elsewhere.com>",
            "",
            &[]
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
        assert!(!rule_matches(&rule, "a@b.com", "", &[]));
    }

    #[test]
    fn to_condition_matches_any_recipient() {
        let rule = rule("to", "address", "list@example.com");
        let to = vec!["Me <me@x.com>".to_string(), "List <list@example.com>".into()];
        assert!(rule_matches(&rule, "", "", &to));
        assert!(!rule_matches(&rule, "", "", &to[..1]));
    }

    #[test]
    fn imap_search_quotes_and_ands() {
        let mut r = rule("from", "contains", r#"a"b"#);
        r.conditions.push(MailRuleCondition {
            field: "to".into(),
            match_type: "contains".into(),
            value: "list".into(),
        });
        assert_eq!(imap_search(&r.conditions), r#"FROM "a\"b" TO "list""#);
    }
}
