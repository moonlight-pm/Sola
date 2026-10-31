//! Bitwarden URI matching for autofill candidate selection.
//!
//! Follows the official clients' `LoginUriView.matchesUri`; a stored URI
//! with no match type uses Domain. Domain compares registrable domains by
//! the public suffix list, private domains included (so `a.github.io` and
//! `b.github.io` differ), widened by four of Bitwarden's global
//! equivalent-domain groups. Sync-driven equivalent domains can come later.

use bitwarden_vault::UriMatchType;
use public_suffix::{DEFAULT_PROVIDER, EffectiveTLDProvider};
use regex::RegexBuilder;
use url::{Host, Url};

/// Bitwarden's global equivalent-domain groups for Google, Apple, Microsoft
/// and Amazon, minus entries that are not a registrable domain (they could
/// never match). Each inner list is one equivalence class.
const EQUIVALENT_DOMAIN_GROUPS: &[&[&str]] = &[
    &["youtube.com", "google.com", "gmail.com"],
    &["apple.com", "icloud.com"],
    &[
        "bing.com",
        "hotmail.com",
        "live.com",
        "microsoft.com",
        "msn.com",
        "passport.net",
        "windows.com",
        "microsoftonline.com",
        "office.com",
        "office365.com",
        "microsoftstore.com",
        "xbox.com",
        "azure.com",
        "windowsazure.com",
        "cloud.microsoft",
    ],
    &[
        "amazon.com",
        "amazon.ae",
        "amazon.ca",
        "amazon.co.uk",
        "amazon.com.au",
        "amazon.com.br",
        "amazon.com.mx",
        "amazon.com.tr",
        "amazon.de",
        "amazon.es",
        "amazon.fr",
        "amazon.in",
        "amazon.it",
        "amazon.nl",
        "amazon.pl",
        "amazon.sa",
        "amazon.se",
        "amazon.sg",
    ],
];

/// Bitwarden's `DomainMatchBlacklist`: page hosts that never Domain-match a
/// login saved on that domain (`script.google.com` serves anyone's pages).
const DOMAIN_MATCH_BLACKLIST: &[(&str, &[&str])] = &[("google.com", &["script.google.com"])];

/// Compiled-size cap for a stored regular expression (vault data).
const REGEX_SIZE_LIMIT: usize = 1 << 20;

/// Returns true when `page_url` should be considered a match for a stored
/// login URI under the given match type (`None` → Domain). StartsWith and
/// Exact compare the raw strings; Host compares host plus explicit port;
/// RegularExpression tests the whole page URL, ignoring case.
pub fn uri_matches(page_url: &str, login_uri: &str, match_type: Option<UriMatchType>) -> bool {
    let stored = login_uri.trim();
    if page_url.is_empty() || stored.is_empty() {
        return false;
    }
    match match_type.unwrap_or(UriMatchType::Domain) {
        UriMatchType::Domain => domain_matches(page_url, stored),
        UriMatchType::Host => url_host(page_url).is_some_and(|h| url_host(stored) == Some(h)),
        UriMatchType::StartsWith => page_url.starts_with(stored),
        UriMatchType::Exact => page_url == stored,
        UriMatchType::RegularExpression => RegexBuilder::new(stored)
            .case_insensitive(true)
            .size_limit(REGEX_SIZE_LIMIT)
            .build()
            .is_ok_and(|re| re.is_match(page_url)),
        UriMatchType::Never => false,
    }
}

fn domain_matches(page_url: &str, stored: &str) -> bool {
    let (Some(page), Some(saved)) = (url_domain(page_url), url_domain(stored)) else {
        return false;
    };
    if page != saved && !equivalent_domains(&page, &saved) {
        return false;
    }
    let host = url_host(page_url);
    !DOMAIN_MATCH_BLACKLIST.iter().any(|(domain, hosts)| {
        *domain == saved && host.as_deref().is_some_and(|h| hosts.contains(&h))
    })
}

fn equivalent_domains(a: &str, b: &str) -> bool {
    EQUIVALENT_DOMAIN_GROUPS
        .iter()
        .any(|group| group.contains(&a) && group.contains(&b))
}

/// Bitwarden's `Utils.getUrl`: without `://`, a string with a dot (a bare
/// `example.com`) is read as `http://…`; anything else is no URL.
fn parse_url(s: &str) -> Option<Url> {
    let s = s.trim();
    if s.contains("://") {
        Url::parse(s).ok()
    } else if s.contains('.') {
        Url::parse(&format!("http://{s}")).ok()
    } else {
        None
    }
}

/// Bitwarden's `Utils.getHost` (the URL's `host`): hostname plus the port
/// when it is not the scheme's default.
fn url_host(s: &str) -> Option<String> {
    let url = parse_url(s)?;
    let host = url.host_str().filter(|h| !h.is_empty())?;
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    })
}

fn url_domain(s: &str) -> Option<String> {
    registrable_domain(parse_url(s)?.host_str()?)
}

/// Registrable domain (eTLD+1) of `host` by the public suffix list, private
/// domains included: `www.bbc.co.uk` → `bbc.co.uk`, `a.b.github.io` →
/// `b.github.io`. An IP address or a single-label host (`localhost`) is its
/// own domain. `None` for a public suffix itself (`github.io`, `co.uk`) or
/// a malformed host.
pub fn registrable_domain(host: &str) -> Option<String> {
    match Host::parse(host.trim_end_matches('.')).ok()? {
        Host::Domain(d) if d.contains('.') => DEFAULT_PROVIDER
            .effective_tld_plus_one(&d)
            .ok()
            .map(str::to_owned),
        h => Some(h.to_string()),
    }
}

/// Registrable domain of `page_url` (bare host, no scheme), to prefill a new
/// login. Empty when the URL is blank, has no host, or its host is a public
/// suffix.
pub fn apex_domain(page_url: &str) -> String {
    url_domain(page_url).unwrap_or_default()
}

/// A page URL as the log may carry it: the host only. Queries hold OAuth
/// codes and other one-time secrets.
pub fn log_host(url: &str) -> String {
    Url::parse(url.trim())
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain(page: &str, stored: &str) -> bool {
        uri_matches(page, stored, None)
    }

    fn host(page: &str, stored: &str) -> bool {
        uri_matches(page, stored, Some(UriMatchType::Host))
    }

    fn starts(page: &str, stored: &str) -> bool {
        uri_matches(page, stored, Some(UriMatchType::StartsWith))
    }

    fn exact(page: &str, stored: &str) -> bool {
        uri_matches(page, stored, Some(UriMatchType::Exact))
    }

    fn regex(page: &str, stored: &str) -> bool {
        uri_matches(page, stored, Some(UriMatchType::RegularExpression))
    }

    #[test]
    fn domain_match_subdomains() {
        assert!(uri_matches(
            "https://accounts.google.com/signin",
            "https://google.com",
            Some(UriMatchType::Domain),
        ));
        assert!(domain("https://github.com/login", "www.github.com"));
    }

    #[test]
    fn domain_match_uses_private_suffixes() {
        assert!(!domain("https://evil.github.io/", "victim.github.io"));
        assert!(!domain("https://a.herokuapp.com/", "b.herokuapp.com"));
        assert!(domain("https://x.me.github.io/", "me.github.io"));
    }

    #[test]
    fn domain_match_uses_icann_suffixes() {
        assert!(domain("https://www.gmx.de/", "https://gmx.de"));
        assert!(domain("https://banking.ing.de/", "ing.de"));
        assert!(domain("https://www.bbc.co.uk/", "bbc.co.uk"));
        assert!(!domain("https://evil.co.uk/", "bbc.co.uk"));
    }

    #[test]
    fn domain_match_ip_and_single_label() {
        assert!(domain("http://localhost:3000/", "http://localhost"));
        assert!(domain("http://10.0.0.1/a", "http://10.0.0.1:8443"));
        assert!(!domain("http://10.0.0.1/", "http://20.0.0.1/"));
    }

    #[test]
    fn domain_match_blacklist() {
        assert!(!domain("https://script.google.com/", "google.com"));
        assert!(domain("https://docs.google.com/", "google.com"));
    }

    #[test]
    fn scheme_less_stored_uri() {
        assert!(domain("https://www.example.com/x", "example.com"));
        assert!(host("https://example.com/x", "example.com"));
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
    fn host_match_ignores_scheme_not_explicit_port() {
        assert!(host("http://a.com/", "https://a.com"));
        assert!(host("https://a.com:443/", "https://a.com"));
        assert!(!host("https://a.com:8443/", "https://a.com"));
        assert!(host("https://a.com:8443/x", "a.com:8443"));
    }

    #[test]
    fn starts_with_compares_raw_strings() {
        assert!(!starts("https://a.com.evil.net/", "https://a.com/"));
        assert!(starts("https://a.com/x/y", " https://a.com/x "));
        assert!(!starts("https://a.com/", "https://a.com/x"));
    }

    #[test]
    fn exact_and_never() {
        assert!(exact("https://example.com/a", "https://example.com/a"));
        assert!(!exact("https://example.com/a", "https://example.com/b"));
        assert!(!exact("https://example.com/", "https://example.com"));
        assert!(!uri_matches(
            "https://example.com",
            "https://example.com",
            Some(UriMatchType::Never),
        ));
    }

    #[test]
    fn regular_expression_tests_full_url() {
        let login = r"^https://a\.com/login";
        assert!(regex("https://A.com/login?x=1", login));
        assert!(!regex("https://a.com/other", login));
        assert!(!regex("https://a.com/", "(unclosed"));
        assert!(!regex("https://a.com/", "a{100000}{100000}"));
    }

    #[test]
    fn google_equivalent_youtube() {
        assert!(uri_matches(
            "https://www.youtube.com/signin",
            "https://accounts.google.com/",
            Some(UriMatchType::Domain),
        ));
        assert!(domain("https://accounts.google.com/", "google.com"));
    }

    #[test]
    fn equivalent_groups_do_not_over_match() {
        assert!(!domain("https://storage.googleapis.com/", "google.com"));
        for group in EQUIVALENT_DOMAIN_GROUPS {
            for d in *group {
                assert_eq!(registrable_domain(d).as_deref(), Some(*d));
            }
        }
    }

    #[test]
    fn registrable_domains() {
        let rd = |host| registrable_domain(host);
        assert_eq!(rd("www.bbc.co.uk").as_deref(), Some("bbc.co.uk"));
        assert_eq!(rd("a.b.github.io").as_deref(), Some("b.github.io"));
        assert_eq!(rd("Example.COM.").as_deref(), Some("example.com"));
        assert_eq!(rd("localhost").as_deref(), Some("localhost"));
        assert_eq!(rd("[::1]").as_deref(), Some("[::1]"));
        assert_eq!(rd("github.io"), None);
        assert_eq!(rd("co.uk"), None);
        assert_eq!(rd(""), None);
    }

    #[test]
    fn apex_from_page_url() {
        assert_eq!(
            apex_domain("https://accounts.google.com/signup?x=1"),
            "google.com"
        );
        assert_eq!(apex_domain("github.com/login"), "github.com");
        assert_eq!(apex_domain("https://me.github.io/"), "me.github.io");
        assert_eq!(apex_domain(""), "");
        assert_eq!(apex_domain("about:blank"), "");
    }
}
