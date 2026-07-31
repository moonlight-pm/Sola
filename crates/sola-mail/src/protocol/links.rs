//! Extract clickable http(s) URLs from plain text and HTML mail bodies.
//!
//! Soft-wrapped URLs (line break mid-path, common at ~78 columns or from
//! `html2text`) are rejoined so they still produce a full clickable target.

use crate::protocol::types::MessageBody;

/// Collect unique openable URLs for a message (HTML `href`s first, then body text).
pub fn links_for_message(body: &MessageBody) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(html) = &body.html {
        // Strip soft hyphens / zero-width chars that break attribute scans.
        let cleaned = scrub_invisible(html);
        for u in extract_hrefs(&cleaned) {
            push_unique(&mut out, u);
        }
        // Also scan html2text-ish plain conversion for bare URLs in HTML.
        for u in extract_urls_from_text(&crate::protocol::html_text::to_plain(&cleaned)) {
            push_unique(&mut out, u);
        }
    }
    for u in extract_urls_from_text(&body.display_text()) {
        push_unique(&mut out, u);
    }
    if !body.text.is_empty() {
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

fn scrub_invisible(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{00ad}' | '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}'))
        .collect()
}

/// Pull `href="..."` / `href='...'` from HTML (case-insensitive attribute name).
pub fn extract_hrefs(html: &str) -> Vec<String> {
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
    let cleaned = scrub_invisible(text);
    // Angle-bracket form: <https://...> (RFC 3986 appendix)
    let mut pre = Vec::new();
    let mut rest = cleaned.as_str();
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        if let Some(end) = after.find('>') {
            let inner = after[..end].trim();
            if is_http_url(inner) || inner.to_ascii_lowercase().starts_with("www.") {
                let u = if inner.to_ascii_lowercase().starts_with("www.") {
                    format!("https://{inner}")
                } else {
                    normalize_url(inner)
                };
                if is_http_url(&u) {
                    push_unique(&mut pre, u);
                }
            }
            rest = &after[end + 1..];
        } else {
            break;
        }
    }

    let chars: Vec<char> = cleaned.chars().collect();
    let mut out = pre;
    let mut i = 0;
    while i < chars.len() {
        let Some((scheme_len, scheme_prefix)) = scheme_at(&chars, i) else {
            i += 1;
            continue;
        };
        let start = i;
        i += scheme_len;
        while i < chars.len() {
            let c = chars[i];
            if is_url_body(c) {
                i += 1;
                continue;
            }
            // Soft wrap: newline (optionally after a format=flowed trailing space).
            if c == ' ' || c == '\t' {
                let mut j = i;
                while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
                    j += 1;
                }
                if j < chars.len() && (chars[j] == '\n' || chars[j] == '\r') {
                    // format=flowed soft break — skip space+newline and continue.
                    while j < chars.len() && (chars[j] == '\n' || chars[j] == '\r') {
                        j += 1;
                    }
                    // Skip quote markers common in replies: "> "
                    while j < chars.len() && (chars[j] == '>' || chars[j] == ' ' || chars[j] == '\t')
                    {
                        if chars[j] == '>' {
                            j += 1;
                            continue;
                        }
                        if chars[j] == ' ' || chars[j] == '\t' {
                            j += 1;
                            continue;
                        }
                        break;
                    }
                    if j < chars.len()
                        && is_url_body(chars[j])
                        && scheme_at(&chars, j).is_none()
                        && !is_sentence_start_after_wrap(&chars, j)
                    {
                        i = j;
                        continue;
                    }
                }
            }
            if c == '\n' || c == '\r' {
                let mut j = i;
                while j < chars.len() && (chars[j] == '\n' || chars[j] == '\r') {
                    j += 1;
                }
                let mut k = j;
                while k < chars.len() && (chars[k] == ' ' || chars[k] == '\t' || chars[k] == '>') {
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
        let mut uri = String::new();
        if scheme_prefix.is_empty() {
            uri.push_str("https://");
        }
        let mut r = start;
        while r < i {
            let c = chars[r];
            if c == '\n' || c == '\r' || c == '>' {
                r += 1;
                while r < i && (chars[r] == ' ' || chars[r] == '\t' || chars[r] == '>') {
                    r += 1;
                }
                continue;
            }
            // Drop single spaces that only exist as format=flowed soft breaks
            // (space immediately before what was a newline — already skipped NL).
            if c == ' ' || c == '\t' {
                // Only keep space if it looks intentional inside URL (rare); drop soft breaks.
                r += 1;
                continue;
            }
            uri.push(c);
            r += 1;
        }
        while uri
            .chars()
            .last()
            .is_some_and(|c| matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '\'' | '"' | '>'))
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

fn normalize_url(raw: &str) -> String {
    let t = raw.trim();
    t.replace("&amp;", "&")
        .replace("&#38;", "&")
        .replace("&quot;", "")
        .replace("&#39;", "")
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

fn is_sentence_start_after_wrap(chars: &[char], i: usize) -> bool {
    let c = chars[i];
    if matches!(c, '/' | '?' | '#' | '&' | '=' | '%' | '-' | '_' | '.' | '~' | '+') {
        return false;
    }
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
    fn rejoins_format_flowed_space_break() {
        // Trailing space before newline (format=flowed soft break).
        let text = "https://example.com/long \npath/more";
        let u = extract_urls_from_text(text);
        assert_eq!(u, vec!["https://example.com/longpath/more"]);
    }

    #[test]
    fn angle_bracket_url() {
        let u = extract_urls_from_text("see <https://example.com/a/b> thanks");
        assert_eq!(u, vec!["https://example.com/a/b"]);
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
