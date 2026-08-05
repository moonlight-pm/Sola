//! Extract clickable http(s) URLs from plain text and HTML mail bodies.
//!
//! Soft-wrapped URLs (line break mid-path, common at ~78 columns or from
//! `html2text`) are rejoined so they still produce a full clickable target.

use crate::protocol::types::MessageBody;

/// Collect unique openable URLs for a message.
///
/// Only scans the **plain reading text** (with soft-wrap rejoin). We deliberately
/// do **not** list every HTML `href` — that floods the UI with trackers,
/// list-unsubscribe, and footer links the user never meant to click.
pub fn links_for_message(body: &MessageBody) -> Vec<String> {
    let mut out = Vec::new();
    for u in extract_urls_from_text(&body.display_text()) {
        push_unique(&mut out, u);
    }
    // Prefer text when empty HTML-only edge cases leave nothing.
    if out.is_empty() && !body.text.is_empty() {
        for u in extract_urls_from_text(&body.text) {
            push_unique(&mut out, u);
        }
    }
    out
}

fn push_unique(out: &mut Vec<String>, u: String) {
    if u.is_empty() {
        return;
    }
    if !out.iter().any(|x| x == &u) {
        out.push(u);
    }
}

/// Pull `href="..."` / `href='...'` from HTML (case-insensitive attribute name).
#[cfg(test)]
fn extract_hrefs(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = html.as_bytes();
    let lower: Vec<u8> = html.bytes().map(|b| b.to_ascii_lowercase()).collect();
    let needle = b"href=";
    let mut i = 0;
    while i + needle.len() < lower.len() {
        if &lower[i..i + needle.len()] != needle {
            i += 1;
            continue;
        }
        i += needle.len();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let quote = bytes[i];
        let (end_pat, start) = if quote == b'"' || quote == b'\'' {
            i += 1;
            (quote, i)
        } else {
            // Unquoted href=value
            (b' ', i)
        };
        let mut j = start;
        while j < bytes.len() {
            let c = bytes[j];
            if quote == b'"' || quote == b'\'' {
                if c == end_pat {
                    break;
                }
            } else if c.is_ascii_whitespace() || c == b'>' {
                break;
            }
            j += 1;
        }
        if j > start {
            if let Ok(raw) = std::str::from_utf8(&bytes[start..j]) {
                let u = normalize_url(raw);
                if is_http_url(&u) {
                    push_unique(&mut out, u);
                }
            }
        }
        i = j.saturating_add(1);
    }
    out
}

/// Find http(s) URLs in plain text, rejoining soft line wraps mid-URL.
pub fn extract_urls_from_text(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let Some((scheme_len, scheme_prefix)) = scheme_at(&chars, i) else {
            i += 1;
            continue;
        };
        let start = i;
        i += scheme_len;
        // Consume URL body, treating a single newline as soft wrap when the
        // next non-empty line continues the path (not a new sentence/scheme).
        while i < chars.len() {
            let c = chars[i];
            if is_url_body(c) {
                i += 1;
                continue;
            }
            if c == '\n' || c == '\r' {
                let mut j = i;
                while j < chars.len() && (chars[j] == '\n' || chars[j] == '\r') {
                    j += 1;
                }
                // Optional leading whitespace after wrap (rare).
                let mut k = j;
                while k < chars.len() && (chars[k] == ' ' || chars[k] == '\t') {
                    k += 1;
                }
                if k < chars.len()
                    && is_url_body(chars[k])
                    && scheme_at(&chars, k).is_none()
                    && !is_sentence_start_after_wrap(&chars, k)
                {
                    i = k;
                    continue;
                }
            }
            break;
        }
        // Build URI without embedded newlines/soft-wrap gaps.
        let mut uri = String::new();
        if scheme_prefix.is_empty() {
            uri.push_str("https://");
        }
        let mut r = start;
        while r < i {
            let c = chars[r];
            if c == '\n' || c == '\r' {
                r += 1;
                while r < i && (chars[r] == ' ' || chars[r] == '\t') {
                    r += 1;
                }
                continue;
            }
            uri.push(c);
            r += 1;
        }
        // Strip trailing punctuation that is rarely part of the URI.
        while uri
            .chars()
            .last()
            .is_some_and(|c| matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '\'' | '"'))
        {
            uri.pop();
        }
        if is_http_url(&uri) {
            push_unique(&mut out, uri);
        }
    }
    out
}

fn is_http_url(u: &str) -> bool {
    let l = u.to_ascii_lowercase();
    (l.starts_with("http://") || l.starts_with("https://")) && u.len() > 10
}

#[cfg(test)]
fn normalize_url(raw: &str) -> String {
    let t = raw.trim();
    // HTML entities common in mail
    t.replace("&amp;", "&")
        .replace("&#38;", "&")
        .replace("&quot;", "")
}

fn scheme_at(chars: &[char], i: usize) -> Option<(usize, &'static str)> {
    if starts_with_ci(chars, i, "https://") {
        return Some((8, "https://"));
    }
    if starts_with_ci(chars, i, "http://") {
        return Some((7, "http://"));
    }
    if starts_with_ci(chars, i, "www.") {
        let boundary = i == 0 || !is_url_body(chars[i - 1]);
        if boundary {
            return Some((4, ""));
        }
    }
    None
}

fn starts_with_ci(chars: &[char], i: usize, prefix: &str) -> bool {
    let pref: Vec<char> = prefix.chars().collect();
    if i + pref.len() > chars.len() {
        return false;
    }
    chars[i..i + pref.len()]
        .iter()
        .zip(pref.iter())
        .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn is_url_body(c: char) -> bool {
    match c {
        ' ' | '\t' | '\n' | '\r' | '<' | '>' | '"' | '\'' | '`' | '{' | '}' | '|' | '\\' | '^'
        | '[' | ']' => false,
        _ => !c.is_control(),
    }
}

/// After a soft wrap, reject lines that look like a new English sentence
/// (`The ...`) rather than a path continuation (`path`, `?q=`, `/foo`).
fn is_sentence_start_after_wrap(chars: &[char], i: usize) -> bool {
    let c = chars[i];
    // Path / query / fragment continuations are fine.
    if matches!(c, '/' | '?' | '#' | '&' | '=' | '%' | '-' | '_' | '.') {
        return false;
    }
    // Uppercase letter then lowercase word → likely prose, not URL.
    if c.is_uppercase() {
        if let Some(next) = chars.get(i + 1) {
            if next.is_lowercase() || next.is_whitespace() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_simple_https() {
        let u = extract_urls_from_text("see https://example.com/path for more");
        assert_eq!(u, vec!["https://example.com/path"]);
    }

    #[test]
    fn rejoins_soft_wrapped_url() {
        let text = "Click https://auth.example.com/login/magic/verify?token=abc\ndef-ghi&next=1 please";
        let u = extract_urls_from_text(text);
        assert_eq!(
            u,
            vec!["https://auth.example.com/login/magic/verify?token=abcdef-ghi&next=1"]
        );
    }

    #[test]
    fn rejoins_wrap_after_slash() {
        let text = "https://cdn.example.com/very/long/\npath/to/resource";
        let u = extract_urls_from_text(text);
        assert_eq!(u, vec!["https://cdn.example.com/very/long/path/to/resource"]);
    }

    #[test]
    fn does_not_eat_next_sentence() {
        let text = "Visit https://example.com/a\nThen reply when done.";
        let u = extract_urls_from_text(text);
        assert_eq!(u, vec!["https://example.com/a"]);
    }

    #[test]
    fn strips_trailing_punct() {
        let u = extract_urls_from_text("go https://example.com/a.");
        assert_eq!(u, vec!["https://example.com/a"]);
    }

    #[test]
    fn href_from_html() {
        let html = r#"<a href="https://example.com/x?a=1&amp;b=2">click</a>"#;
        let u = extract_hrefs(html);
        assert_eq!(u, vec!["https://example.com/x?a=1&b=2"]);
    }

    #[test]
    fn href_single_quotes() {
        let html = r#"<A HREF='https://example.com/y'>y</A>"#;
        let u = extract_hrefs(html);
        assert_eq!(u, vec!["https://example.com/y"]);
    }
}
