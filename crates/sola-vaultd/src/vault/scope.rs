//! Which passkey rp id a page may use, decided from the frame's origin as
//! CEF reports it, never from what the page says.

use url::Url;

use super::match_uri::registrable_domain;

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

/// The origin's host: WebAuthn's rp id when the options name none.
pub fn origin_host(origin: &str) -> Option<String> {
    Url::parse(origin).ok()?.host_str().map(str::to_ascii_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn origin_host_is_lowercase() {
        assert_eq!(origin_host("https://Login.Example.com:8443").as_deref(), Some("login.example.com"));
        assert_eq!(origin_host("about:blank"), None);
    }
}
