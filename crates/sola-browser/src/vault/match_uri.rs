//! Bitwarden-style URI matching for autofill candidate selection.
//!
//! Defaults to domain match when the cipher URI has no explicit match type.
//! Includes a small built-in equivalent-domain set (Google family) so a
//! `google.com` login also matches `youtube.com` / `accounts.google.com`
//! the way official clients do. Full sync-driven global domains can come later.

use bitwarden_vault::UriMatchType;
use url::Url;

/// Built-in equivalent domain groups (subset of Bitwarden global defaults).
/// Each inner list is one equivalence class.
const EQUIVALENT_DOMAIN_GROUPS: &[&[&str]] = &[
    &[
        "google.com",
        "googleapis.com",
        "gstatic.com",
        "youtube.com",
        "youtu.be",
        "ytimg.com",
        "googlemail.com",
        "gmail.com",
        "google.co.uk",
        "google.de",
        "google.fr",
        "google.ca",
        "google.com.au",
        "android.com",
        "chrome.com",
        "chromium.org",
    ],
    &["apple.com", "icloud.com", "icloud.com.cn", "me.com", "mzstatic.com"],
    &["microsoft.com", "live.com", "outlook.com", "office.com", "office365.com", "microsoftonline.com", "xbox.com", "skype.com"],
    &["amazon.com", "amazon.co.uk", "amazon.de", "amazon.fr", "amazon.ca", "amazon.com.au", "aws.amazon.com"],
];

/// Returns true when `page_url` should be considered a match for a stored
/// login URI under the given match type (`None` → Domain).
pub fn uri_matches(page_url: &str, login_uri: &str, match_type: Option<UriMatchType>) -> bool {
    let Some(page) = parse_url(page_url) else {
        return false;
    };
    let Some(stored) = parse_url(login_uri).or_else(|| {
        // Bitwarden also stores bare hostnames; try as https://host
        parse_url(&format!("https://{login_uri}"))
    }) else {
        return false;
    };
    match_with_urls(&page, &stored, match_type)
}

fn parse_url(s: &str) -> Option<Url> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    Url::parse(trimmed)
        .ok()
        .or_else(|| Url::parse(&format!("https://{trimmed}")).ok())
}

fn match_with_urls(page: &Url, stored: &Url, match_type: Option<UriMatchType>) -> bool {
    match match_type.unwrap_or(UriMatchType::Domain) {
        UriMatchType::Never => false,
        UriMatchType::Exact => page.as_str().trim_end_matches('/') == stored.as_str().trim_end_matches('/'),
        UriMatchType::StartsWith => {
            let page_s = page.as_str();
            let stored_s = stored.as_str().trim_end_matches('/');
            page_s.starts_with(stored_s)
        }
        UriMatchType::Host => {
            hosts_equal(page, stored) && ports_compatible(page, stored)
        }
        UriMatchType::Domain => base_domains_equal(page, stored),
        UriMatchType::RegularExpression => {
            // Spike: treat as exact host match rather than compiling untrusted regex
            // from vault data without a safe engine bound yet.
            hosts_equal(page, stored)
        }
    }
}

fn hosts_equal(a: &Url, b: &Url) -> bool {
    match (a.host_str(), b.host_str()) {
        (Some(ha), Some(hb)) => ha.eq_ignore_ascii_case(hb),
        _ => false,
    }
}

fn ports_compatible(a: &Url, b: &Url) -> bool {
    a.port_or_known_default() == b.port_or_known_default()
}

/// Rough registrable-domain equality: last two labels of the host (good enough
/// for dogfood; not PSL-complete), plus equivalent-domain groups.
fn base_domains_equal(a: &Url, b: &Url) -> bool {
    match (a.host_str(), b.host_str()) {
        (Some(ha), Some(hb)) => {
            let da = base_domain(ha);
            let db = base_domain(hb);
            da.eq_ignore_ascii_case(&db) || equivalent_domains(&da, &db)
        }
        _ => false,
    }
}

fn equivalent_domains(a: &str, b: &str) -> bool {
    let a = a.to_ascii_lowercase();
    let b = b.to_ascii_lowercase();
    if a == b {
        return true;
    }
    // Inputs are already base domains (e.g. google.com, youtube.com).
    for group in EQUIVALENT_DOMAIN_GROUPS {
        if group.iter().any(|d| a == *d) && group.iter().any(|d| b == *d) {
            return true;
        }
    }
    false
}

fn base_domain(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    // IPv4 / IPv6: require exact host match path — here return whole host.
    if host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[') {
        return host;
    }
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() <= 2 {
        return host;
    }
    // Handle common multi-part TLD-ish endings poorly but predictably:
    // foo.co.uk → co.uk only when middle is short; keep simple 2-label default.
    if parts.len() >= 3 {
        let tld = parts[parts.len() - 1];
        let sld = parts[parts.len() - 2];
        // e.g. co.uk, com.au
        if tld.len() == 2 && sld.len() <= 3 && parts.len() >= 3 {
            return parts[parts.len() - 3..].join(".");
        }
    }
    parts[parts.len() - 2..].join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_match_subdomains() {
        assert!(uri_matches(
            "https://accounts.google.com/signin",
            "https://google.com",
            Some(UriMatchType::Domain),
        ));
        assert!(uri_matches(
            "https://github.com/login",
            "https://www.github.com/",
            None,
        ));
    }

    #[test]
    fn host_match_rejects_other_subdomain() {
        assert!(!uri_matches(
            "https://evil.example.com/login",
            "https://app.example.com/login",
            Some(UriMatchType::Host),
        ));
        assert!(uri_matches(
            "https://app.example.com/x",
            "https://app.example.com/login",
            Some(UriMatchType::Host),
        ));
    }

    #[test]
    fn exact_and_never() {
        assert!(uri_matches(
            "https://example.com/a",
            "https://example.com/a",
            Some(UriMatchType::Exact),
        ));
        assert!(!uri_matches(
            "https://example.com/a",
            "https://example.com/b",
            Some(UriMatchType::Exact),
        ));
        assert!(!uri_matches(
            "https://example.com",
            "https://example.com",
            Some(UriMatchType::Never),
        ));
    }

    #[test]
    fn base_domain_co_uk() {
        assert_eq!(base_domain("www.bbc.co.uk"), "bbc.co.uk");
        assert_eq!(base_domain("github.com"), "github.com");
    }

    #[test]
    fn google_equivalent_youtube() {
        assert!(uri_matches(
            "https://www.youtube.com/signin",
            "https://accounts.google.com/",
            Some(UriMatchType::Domain),
        ));
        assert!(uri_matches(
            "https://accounts.google.com/v3/signin",
            "https://google.com",
            None,
        ));
    }
}
