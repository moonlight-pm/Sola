//! From-address pick for compose / reply.

use sola_bus::topics::mail_addr_key;

/// Identity that appears in the original To/Cc, else `None`.
pub fn pick_from_for_reply(identities: &[String], to: &[String], cc: &[String]) -> Option<String> {
    let recipients: Vec<String> = to
        .iter()
        .chain(cc)
        .map(|r| mail_addr_key(r))
        .filter(|k| !k.is_empty())
        .collect();
    identities
        .iter()
        .find(|id| recipients.contains(&mail_addr_key(id)))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> Vec<String> {
        vec![
            "josh@wicket.example".into(),
            "hello@wicket.example".into(),
            "me@gmail.com".into(),
        ]
    }

    #[test]
    fn reply_picks_gmail_from_original_to() {
        let picked = pick_from_for_reply(&ids(), &["Me <me@gmail.com>".into()], &[]);
        assert_eq!(picked.as_deref(), Some("me@gmail.com"));
    }

    #[test]
    fn reply_checks_cc_too() {
        let picked = pick_from_for_reply(
            &ids(),
            &["someone@else.com".into()],
            &["Hello <hello@wicket.example>".into()],
        );
        assert_eq!(picked.as_deref(), Some("hello@wicket.example"));
    }

    #[test]
    fn reply_falls_back_when_to_is_inbox_only() {
        assert!(pick_from_for_reply(&ids(), &["someone@else.com".into()], &[]).is_none());
    }
}
