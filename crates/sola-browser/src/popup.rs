//! `window.open` / `target=_blank` policy for CEF `on_before_popup`.
//!
//! Native OS popup windows are never created. A real URL becomes a chrome
//! tab, except `NEW_POPUP` / `about:blank` which stay a windowless CEF
//! browser so `window.open` still returns a `Window` (Slack huddle,
//! Cloudways consoles that write into the popup). Chrome adopts that
//! engine tab. Wrappers send off-site http(s) to sola-browser.

use crate::util::href_is_new_tab_target;

/// What the life-span handler should do. Never maps a native window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupAction {
    /// Cancel the native window; chrome (or the wrapper outbound queue)
    /// opens this URL as a tab.
    ChromeTab { activate: bool },
    /// Allow a windowless CEF browser. Chrome / the wrapper paints it.
    Osr,
    /// Cancel and open nothing.
    Cancel,
}

/// CEF `WindowOpenDisposition` without taking a `cef` dependency here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupDisposition {
    BackgroundTab,
    ForegroundTab,
    Popup,
    Window,
    Ignore,
    Other,
}

pub fn classify_popup(
    opener: &str,
    url: &str,
    disposition: PopupDisposition,
    wrapper: bool,
) -> PopupAction {
    let url = url.trim();
    if is_junk_popup_url(url) {
        return PopupAction::Cancel;
    }
    if matches!(disposition, PopupDisposition::Ignore) {
        return PopupAction::Cancel;
    }
    if wrapper && is_offsite_http(opener, url) {
        return PopupAction::ChromeTab { activate: true };
    }
    if is_blank_popup_url(url) || is_devtools_url(url) {
        return PopupAction::Osr;
    }
    // `window.open(url, name, 'width=…')` is NEW_POPUP. Cancelling it
    // makes `window.open` return null and the site does nothing (Slack
    // huddle, Cloudways database / SSH consoles).
    if matches!(disposition, PopupDisposition::Popup) {
        return PopupAction::Osr;
    }
    if wrapper && matches!(disposition, PopupDisposition::Window) {
        return PopupAction::Osr;
    }
    if !href_is_new_tab_target(url) {
        return PopupAction::Cancel;
    }
    PopupAction::ChromeTab {
        activate: !matches!(disposition, PopupDisposition::BackgroundTab),
    }
}

pub fn is_devtools_url(url: &str) -> bool {
    let t = url.trim().to_ascii_lowercase();
    t.starts_with("devtools:") || t.contains("/devtools/inspector.html")
}

pub fn is_blank_popup_url(url: &str) -> bool {
    let t = url.trim();
    t.is_empty() || t.eq_ignore_ascii_case("about:blank") || {
        let lower = t.to_ascii_lowercase();
        lower.starts_with("about:blank?") || lower.starts_with("about:blank#")
    }
}

fn is_junk_popup_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }
    lower.starts_with("javascript:") || lower.starts_with("data:") || lower.starts_with("mailto:")
}

pub fn is_offsite_http(opener: &str, target: &str) -> bool {
    let t = target.trim();
    let lower = t.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return false;
    }
    offsite_hosts(opener, t)
}

fn offsite_hosts(a: &str, b: &str) -> bool {
    match (popup_host(a), popup_host(b)) {
        (Some(ha), Some(hb)) => !ha.eq_ignore_ascii_case(&hb) && popup_apex(&ha) != popup_apex(&hb),
        _ => true,
    }
}

fn popup_host(url: &str) -> Option<String> {
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

fn popup_apex(host: &str) -> &str {
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

    fn act(opener: &str, url: &str, disposition: PopupDisposition, wrapper: bool) -> PopupAction {
        classify_popup(opener, url, disposition, wrapper)
    }

    #[test]
    fn cloudways_console_is_osr_so_window_open_returns() {
        assert_eq!(
            act(
                "https://platform.cloudways.com/server/1",
                "https://platform.cloudways.com/db-console?id=1",
                PopupDisposition::Popup,
                false,
            ),
            PopupAction::Osr
        );
        assert_eq!(
            act(
                "https://platform.cloudways.com/",
                "https://ssh.cloudways.com/term",
                PopupDisposition::Popup,
                false,
            ),
            PopupAction::Osr
        );
    }

    #[test]
    fn cmd_click_stays_a_background_tab() {
        assert_eq!(
            act(
                "https://imdb.com/title/tt1",
                "https://imdb.com/title/tt2",
                PopupDisposition::BackgroundTab,
                false,
            ),
            PopupAction::ChromeTab { activate: false }
        );
    }

    #[test]
    fn target_blank_is_a_foreground_tab() {
        assert_eq!(
            act(
                "https://example.com/",
                "https://example.com/help",
                PopupDisposition::ForegroundTab,
                false,
            ),
            PopupAction::ChromeTab { activate: true }
        );
    }

    #[test]
    fn new_window_without_features_is_a_foreground_tab() {
        assert_eq!(
            act(
                "https://example.com/",
                "https://example.com/print",
                PopupDisposition::Window,
                false,
            ),
            PopupAction::ChromeTab { activate: true }
        );
    }

    #[test]
    fn devtools_popup_is_osr() {
        assert_eq!(
            act(
                "https://example.com/",
                "devtools://devtools/bundled/inspector.html",
                PopupDisposition::Window,
                false,
            ),
            PopupAction::Osr
        );
        assert!(is_devtools_url(
            "devtools://devtools/bundled/devtools_app.html?remoteBase=https://chrome-devtools-frontend.appspot.com/serve_file/@e46e70b7112e24cb0501b746c09f8228ff88850a/&targetType=tab"
        ));
    }

    #[test]
    fn blank_popup_is_osr_browser_and_wrapper() {
        for wrapper in [false, true] {
            assert_eq!(
                act(
                    "https://app.slack.com/client",
                    "about:blank",
                    PopupDisposition::Popup,
                    wrapper,
                ),
                PopupAction::Osr
            );
            assert_eq!(
                act(
                    "https://app.slack.com/client",
                    "",
                    PopupDisposition::Popup,
                    wrapper
                ),
                PopupAction::Osr
            );
        }
    }

    #[test]
    fn wrapper_offsite_goes_to_the_product_browser() {
        assert_eq!(
            act(
                "https://illuno.slack.com/",
                "https://github.com/sola",
                PopupDisposition::Popup,
                true,
            ),
            PopupAction::ChromeTab { activate: true }
        );
        assert_eq!(
            act(
                "https://illuno.slack.com/",
                "https://github.com/sola",
                PopupDisposition::ForegroundTab,
                true,
            ),
            PopupAction::ChromeTab { activate: true }
        );
    }

    #[test]
    fn wrapper_same_site_window_stays_osr() {
        assert_eq!(
            act(
                "https://illuno.slack.com/",
                "https://files.slack.com/files-pri/x",
                PopupDisposition::Window,
                true,
            ),
            PopupAction::Osr
        );
    }

    #[test]
    fn junk_urls_are_cancelled() {
        for url in ["javascript:void(0)", "data:text/html,hi", "mailto:a@b.c"] {
            assert_eq!(
                act("https://example.com/", url, PopupDisposition::Popup, false),
                PopupAction::Cancel
            );
        }
    }

    #[test]
    fn ignore_and_save_are_cancelled() {
        assert_eq!(
            act(
                "https://example.com/",
                "https://example.com/file.pdf",
                PopupDisposition::Ignore,
                false,
            ),
            PopupAction::Cancel
        );
    }

    #[test]
    fn offsite_uses_registrable_domain() {
        assert!(!is_offsite_http(
            "https://platform.cloudways.com/",
            "https://ssh.cloudways.com/term"
        ));
        assert!(is_offsite_http(
            "https://illuno.slack.com/",
            "https://github.com/sola"
        ));
    }
}
