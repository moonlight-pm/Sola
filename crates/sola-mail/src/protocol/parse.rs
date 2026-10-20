//! FETCH responses → [`MessageSummary`] / [`MessageBody`].
//!
//! Headers go through mail-parser so RFC 2047 encoded words and legacy
//! charsets decode. IMAP `ENVELOPE` strings are raw header bytes
//! (`=?UTF-8?B?…?=`), so the list does not use them.

use imap::types::{Fetch, Flag};
use mail_parser::{Addr, Address, HeaderValue, MessageParser};

use super::attachments::collect_attachments;
use super::html_text::reading_blocks;
use super::types::{MailId, MessageBody, MessageSummary};

/// Flags + structure (list paperclip) + the headers a list row shows.
pub const SUMMARY_ITEMS: &str = "(UID FLAGS INTERNALDATE BODYSTRUCTURE \
     BODY.PEEK[HEADER.FIELDS (FROM TO CC SUBJECT DATE X-FORWARDED-FOR)])";

pub const BODY_ITEMS: &str = "(UID FLAGS BODY.PEEK[])";

/// One list row. `None` for soft-deleted (`\Deleted`) messages.
pub fn parse_summary(fetch: &Fetch, account: &str, folder: &str) -> Option<MessageSummary> {
    let uid = fetch.uid?;
    let flags = fetch.flags();
    if flags.iter().any(|f| matches!(f, Flag::Deleted)) {
        return None;
    }
    let headers = fetch
        .header()
        .and_then(|h| MessageParser::default().parse_headers(h));
    let (from, to, subject, header_date, forwarded_for) = match &headers {
        Some(m) => (
            m.from().and_then(Address::first).map(format_addr).unwrap_or_default(),
            addr_list(m.to()),
            m.subject().unwrap_or("").trim().to_string(),
            m.date().filter(|d| d.is_valid()).map(|d| d.to_timestamp()),
            m.header_raw("X-Forwarded-For").and_then(first_forwarded),
        ),
        None => Default::default(),
    };
    let date = header_date
        .filter(|t| *t != 0)
        .or_else(|| fetch.internal_date().map(|d| d.timestamp()))
        .unwrap_or(0);
    Some(MessageSummary {
        id: MailId::new(account, folder, uid),
        from,
        to,
        subject,
        date,
        seen: flags.iter().any(|f| matches!(f, Flag::Seen)),
        flagged: flags.iter().any(|f| matches!(f, Flag::Flagged)),
        forwarded_for,
        has_attachment: fetch.bodystructure().is_some_and(structure_has_attachment),
    })
}

/// Full letter: headers, reading blocks (HTML → prose here, off the UI
/// thread), and attachments.
pub fn parse_body(fetch: &Fetch, id: MailId) -> anyhow::Result<MessageBody> {
    let raw = fetch
        .body()
        .ok_or_else(|| anyhow::anyhow!("server sent no body"))?;
    let parsed = MessageParser::default()
        .parse(raw)
        .ok_or_else(|| anyhow::anyhow!("could not parse the message"))?;

    let html = parsed.body_html(0).map(|s| s.into_owned());
    let text = parsed
        .body_text(0)
        .map(|s| s.into_owned())
        .unwrap_or_default();
    let blocks = reading_blocks(html.as_deref(), &text);

    Ok(MessageBody {
        id,
        from: parsed
            .from()
            .and_then(Address::first)
            .map(format_addr)
            .unwrap_or_default(),
        reply_to: addr_list(parsed.reply_to()),
        to: addr_list(parsed.to()),
        cc: addr_list(parsed.cc()),
        subject: parsed.subject().unwrap_or("").trim().to_string(),
        date: parsed
            .date()
            .filter(|d| d.is_valid())
            .map(|d| d.to_timestamp())
            .unwrap_or(0),
        text,
        message_id: parsed.message_id().map(str::to_string),
        in_reply_to: text_list(parsed.in_reply_to()).into_iter().next(),
        references: text_list(parsed.references()),
        attachments: collect_attachments(&parsed),
        blocks,
    })
}

/// `Name <addr>`, or the bare address when there is no display name.
pub fn format_addr(a: &Addr<'_>) -> String {
    let name = a.name().map(str::trim).unwrap_or("");
    let addr = a.address().map(str::trim).unwrap_or("");
    match (name.is_empty(), addr.is_empty()) {
        (false, false) => format!("{name} <{addr}>"),
        (true, _) => addr.to_string(),
        (false, true) => name.to_string(),
    }
}

fn addr_list(field: Option<&Address<'_>>) -> Vec<String> {
    field
        .map(|a| a.iter().map(format_addr).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

fn text_list(value: &HeaderValue<'_>) -> Vec<String> {
    value
        .as_text_list()
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// First address in `X-Forwarded-For` (the recipient the mail was
/// forwarded from; the header may list several).
fn first_forwarded(raw: &str) -> Option<String> {
    raw.split([' ', ',', '\t', '\r', '\n'])
        .map(|s| s.trim().trim_matches(|c| c == '<' || c == '>'))
        .find(|s| s.contains('@'))
        .map(str::to_string)
}

fn structure_has_attachment(bs: &imap_proto::types::BodyStructure<'_>) -> bool {
    use imap_proto::types::BodyStructure::*;
    match bs {
        Multipart { bodies, .. } => bodies.iter().any(structure_has_attachment),
        Message { .. } => true,
        Text { common, .. } => leaf_is_file(common, true),
        Basic { common, .. } => leaf_is_file(common, false),
    }
}

fn leaf_is_file(common: &imap_proto::types::BodyContentCommon<'_>, is_text: bool) -> bool {
    if let Some(disp) = &common.disposition {
        if disp.ty.eq_ignore_ascii_case("attachment") {
            return true;
        }
        if disp.ty.eq_ignore_ascii_case("inline") {
            return false;
        }
    }
    let named = |params: Option<&[(&str, &str)]>| {
        params.is_some_and(|ps| {
            ps.iter().any(|(k, v)| {
                !v.is_empty()
                    && (k.eq_ignore_ascii_case("filename") || k.eq_ignore_ascii_case("name"))
            })
        })
    };
    if named(common.disposition.as_ref().and_then(|d| d.params.as_deref()))
        || named(common.ty.params.as_deref())
    {
        return true;
    }
    !is_text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(raw: &str) -> mail_parser::Message<'_> {
        MessageParser::default()
            .parse_headers(raw.as_bytes())
            .expect("headers")
    }

    #[test]
    fn encoded_words_decode() {
        let m = headers(
            "From: =?UTF-8?Q?Ren=C3=A9e?= <renee@example.com>\r\n\
             Subject: =?UTF-8?B?8J+OiSBZb3VyIG9yZGVy?=\r\n\r\n",
        );
        assert_eq!(
            format_addr(m.from().and_then(Address::first).expect("from")),
            "Renée <renee@example.com>"
        );
        assert_eq!(m.subject(), Some("🎉 Your order"));
    }

    #[test]
    fn recipient_lists_keep_quoted_commas() {
        let m = headers("To: \"Doe, Jane\" <jane@example.com>, bob@example.com\r\n\r\n");
        assert_eq!(
            addr_list(m.to()),
            vec!["Doe, Jane <jane@example.com>", "bob@example.com"]
        );
    }

    #[test]
    fn forwarded_for_takes_first_address() {
        assert_eq!(
            first_forwarded(" me@gmail.com other@x.com"),
            Some("me@gmail.com".into())
        );
        assert_eq!(first_forwarded("<a@b.co>,c@d.co"), Some("a@b.co".into()));
        assert_eq!(first_forwarded("  "), None);
    }
}
