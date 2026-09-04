//! Map each account's IMAP mailboxes onto Sola's six canonical boxes.
//!
//! The UI never lists Gmail labels / `[Gmail]/All Mail` / Starred. It
//! shows Inbox, Sent, Drafts, Archive, Junk, Trash — combined across
//! accounts. Moves still hit the *account's* mapped mailbox.

/// Canonical mailbox names (IMAP INBOX stays `INBOX` on the wire).
pub const CANONICAL: &[&str] = &["INBOX", "Sent", "Drafts", "Archive", "Junk", "Trash"];

pub fn is_canonical(name: &str) -> bool {
    CANONICAL.iter().any(|c| name.eq_ignore_ascii_case(c))
}

/// One LIST row: name + attribute tokens (`\\Trash`, `\\Noselect`, …).
#[derive(Debug, Clone)]
pub struct ListedMailbox {
    pub name: String,
    pub attrs: Vec<String>,
}

/// Canonical name → remote mailbox name for one account.
pub type MailboxMap = std::collections::HashMap<String, String>;

/// Pick a remote mailbox for each canonical box. SPECIAL-USE wins,
/// then well-known names (including Gmail's `[Gmail]/…` set). Unmapped
/// boxes are omitted (Archive on a host that has none).
pub fn map_mailboxes(listed: &[ListedMailbox]) -> MailboxMap {
    let mut ranked: Vec<(u8, String, String)> = Vec::new();
    for mb in listed {
        if mb.attrs.iter().any(|a| eq_attr(a, "\\Noselect")) {
            continue;
        }
        if let Some(canon) = classify(&mb.name, &mb.attrs) {
            ranked.push((rank(&mb.name, &mb.attrs), canon, mb.name.clone()));
        }
    }
    ranked.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    let mut out = MailboxMap::new();
    for (_, canon, remote) in ranked {
        out.entry(canon).or_insert(remote);
    }
    out
}

pub fn remote<'a>(map: &'a MailboxMap, canonical: &str) -> Option<&'a str> {
    let key = canonical_key(canonical)?;
    map.get(key).map(String::as_str)
}

pub fn is_gmail_host(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h.contains("gmail.com") || h.contains("googlemail.com")
}

pub fn is_gmail_listing(listed: &[ListedMailbox]) -> bool {
    listed.iter().any(|m| {
        let n = m.name.to_ascii_lowercase();
        n == "[gmail]" || n.starts_with("[gmail]/")
    })
}

/// Gmail has no real Archive folder (`[Gmail]/All Mail` is hidden). Create
/// a user label named Archive when the account has none.
pub fn should_create_archive(map: &MailboxMap, imap_host: &str, listed: &[ListedMailbox]) -> bool {
    !map.contains_key("Archive") && (is_gmail_host(imap_host) || is_gmail_listing(listed))
}

fn canonical_key(name: &str) -> Option<&'static str> {
    CANONICAL
        .iter()
        .copied()
        .find(|c| name.eq_ignore_ascii_case(c))
}

fn classify(name: &str, attrs: &[String]) -> Option<String> {
    if skip_name(name) {
        return None;
    }
    for a in attrs {
        if eq_attr(a, "\\All") || eq_attr(a, "\\Flagged") {
            return None;
        }
        if eq_attr(a, "\\Inbox")
            || eq_attr(a, "\\Sent")
            || eq_attr(a, "\\Drafts")
            || eq_attr(a, "\\Trash")
            || eq_attr(a, "\\Junk")
            || eq_attr(a, "\\Archive")
        {
            return Some(special_use_canonical(a).to_string());
        }
    }
    classify_name(name)
}

fn special_use_canonical(attr: &str) -> &'static str {
    let a = attr.trim();
    if eq_attr(a, "\\Inbox") {
        "INBOX"
    } else if eq_attr(a, "\\Sent") {
        "Sent"
    } else if eq_attr(a, "\\Drafts") {
        "Drafts"
    } else if eq_attr(a, "\\Trash") {
        "Trash"
    } else if eq_attr(a, "\\Junk") {
        "Junk"
    } else {
        "Archive"
    }
}

fn classify_name(name: &str) -> Option<String> {
    let n = name.trim();
    let base = n.rsplit(['/', '.']).next().unwrap_or(n);
    let key = base.replace(['-', '_'], " ");
    let key = key.to_ascii_lowercase();
    let whole = n.to_ascii_lowercase();

    if n.eq_ignore_ascii_case("INBOX") {
        return Some("INBOX".into());
    }
    if is_sent(&key, &whole) {
        return Some("Sent".into());
    }
    if is_drafts(&key) {
        return Some("Drafts".into());
    }
    if is_junk(&key) {
        return Some("Junk".into());
    }
    if is_trash(&key) {
        return Some("Trash".into());
    }
    if is_archive(&key, &whole) {
        return Some("Archive".into());
    }
    None
}

fn skip_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "[gmail]"
        || n.ends_with("/all mail")
        || n.ends_with("/starred")
        || n.ends_with("/important")
        || n == "all mail"
        || n == "starred"
        || n == "important"
}

fn is_sent(base: &str, whole: &str) -> bool {
    matches!(base, "sent" | "sent mail" | "sent items" | "sent messages")
        || whole.ends_with("/sent mail")
}

fn is_drafts(base: &str) -> bool {
    matches!(base, "drafts" | "draft")
}

fn is_junk(base: &str) -> bool {
    matches!(
        base,
        "junk" | "spam" | "junk e mail" | "junk email" | "bulk mail"
    )
}

fn is_trash(base: &str) -> bool {
    matches!(
        base,
        "trash" | "deleted" | "deleted items" | "deleted messages" | "bin"
    )
}

fn is_archive(base: &str, whole: &str) -> bool {
    matches!(base, "archive" | "archives") && !whole.contains("all mail")
}

fn rank(name: &str, attrs: &[String]) -> u8 {
    if attrs.iter().any(|a| {
        eq_attr(a, "\\Inbox")
            || eq_attr(a, "\\Sent")
            || eq_attr(a, "\\Drafts")
            || eq_attr(a, "\\Trash")
            || eq_attr(a, "\\Junk")
            || eq_attr(a, "\\Archive")
    }) {
        0
    } else if name.eq_ignore_ascii_case("INBOX")
        || name.eq_ignore_ascii_case("Sent")
        || name.eq_ignore_ascii_case("Drafts")
        || name.eq_ignore_ascii_case("Archive")
        || name.eq_ignore_ascii_case("Junk")
        || name.eq_ignore_ascii_case("Trash")
    {
        1
    } else {
        2
    }
}

fn eq_attr(attr: &str, want: &str) -> bool {
    attr.trim().eq_ignore_ascii_case(want)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mb(name: &str, attrs: &[&str]) -> ListedMailbox {
        ListedMailbox {
            name: name.into(),
            attrs: attrs.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    #[test]
    fn gmail_maps_six_hides_weird() {
        let listed = vec![
            mb("INBOX", &[]),
            mb("[Gmail]", &["\\Noselect"]),
            mb("[Gmail]/All Mail", &["\\All"]),
            mb("[Gmail]/Sent Mail", &["\\Sent"]),
            mb("[Gmail]/Drafts", &["\\Drafts"]),
            mb("[Gmail]/Spam", &["\\Junk"]),
            mb("[Gmail]/Trash", &["\\Trash"]),
            mb("[Gmail]/Starred", &["\\Flagged"]),
            mb("[Gmail]/Important", &[]),
            mb("Receipts", &[]),
        ];
        let map = map_mailboxes(&listed);
        assert_eq!(map.get("INBOX").map(String::as_str), Some("INBOX"));
        assert_eq!(
            map.get("Sent").map(String::as_str),
            Some("[Gmail]/Sent Mail")
        );
        assert_eq!(
            map.get("Drafts").map(String::as_str),
            Some("[Gmail]/Drafts")
        );
        assert_eq!(map.get("Junk").map(String::as_str), Some("[Gmail]/Spam"));
        assert_eq!(map.get("Trash").map(String::as_str), Some("[Gmail]/Trash"));
        assert!(!map.contains_key("Archive"));
        assert_eq!(map.len(), 5);
    }

    #[test]
    fn gmail_user_archive_label_maps() {
        let listed = vec![
            mb("INBOX", &[]),
            mb("[Gmail]", &["\\Noselect"]),
            mb("[Gmail]/All Mail", &["\\All"]),
            mb("[Gmail]/Sent Mail", &["\\Sent"]),
            mb("[Gmail]/Drafts", &["\\Drafts"]),
            mb("[Gmail]/Spam", &["\\Junk"]),
            mb("[Gmail]/Trash", &["\\Trash"]),
            mb("Archive", &[]),
        ];
        let map = map_mailboxes(&listed);
        assert_eq!(map.get("Archive").map(String::as_str), Some("Archive"));
        assert_eq!(map.len(), 6);
        assert!(!should_create_archive(&map, "imap.gmail.com", &listed));
    }

    #[test]
    fn wicket_style_names() {
        let listed = vec![
            mb("INBOX", &[]),
            mb("Sent", &[]),
            mb("Drafts", &[]),
            mb("Archive", &[]),
            mb("Junk", &[]),
            mb("Trash", &[]),
            mb("Projects", &[]),
        ];
        let map = map_mailboxes(&listed);
        assert_eq!(map.get("Archive").map(String::as_str), Some("Archive"));
        assert_eq!(map.len(), 6);
    }

    #[test]
    fn special_use_beats_gmail_all_mail() {
        let listed = vec![
            mb("Archive", &["\\Archive"]),
            mb("[Gmail]/All Mail", &["\\All"]),
        ];
        let map = map_mailboxes(&listed);
        assert_eq!(map.get("Archive").map(String::as_str), Some("Archive"));
    }

    #[test]
    fn gmail_without_archive_should_create() {
        let listed = vec![
            mb("INBOX", &[]),
            mb("[Gmail]", &["\\Noselect"]),
            mb("[Gmail]/All Mail", &["\\All"]),
            mb("[Gmail]/Trash", &["\\Trash"]),
        ];
        let map = map_mailboxes(&listed);
        assert!(should_create_archive(&map, "imap.gmail.com", &listed));
        assert!(should_create_archive(&map, "mail.example.com", &listed));
    }

    #[test]
    fn wicket_does_not_invent_archive() {
        let listed = vec![mb("INBOX", &[]), mb("Trash", &[])];
        let map = map_mailboxes(&listed);
        assert!(!should_create_archive(&map, "imap.example.com", &listed));
    }

    #[test]
    fn existing_archive_is_left_alone() {
        let listed = vec![
            mb("INBOX", &[]),
            mb("Archive", &[]),
            mb("[Gmail]/Trash", &["\\Trash"]),
        ];
        let map = map_mailboxes(&listed);
        assert!(!should_create_archive(&map, "imap.gmail.com", &listed));
    }
}
