//! What a typed line means to the browser: an address or a search. Shared
//! by the browser's location bar and the launcher's Browser field so the
//! same words go to the same place.
//!
//! - whitespace anywhere → search ("how to tie a tie")
//! - an explicit scheme, a dotted host, or loopback → URL
//!   ("https://x", "about:blank", "github.com", "localhost:3000", "[::1]")
//! - anything else (a single bare word) → search ("weather", "rust")
//!
//! Searches go to Kagi: Enter is "I'm feeling lucky" (the first result),
//! the results page on request. [`history`] reads the browser's visited
//! pages, ranked as the location bar ranks them.

use std::path::PathBuf;

/// Kagi's search endpoint.
const SEARCH_PREFIX: &str = "https://kagi.com/search?q=";

/// Whether typed input is an address (vs. a search).
pub fn looks_like_url(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || t.chars().any(char::is_whitespace) {
        return false;
    }
    if explicit_scheme(t).is_some() {
        return true;
    }
    if t.eq_ignore_ascii_case("scroll-stress") || t.to_ascii_lowercase().starts_with("sola:") {
        return true;
    }
    if is_loopback_host(scheme_less_host(t)) {
        return true;
    }
    // A dotted host like "github.com" or "a.b/c": a dot that is neither the
    // first nor the last character.
    matches!(t.find('.'), Some(i) if i > 0 && i < t.len() - 1)
}

/// An address as typed, made loadable. A string with an explicit scheme
/// (`https:`, `about:`, `mailto:`, `file:`, …) is left intact. A local file
/// path (absolute, `./`, `../`, or an existing relative path) becomes an
/// absolute `file://` URL. Everything else gets a scheme: `http://` for
/// localhost / loopback (local servers rarely have a trusted cert),
/// `https://` otherwise. `host:port` counts as a bare host.
pub fn normalize_url(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(file) = crate::open_url::file_url_from_local_path(trimmed) {
        return file;
    }
    if explicit_scheme(trimmed).is_some() {
        return trimmed.to_string();
    }
    with_default_scheme(trimmed)
}

/// Kagi results page for `q`.
pub fn kagi_search_url(q: &str) -> String {
    format!("{SEARCH_PREFIX}{}", encode_query(q.trim()))
}

/// Kagi "I'm feeling lucky" (`\query`): the first result for `q`.
pub fn kagi_lucky_url(q: &str) -> String {
    format!("{SEARCH_PREFIX}%5C{}", encode_query(q.trim()))
}

/// A Kagi results / lucky URL (kept out of visit history).
pub fn is_kagi_search_url(url: &str) -> bool {
    url.to_ascii_lowercase().contains("kagi.com/search")
}

/// Where Enter takes typed input: the address, or the first result for a
/// search. Empty input goes nowhere (an empty string).
pub fn enter_url(typed: &str) -> String {
    let typed = typed.trim();
    if typed.is_empty() {
        String::new()
    } else if looks_like_url(typed) {
        normalize_url(typed)
    } else {
        kagi_lucky_url(typed)
    }
}

/// Prefix a scheme-less token. Loopback is `http://`; everything else
/// is `https://`. Unbracketed IPv6 loopback (`::1`) is wrapped.
fn with_default_scheme(s: &str) -> String {
    let host = scheme_less_host(s);
    let scheme = if is_loopback_host(host) { "http" } else { "https" };
    if host.contains(':') && !s.starts_with('[') {
        let rest = &s[host.len()..];
        format!("{scheme}://[{host}]{rest}")
    } else {
        format!("{scheme}://{s}")
    }
}

/// Host of a scheme-less token (`host`, `host:port`, `host/path`, `[::1]`,
/// `[::1]:8080`).
fn scheme_less_host(s: &str) -> &str {
    if let Some(rest) = s.strip_prefix('[')
        && let Some(end) = rest.find(']')
    {
        return &rest[..end];
    }
    let no_path = s.split_once('/').map(|(h, _)| h).unwrap_or(s);
    if let Some((host, port)) = no_path.rsplit_once(':')
        && !port.is_empty()
        && port.chars().all(|c| c.is_ascii_digit())
        && !host.contains(':')
    {
        return host;
    }
    no_path
}

/// `localhost`, `*.localhost`, `127.0.0.0/8`, and IPv6 `::1`.
fn is_loopback_host(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    if h == "localhost" || h.ends_with(".localhost") || is_ipv4_loopback(&h) {
        return true;
    }
    h == "::1" || h == "0:0:0:0:0:0:0:1" || h.strip_prefix("::ffff:").is_some_and(is_ipv4_loopback)
}

fn is_ipv4_loopback(s: &str) -> bool {
    let mut parts = s.split('.');
    if parts.next().and_then(|p| p.parse::<u8>().ok()) != Some(127) {
        return false;
    }
    let mut n = 1usize;
    for p in parts {
        if p.parse::<u8>().is_err() {
            return false;
        }
        n += 1;
    }
    n == 4
}

/// The explicit URL scheme of `s` (the alphabetic run before the first
/// `:`), but not for `host:port`, where the colon is followed by digits.
fn explicit_scheme(s: &str) -> Option<&str> {
    let colon = s.find(':')?;
    let scheme = &s[..colon];
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let port = s[colon + 1..].split('/').next().unwrap_or("");
    if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(scheme)
}

/// Percent-encode `s` as a URL query value: RFC 3986 unreserved bytes pass
/// through, everything else (spaces too) becomes `%XX`.
fn encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The browser's visited pages (`~/.local/share/sola/browser/shared/history.json`,
/// written by the browser): the record, the file, and the ranking the
/// location bar shows its matches by.
pub mod history {
    use super::{PathBuf, is_kagi_search_url};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Visit {
        pub url: String,
        #[serde(default)]
        pub title: String,
        #[serde(default)]
        pub last_unix: u64,
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct Store {
        #[serde(default)]
        pub version: u32,
        #[serde(default)]
        pub items: Vec<Visit>,
    }

    /// The history file.
    pub fn path() -> PathBuf {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|| PathBuf::from(".local/share"))
            .join("sola/browser/shared/history.json")
    }

    /// The visits, newest first (none when the browser has not written any).
    pub fn load() -> Vec<Visit> {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|raw| serde_json::from_str::<Store>(&raw).ok())
            .map(|s| s.items)
            .unwrap_or_default()
    }

    /// A page worth remembering: not blank, internal, or a search page.
    pub fn keep_url(url: &str) -> bool {
        if url.is_empty() || url == "about:blank" {
            return false;
        }
        let lower = url.to_ascii_lowercase();
        if lower.starts_with("devtools:")
            || lower.starts_with("sola:")
            || lower.starts_with("data:")
            || lower.starts_with("javascript:")
        {
            return false;
        }
        !is_kagi_search_url(&lower)
    }

    /// The best `n` matches for `query`, one per page (query strings
    /// aside): a host that starts with it first, then a host, title or
    /// address that holds it; newer first among equals. An empty query
    /// matches nothing.
    pub fn search(items: &[Visit], query: &str, n: usize) -> Vec<Visit> {
        let q = query.trim().to_ascii_lowercase();
        if q.is_empty() || q == "https://" || q == "http://" {
            return Vec::new();
        }
        let mut ranked: Vec<(u32, usize, &Visit)> = items
            .iter()
            .enumerate()
            .filter_map(|(i, v)| score(&q, v).map(|s| (s, i, v)))
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for (_, _, v) in ranked {
            if !seen.insert(path_key(&v.url)) {
                continue;
            }
            out.push(v.clone());
            if out.len() == n {
                break;
            }
        }
        out
    }

    fn score(q: &str, v: &Visit) -> Option<u32> {
        let url = v.url.to_ascii_lowercase();
        let title = v.title.to_ascii_lowercase();
        let host = host_of(&url);
        let s = if host.starts_with(q)
            || url.contains(&format!("://{q}"))
            || url.contains(&format!(".{q}"))
        {
            300
        } else if host.contains(q) {
            200
        } else if title.contains(q) {
            120
        } else if url.contains(q) {
            80
        } else {
            return None;
        };
        Some(s)
    }

    fn path_key(url: &str) -> String {
        let mut s = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
        while s.ends_with('/') && s.matches('/').count() > 2 {
            s.pop();
        }
        s
    }

    /// The host of a visited URL, lowercase.
    pub fn host_of(url: &str) -> String {
        let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
        rest.split('/').next().unwrap_or("").to_ascii_lowercase()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn visit(url: &str, title: &str) -> Visit {
            Visit {
                url: url.into(),
                title: title.into(),
                last_unix: 0,
            }
        }

        #[test]
        fn hosts_first_then_titles_one_per_page() {
            let items = vec![
                visit("https://news.ycombinator.com/", "Hacker News"),
                visit("https://github.com/moonlight-pm/Sola", "Sola"),
                visit("https://github.com/moonlight-pm/Sola?tab=readme", "Sola readme"),
                visit("https://docs.rs/iced", "iced — GitHub"),
            ];
            let hits = search(&items, "git", 5);
            assert_eq!(hits[0].url, "https://github.com/moonlight-pm/Sola");
            assert_eq!(hits.len(), 2);
            assert!(search(&items, "  ", 5).is_empty());
        }

        #[test]
        fn search_pages_are_not_kept() {
            assert!(!keep_url("https://kagi.com/search?q=x"));
            assert!(!keep_url("about:blank"));
            assert!(keep_url("https://example.com"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_and_searches() {
        assert!(looks_like_url("github.com"));
        assert!(looks_like_url("localhost:3000"));
        assert!(looks_like_url("https://x"));
        assert!(!looks_like_url("how to tie a tie"));
        assert!(!looks_like_url("weather"));
    }

    #[test]
    fn enter_goes_to_the_address_or_the_first_result() {
        assert_eq!(enter_url("github.com"), "https://github.com");
        assert_eq!(enter_url("localhost:8080"), "http://localhost:8080");
        assert_eq!(enter_url("rust iced"), "https://kagi.com/search?q=%5Crust%20iced");
        assert_eq!(enter_url("   "), "");
        assert_eq!(kagi_search_url("a&b"), "https://kagi.com/search?q=a%26b");
    }
}
