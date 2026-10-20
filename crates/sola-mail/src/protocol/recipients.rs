//! Typed recipient lists (`Ana <a@x.com>, "Doe, J" <j@y.com>, b@z.com`).

use lettre::message::Mailbox;

/// Top-level comma-separated parts, keeping commas inside quotes and
/// `<…>`. Empty parts are dropped; each part is trimmed.
pub fn split(raw: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut quoted, mut angle, mut start) = (false, false, 0);
    for (i, c) in raw.char_indices() {
        match c {
            '"' if !angle => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            ',' | ';' if !quoted && !angle => {
                out.push(raw[start..i].trim());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(raw[start..].trim());
    out.retain(|p| !p.is_empty());
    out
}

/// Parts that are not a usable address.
pub fn invalid(raw: &str) -> Vec<String> {
    split(raw)
        .into_iter()
        .filter(|p| p.parse::<Mailbox>().is_err())
        .map(str::to_string)
        .collect()
}

/// The part being typed: after the last top-level comma (may be empty).
pub fn current_token(raw: &str) -> &str {
    let (mut quoted, mut angle, mut start) = (false, false, 0);
    for (i, c) in raw.char_indices() {
        match c {
            '"' if !angle => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            ',' | ';' if !quoted && !angle => start = i + c.len_utf8(),
            _ => {}
        }
    }
    raw[start..].trim_start()
}

/// Swap the part being typed for `chosen` and leave `", "` to keep going.
pub fn complete(raw: &str, chosen: &str) -> String {
    let token = current_token(raw);
    let head = raw[..raw.len() - token.len()].trim_end();
    if head.is_empty() {
        format!("{chosen}, ")
    } else {
        format!("{head} {chosen}, ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_keeps_quoted_and_angle_commas() {
        assert_eq!(
            split(r#""Doe, Jane" <jane@x.com>, bob@y.com;  , <c,d@z.com>"#),
            vec![r#""Doe, Jane" <jane@x.com>"#, "bob@y.com", "<c,d@z.com>"]
        );
        assert!(split("  ").is_empty());
    }

    #[test]
    fn invalid_names_the_bad_part() {
        assert_eq!(
            invalid("Ana <ana@x.com>, half typed@, bo@y.org"),
            vec!["half typed@"]
        );
        assert!(invalid("").is_empty());
    }

    #[test]
    fn complete_replaces_the_typed_token() {
        assert_eq!(current_token("ana@x.com, bo"), "bo");
        assert_eq!(
            complete("ana@x.com, bo", "Bo <bo@y.org>"),
            "ana@x.com, Bo <bo@y.org>, "
        );
        assert_eq!(complete("an", "Ana <ana@x.com>"), "Ana <ana@x.com>, ");
        assert_eq!(current_token("ana@x.com, "), "");
    }
}
