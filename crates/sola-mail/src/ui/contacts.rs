//! Addresses seen in mail, for To / Cc suggestions.

use std::collections::HashMap;

use sola_bus::topics::mail_addr_key;

use super::format::split_address;
use crate::protocol::recipients;

/// Plenty for suggestions; keeps a pathological inbox from growing it.
const MAX: usize = 5000;
/// Tokens shorter than this show no suggestions.
const MIN_QUERY: usize = 2;

#[derive(Default)]
pub struct Contacts {
    /// address key → display form (`Name <addr>` when a name was seen).
    by_key: HashMap<String, String>,
}

impl Contacts {
    pub fn add(&mut self, raw: &str) {
        let key = mail_addr_key(raw);
        if !key.contains('@') || is_robot(&key) {
            return;
        }
        let named = split_address(raw).1.is_some();
        match self.by_key.get(&key) {
            // Keep a display name once we have one.
            Some(_) if !named => {}
            _ if self.by_key.len() >= MAX && !self.by_key.contains_key(&key) => {}
            _ => {
                self.by_key.insert(key, raw.trim().to_string());
            }
        }
    }

    /// Up to `limit` matches for the part of `field` being typed, minus
    /// addresses already in the field. Prefix matches (address or a word
    /// of the name) rank above substring matches.
    pub fn suggest(&self, field: &str, limit: usize) -> Vec<String> {
        let typed = recipients::current_token(field);
        let token = typed.trim().to_lowercase();
        if token.chars().count() < MIN_QUERY {
            return Vec::new();
        }
        let head = &field[..field.len() - typed.len()];
        let present: Vec<String> = recipients::split(head)
            .into_iter()
            .map(mail_addr_key)
            .collect();
        let mut hits: Vec<(u8, &String)> = self
            .by_key
            .iter()
            // Already in the field, or already typed out in full.
            .filter(|(key, _)| !present.contains(key) && **key != token)
            .filter_map(|(key, display)| {
                let name = split_address(display).0.to_lowercase();
                if key.starts_with(&token) || name.split_whitespace().any(|w| w.starts_with(&token))
                {
                    Some((0, display))
                } else if key.contains(&token) || name.contains(&token) {
                    Some((1, display))
                } else {
                    None
                }
            })
            .collect();
        hits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
        hits.into_iter()
            .take(limit)
            .map(|(_, d)| d.clone())
            .collect()
    }
}

/// Bulk senders nobody writes back to.
fn is_robot(key: &str) -> bool {
    let local = key.split('@').next().unwrap_or("");
    [
        "noreply",
        "no-reply",
        "donotreply",
        "do-not-reply",
        "mailer-daemon",
        "bounce",
    ]
    .iter()
    .any(|r| local.contains(r))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> Contacts {
        let mut c = Contacts::default();
        for a in [
            "ana@x.com",
            "Ana Lopez <ana@x.com>",
            "Bo Andersen <bo@y.org>",
            "noreply@steampowered.com",
            "dana@z.net",
        ] {
            c.add(a);
        }
        c
    }

    #[test]
    fn keeps_names_and_skips_robots() {
        let c = book();
        assert_eq!(
            c.by_key.get("ana@x.com").map(String::as_str),
            Some("Ana Lopez <ana@x.com>")
        );
        assert!(!c.by_key.contains_key("noreply@steampowered.com"));
    }

    #[test]
    fn prefix_ranks_first_and_present_are_skipped() {
        let c = book();
        // "an": Ana (address prefix), Bo Andersen (name word prefix), dana (contains).
        assert_eq!(
            c.suggest("an", 5),
            vec![
                "Ana Lopez <ana@x.com>",
                "Bo Andersen <bo@y.org>",
                "dana@z.net"
            ]
        );
        assert_eq!(
            c.suggest("Ana Lopez <ana@x.com>, an", 5),
            vec!["Bo Andersen <bo@y.org>", "dana@z.net"]
        );
        assert!(c.suggest("a", 5).is_empty());
    }
}
