//! Where vault traffic may go: which frames of a page a fill runs in, and
//! which passkey rpId a frame may ask for. Both are decided from the URL
//! of the frame itself (helper side), never from what the page says.

use bitwarden_vault::UriMatchType;
use url::Url;

use super::match_uri::{registrable_domain, uri_matches};
use crate::engine::SiteUri;

/// `scheme://host[:port]` of `url`; `None` for opaque origins
/// (`about:blank`, `data:`, `file:`).
pub fn origin_of(url: &str) -> Option<String> {
    let origin = Url::parse(url).ok()?.origin();
    origin.is_tuple().then(|| origin.ascii_serialization())
}

/// A login website for [`crate::engine::FrameScope::Site`].
pub fn site_uri(uri: &str, match_type: Option<UriMatchType>) -> SiteUri {
    SiteUri {
        uri: uri.to_string(),
        match_type: match_type.map(|m| m as u8),
    }
}

fn match_type(code: Option<u8>) -> Option<UriMatchType> {
    Some(match code? {
        0 => UriMatchType::Domain,
        1 => UriMatchType::Host,
        2 => UriMatchType::StartsWith,
        3 => UriMatchType::Exact,
        4 => UriMatchType::RegularExpression,
        _ => UriMatchType::Never,
    })
}

/// A fill for a page on `origin` may run in the frame at `frame_url`: the
/// same origin, or a frame the login's own websites match.
pub fn frame_in_site(origin: &str, uris: &[SiteUri], frame_url: &str) -> bool {
    origin_of(frame_url).as_deref() == Some(origin)
        || uris
            .iter()
            .any(|u| uri_matches(frame_url, &u.uri, match_type(u.match_type)))
}

/// WebAuthn: a frame on `origin` may use `rp_id` when the origin is a
/// secure context and `rp_id` is its host or a parent domain that is not
/// a public suffix (`login.example.com` → `example.com`, never `com`).
pub fn rp_id_allowed(origin: &str, rp_id: &str) -> bool {
    let Ok(url) = Url::parse(origin) else {
        return false;
    };
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    let local = host == "localhost" || host.ends_with(".localhost");
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return false;
    }
    let rp = rp_id.trim().to_ascii_lowercase();
    if rp.is_empty() || (host != rp && !host.ends_with(&format!(".{rp}"))) {
        return false;
    }
    match registrable_domain(&host) {
        Some(site) => rp == site || rp.ends_with(&format!(".{site}")),
        None => rp == host,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_drops_path_and_default_port() {
        assert_eq!(
            origin_of("https://a.example.com:443/x?y").as_deref(),
            Some("https://a.example.com")
        );
        assert_eq!(
            origin_of("http://localhost:8080/").as_deref(),
            Some("http://localhost:8080")
        );
        assert_eq!(origin_of("about:blank"), None);
    }

    #[test]
    fn fill_reaches_same_origin_and_matching_frames_only() {
        let uris = [site_uri("https://idmsa.apple.com", Some(UriMatchType::Host))];
        let origin = "https://www.icloud.com";
        assert!(frame_in_site(origin, &uris, "https://www.icloud.com/frame"));
        assert!(frame_in_site(origin, &uris, "https://idmsa.apple.com/auth"));
        assert!(!frame_in_site(origin, &uris, "https://ads.example.net/x"));
        assert!(!frame_in_site(origin, &[], "https://icloud.com/"));
    }

    #[test]
    fn rp_id_is_the_host_or_a_parent_on_a_secure_origin() {
        assert!(rp_id_allowed("https://accounts.google.com", "google.com"));
        assert!(rp_id_allowed("https://accounts.google.com", "accounts.google.com"));
        assert!(!rp_id_allowed("https://evil.com", "google.com"));
        assert!(!rp_id_allowed("https://google.com.evil.net", "google.com"));
        assert!(!rp_id_allowed("https://accounts.google.com", "com"));
        assert!(!rp_id_allowed("http://accounts.google.com", "google.com"));
        assert!(rp_id_allowed("http://localhost:3000", "localhost"));
    }
}
