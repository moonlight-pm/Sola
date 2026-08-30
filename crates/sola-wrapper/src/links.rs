//! Outbound links from a wrapper page → sola-browser.
//!
//! CEF already cancels native popups (`target=_blank`, `window.open`,
//! ⌘-click) and queues the URL as a "background tab". The wrapper has
//! no tabs — chrome drains that queue and opens http(s) off the start
//! site in sola-browser (`sola_core::open_url`). Same-site URLs stay
//! in-app (SPA / same cookies). Main-frame navigations are not
//! intercepted so SSO redirects (Google in the Slack window) still work.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkAction {
    /// `target=_blank` / popup to another site — product browser.
    Browser,
    /// Same registrable domain as the wrapper start URL.
    InApp,
    /// `javascript:`, `mailto:`, empty, `about:`, …
    Ignore,
}

pub fn classify(start_url: &str, target: &str) -> LinkAction {
    let t = target.trim();
    if t.is_empty() {
        return LinkAction::Ignore;
    }
    let lower = t.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return LinkAction::Ignore;
    }
    if is_same_site(start_url, t) {
        LinkAction::InApp
    } else {
        LinkAction::Browser
    }
}

pub fn open_in_browser(url: &str) {
    tracing::info!(%url, "wrapper: link → sola-browser");
    sola_core::open_url_logged(url);
}

fn is_same_site(start_url: &str, target: &str) -> bool {
    match (host_of(start_url), host_of(target)) {
        (Some(a), Some(b)) => {
            a.eq_ignore_ascii_case(&b) || base_domain(&a).eq_ignore_ascii_case(&base_domain(&b))
        }
        _ => false,
    }
}

fn host_of(url: &str) -> Option<String> {
    let t = url.trim();
    let rest = if t.len() >= 8 && t[..8].eq_ignore_ascii_case("https://") {
        &t[8..]
    } else if t.len() >= 7 && t[..7].eq_ignore_ascii_case("http://") {
        &t[7..]
    } else {
        return None;
    };
    let hostport = rest
        .split(['/', '?', '#'])
        .next()
        .filter(|s| !s.is_empty())?;
    let hostport = hostport.rsplit('@').next()?;
    let host = if let Some(rest) = hostport.strip_prefix('[') {
        rest.split(']').next()?
    } else {
        hostport.split(':').next()?
    };
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

/// Last two labels (`illuno.slack.com` → `slack.com`). Hosts without a
/// dot, and IPv4, stay as-is. Not PSL-complete (same bar as vault match).
fn base_domain(host: &str) -> &str {
    if host.parse::<std::net::Ipv4Addr>().is_ok() {
        return host;
    }
    let mut iter = host.rsplitn(3, '.');
    let Some(tld) = iter.next() else {
        return host;
    };
    match iter.next() {
        Some(sld) => {
            let start = host.len() - sld.len() - 1 - tld.len();
            &host[start..]
        }
        None => host,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_http_opens_in_browser() {
        assert_eq!(
            classify("https://illuno.slack.com", "https://github.com/sola"),
            LinkAction::Browser
        );
        assert_eq!(
            classify(
                "https://illuno.slack.com/",
                "https://slack-redir.net/link?url=https://ex.com"
            ),
            LinkAction::Browser
        );
    }

    #[test]
    fn slack_subdomains_stay_in_app() {
        let start = "https://illuno.slack.com";
        assert_eq!(
            classify(start, "https://app.slack.com/client"),
            LinkAction::InApp
        );
        assert_eq!(
            classify(start, "https://files.slack.com/files-pri/x"),
            LinkAction::InApp
        );
        assert_eq!(
            classify(start, "https://illuno.slack.com/archives/C1"),
            LinkAction::InApp
        );
    }

    #[test]
    fn non_http_ignored() {
        let start = "https://illuno.slack.com";
        assert_eq!(classify(start, ""), LinkAction::Ignore);
        assert_eq!(classify(start, "javascript:void(0)"), LinkAction::Ignore);
        assert_eq!(classify(start, "mailto:a@b.c"), LinkAction::Ignore);
        assert_eq!(classify(start, "about:blank"), LinkAction::Ignore);
        assert_eq!(classify(start, "data:text/plain,hi"), LinkAction::Ignore);
    }

    #[test]
    fn localhost_is_its_own_site() {
        assert_eq!(
            classify("http://localhost:3000/", "http://localhost:3000/app"),
            LinkAction::InApp
        );
        assert_eq!(
            classify("http://localhost:3000/", "https://example.com"),
            LinkAction::Browser
        );
    }

    #[test]
    fn host_strips_userinfo_and_port() {
        assert_eq!(
            host_of("https://user:pw@illuno.slack.com:443/path"),
            Some("illuno.slack.com".into())
        );
        assert_eq!(host_of("HTTP://Example.COM"), Some("example.com".into()));
    }

    #[test]
    fn base_domain_two_labels() {
        assert_eq!(base_domain("illuno.slack.com"), "slack.com");
        assert_eq!(base_domain("slack.com"), "slack.com");
        assert_eq!(base_domain("localhost"), "localhost");
        assert_eq!(base_domain("127.0.0.1"), "127.0.0.1");
    }
}
