//! From-address pick for compose / reply.

use sola_bus::topics::mail_addr_key;

use super::rules::extract_address;

/// Merge `extra` into `base` (case-insensitive, first spelling wins).
pub fn merge_from_addresses(base: &[String], extra: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |raw: &str| {
        let t = raw.trim();
        if t.is_empty() || sola_bus::topics::is_catchall_addr(t) {
            return;
        }
        let key = mail_addr_key(t);
        if !out.iter().any(|e| mail_addr_key(e) == key) {
            out.push(t.to_string());
        }
    };
    for a in base {
        push(a);
    }
    for a in extra {
        push(a);
    }
    out
}

/// Identity that appears in the original To/Cc, else `None`.
pub fn pick_from_for_reply(identities: &[String], to: &str, cc: &str) -> Option<String> {
    let mut recipients: Vec<String> = Vec::new();
    for part in to.split(',').chain(cc.split(',')) {
        let key = mail_addr_key(extract_address(part));
        if !key.is_empty() {
            recipients.push(key);
        }
    }
    identities
        .iter()
        .find(|id| recipients.iter().any(|r| mail_addr_key(id) == *r))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_picks_gmail_from_original_to() {
        let ids = vec![
            "josh@wicket.example".into(),
            "hello@wicket.example".into(),
            "me@gmail.com".into(),
        ];
        let picked = pick_from_for_reply(&ids, "Me <me@gmail.com>", "");
        assert_eq!(picked.as_deref(), Some("me@gmail.com"));
    }

    #[test]
    fn reply_falls_back_when_to_is_inbox_only() {
        let ids = vec!["josh@wicket.example".into(), "me@gmail.com".into()];
        assert!(pick_from_for_reply(&ids, "someone@else.com", "").is_none());
    }

    #[test]
    fn merge_keeps_primary_spelling() {
        let merged = merge_from_addresses(
            &["Josh@Wicket.example".into()],
            &["josh@wicket.example".into(), "hello@wicket.example".into()],
        );
        assert_eq!(merged, vec!["Josh@Wicket.example", "hello@wicket.example"]);
    }

    #[test]
    fn merge_drops_catchall() {
        let merged = merge_from_addresses(
            &["josh@niarada.co".into()],
            &["*@moonlight.pm".into(), "hello@niarada.co".into()],
        );
        assert_eq!(merged, vec!["josh@niarada.co", "hello@niarada.co"]);
    }
}
