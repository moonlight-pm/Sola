//! Destination UID after a MOVE/COPY: COPYUID parsing (RFC 4315) and the
//! Message-ID fallback used when the server does not report one.

use imap::types::Fetch;

/// How many newest dest-folder messages to scan when COPYUID is missing.
pub const DEST_UID_SCAN: u32 = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailIdent {
    message_id: Option<String>,
    from: String,
    subject: String,
    date: String,
}

impl MailIdent {
    /// From an `(UID ENVELOPE)` fetch. Raw envelope bytes are fine here:
    /// both sides of the comparison come from the same server.
    pub fn from_fetch(fetch: &Fetch) -> Option<Self> {
        let env = fetch.envelope()?;
        let s = |b: Option<&[u8]>| {
            b.and_then(|b| std::str::from_utf8(b).ok())
                .unwrap_or("")
                .to_string()
        };
        let from = env
            .from
            .as_ref()
            .and_then(|addrs| addrs.first())
            .map(|a| format!("{} {}@{}", s(a.name), s(a.mailbox), s(a.host)))
            .unwrap_or_default();
        Some(Self {
            message_id: normalize_message_id(&s(env.message_id)),
            from,
            subject: s(env.subject),
            date: s(env.date),
        })
    }

    pub fn matches(&self, other: &Self) -> bool {
        match (&self.message_id, &other.message_id) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            _ => {
                !(self.from.trim().is_empty() && self.subject.is_empty())
                    && self.from == other.from
                    && self.subject == other.subject
                    && self.date == other.date
            }
        }
    }
}

fn normalize_message_id(raw: &str) -> Option<String> {
    let s = raw.trim().trim_matches(|c| c == '<' || c == '>').trim();
    (!s.is_empty()).then(|| s.to_string())
}

pub fn quote_mailbox(name: &str) -> String {
    format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Dest UID from an IMAP COPYUID response code.
///
/// The rust imap crate drops the tagged OK line, so this only sees COPYUID
/// when the server sends it untagged (typical for UID MOVE).
pub fn parse_copyuid_dest(response: &[u8]) -> Option<u32> {
    String::from_utf8_lossy(response)
        .split(['\r', '\n'])
        .find_map(copyuid_dest_from_line)
}

fn copyuid_dest_from_line(line: &str) -> Option<u32> {
    let idx = line.to_ascii_uppercase().find("[COPYUID ")?;
    let rest = line.get(idx + "[COPYUID ".len()..)?;
    let inner = rest[..rest.find(']')?].trim();
    let mut parts = inner.splitn(3, char::is_whitespace);
    let _uidvalidity = parts.next()?;
    let _src = parts.next()?;
    let first = parts.next()?.trim().split(',').next()?;
    first.split(':').next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(id: Option<&str>, from: &str, subject: &str, date: &str) -> MailIdent {
        MailIdent {
            message_id: id.map(str::to_string),
            from: from.into(),
            subject: subject.into(),
            date: date.into(),
        }
    }

    #[test]
    fn copyuid_from_untagged_move() {
        let raw = b"* OK [COPYUID 1511554416 142 4567] Moved UIDs.\r\n";
        assert_eq!(parse_copyuid_dest(raw), Some(4567));
    }

    #[test]
    fn copyuid_from_tagged_ok() {
        let raw = b"a1 OK [COPYUID 38505 304 3956] Done\r\n";
        assert_eq!(parse_copyuid_dest(raw), Some(3956));
    }

    #[test]
    fn copyuid_dest_set_takes_first_uid() {
        let raw = b"* OK [COPYUID 1511554416 142,399 41:42] Moved UIDs.\r\n";
        assert_eq!(parse_copyuid_dest(raw), Some(41));
    }

    #[test]
    fn copyuid_absent() {
        assert_eq!(parse_copyuid_dest(b"a1 OK COPY completed\r\n"), None);
        assert_eq!(parse_copyuid_dest(b""), None);
    }

    #[test]
    fn quote_mailbox_escapes() {
        assert_eq!(quote_mailbox("Trash"), "\"Trash\"");
        assert_eq!(quote_mailbox(r#"a"b"#), r#""a\"b""#);
    }

    #[test]
    fn ident_matches_message_id() {
        let a = ident(Some("abc@x"), "a@x", "s", "d");
        assert!(a.matches(&ident(Some("ABC@x"), "other", "nope", "nope")));
        assert!(!a.matches(&ident(Some("other@x"), "a@x", "s", "d")));
    }

    #[test]
    fn ident_weak_match_without_message_id() {
        let a = ident(None, "a@x", "Hello", "1 Jan");
        let empty = ident(None, "", "", "");
        assert!(a.matches(&a.clone()));
        assert!(!empty.matches(&a));
        assert!(!a.matches(&empty));
    }

    #[test]
    fn normalize_strips_angle_brackets() {
        assert_eq!(normalize_message_id(" <id@host> "), Some("id@host".into()));
        assert_eq!(normalize_message_id("   "), None);
    }
}
