//! Pure string helpers for the browser chrome.

use crate::engine::EditCmd;

/// Whether a link click should open a background tab instead of navigating
/// in place: a left click (button 1) with Ctrl or Super held. Middle-click is
/// intentionally inert in Sola — it is filtered before it ever reaches the
/// engine, so it never opens a tab.
pub fn is_new_tab_click(mouse_button: u32, ctrl: bool, super_key: bool) -> bool {
    mouse_button == 1 && (ctrl || super_key)
}

/// Whether a hit-tested `href` should open as a background tab.
/// Drops empty / `javascript:` / `data:` so a miss can fall through to a
/// normal click instead of spawning a junk tab.
pub fn href_is_new_tab_target(href: &str) -> bool {
    let t = href.trim();
    if t.is_empty() {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("javascript:") || lower.starts_with("data:") {
        return false;
    }
    true
}

/// The WebKit editing-command string for an [`EditCmd`]. WebKit command
/// names are case-sensitive.
pub fn editing_command_name(cmd: EditCmd) -> &'static str {
    match cmd {
        EditCmd::Copy => "Copy",
        EditCmd::Cut => "Cut",
        EditCmd::Paste => "Paste",
        EditCmd::SelectAll => "SelectAll",
        EditCmd::Undo => "Undo",
        EditCmd::Redo => "Redo",
    }
}

/// Clipboard text that is safe to apply to a field. Drops `None`, empty,
/// and control-only payloads so a failed / consumed Wayland read cannot
/// wipe the field or get written back as an empty selection.
pub fn usable_clipboard_text(text: Option<String>) -> Option<String> {
    // Keep newline / tab — markdown copy buttons and ⌘C of a block
    // would otherwise flatten to one line. Drop the rest (NUL, etc.).
    let cleaned: String = text?
        .chars()
        .filter(|c| matches!(c, '\n' | '\r' | '\t') || !c.is_control())
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// How many title characters fit a Large-density etch row of `sidebar_w` px.
///
/// Slightly optimistic so the well fills; the kit still clips if a wide
/// glyph run overruns. The old hard cap of 20 left a visible empty band
/// at the default 200 px column.
pub fn tab_title_chars(sidebar_w: f32) -> usize {
    // Body pad 8+8, row pad 10+10, active lip 2, a hair of clip slack.
    const INSET: f32 = 32.0;
    // 12 px SF Pro mixed-case. Optimistic so we use the well, not sit short.
    const PX_PER: f32 = 5.5;
    let inner = (sidebar_w - INSET).max(48.0);
    (inner / PX_PER).floor().max(8.0) as usize
}

/// Tab-strip label: page title, else URL, else a loading placeholder.
/// Ellipsizes to the characters that fit `sidebar_w`.
pub fn tab_strip_label(title: &str, url: &str, sidebar_w: f32) -> String {
    let raw = if !title.is_empty() {
        title
    } else if !url.is_empty() {
        url
    } else {
        return String::from("Loading…");
    };
    truncate(raw, tab_title_chars(sidebar_w))
}

/// Built-in scroll/tile stress page (fixed nav + tall image grid).
pub const SCROLL_STRESS_URL: &str = "sola:scroll-stress";

/// Normalize input into a navigable URL. An explicit scheme (`https:`,
/// `about:`, `mailto:`, `file:`, `sola:` …) is left intact; everything else
/// gets a `https://` prefix. `host:port` (digits after the colon) counts as a
/// bare host, not a scheme, so it is prefixed too.
pub fn normalize_url(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // Shortcuts → built-in stress page.
    let lower = trimmed.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "sola:scroll-stress" | "sola://scroll-stress" | "about:scroll-stress" | "scroll-stress"
    ) {
        return SCROLL_STRESS_URL.to_string();
    }
    if explicit_scheme(trimmed).is_some() {
        return trimmed.to_string();
    }
    format!("https://{trimmed}")
}

/// HTML for [`SCROLL_STRESS_URL`] (embedded asset).
pub fn scroll_stress_html() -> &'static str {
    include_str!("../assets/scroll-stress.html")
}

/// Base URL for chrome input that doesn't parse as a URL — searched on Kagi.
const SEARCH_PREFIX: &str = "https://kagi.com/search?q=";

/// Decide whether chrome input should be loaded as a URL (vs. searched).
/// The common browser-omnibox heuristic, deliberately simple:
/// - whitespace anywhere → search ("how to tie a tie")
/// - an explicit scheme, a dotted host, or localhost → URL
///   ("https://x", "about:blank", "github.com", "localhost:3000")
/// - anything else (a single bare word) → search ("weather", "rust")
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
    if t == "localhost" || t.starts_with("localhost:") || t.starts_with("localhost/") {
        return true;
    }
    // A dotted host like "github.com" or "a.b/c": a dot that is neither the
    // first nor the last character.
    matches!(t.find('.'), Some(i) if i > 0 && i < t.len() - 1)
}

/// Turn chrome input into a final navigation target: the URL itself when it
/// looks like one, otherwise a Kagi search for the text. Empty input yields an
/// empty string (the caller does nothing).
pub fn resolve_query(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return String::new();
    }
    if looks_like_url(t) {
        normalize_url(t)
    } else {
        format!("{SEARCH_PREFIX}{}", encode_query(t))
    }
}

/// Return the explicit URL scheme of `s` (the alphabetic run before the first
/// `:`), but NOT for `host:port`, where the colon is followed only by digits.
fn explicit_scheme(s: &str) -> Option<&str> {
    let colon = s.find(':')?;
    let scheme = &s[..colon];
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    // `host:port` — the segment after the colon (up to any `/`) is all digits.
    let after = &s[colon + 1..];
    let port = after.split('/').next().unwrap_or("");
    if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(scheme)
}

/// Percent-encode `s` as a URL query value: RFC 3986 unreserved bytes pass
/// through, everything else (including spaces) becomes `%XX`.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tab_click_rules() {
        // Left-click with ctrl or super opens a background tab.
        assert!(is_new_tab_click(1, true, false));
        assert!(is_new_tab_click(1, false, true));
        // Plain left-click navigates in place.
        assert!(!is_new_tab_click(1, false, false));
        // Middle-click is inert — never a new tab, with or without modifiers.
        assert!(!is_new_tab_click(2, false, false));
        assert!(!is_new_tab_click(2, true, true));
        // Right-click is the context menu, not a new tab.
        assert!(!is_new_tab_click(3, true, true));
    }

    #[test]
    fn href_is_new_tab_target_rejects_junk() {
        assert!(href_is_new_tab_target("https://imdb.com/title/tt1"));
        assert!(href_is_new_tab_target("https://imdb.com/title/tt1#top"));
        assert!(!href_is_new_tab_target(""));
        assert!(!href_is_new_tab_target("   "));
        assert!(!href_is_new_tab_target("javascript:void(0)"));
        assert!(!href_is_new_tab_target("data:text/html,hi"));
    }

    #[test]
    fn usable_clipboard_text_rejects_empty() {
        assert_eq!(usable_clipboard_text(None), None);
        assert_eq!(usable_clipboard_text(Some(String::new())), None);
        assert_eq!(usable_clipboard_text(Some(" \t\n".into())), None);
        assert_eq!(
            usable_clipboard_text(Some("hello".into())),
            Some("hello".into())
        );
        assert_eq!(
            usable_clipboard_text(Some("a\u{0}b".into())),
            Some("ab".into())
        );
        assert_eq!(
            usable_clipboard_text(Some("a\nb\t c".into())),
            Some("a\nb\t c".into())
        );
    }

    #[test]
    fn editing_command_names_match_webkit() {
        use crate::engine::EditCmd;
        assert_eq!(editing_command_name(EditCmd::Copy), "Copy");
        assert_eq!(editing_command_name(EditCmd::Cut), "Cut");
        assert_eq!(editing_command_name(EditCmd::Paste), "Paste");
        assert_eq!(editing_command_name(EditCmd::SelectAll), "SelectAll");
        assert_eq!(editing_command_name(EditCmd::Undo), "Undo");
        assert_eq!(editing_command_name(EditCmd::Redo), "Redo");
    }

    #[test]
    fn truncate_leaves_short_strings_untouched() {
        assert_eq!(truncate("hi", 20), "hi");
    }

    #[test]
    fn truncate_shortens_long_strings_within_budget() {
        let out = truncate("a very long tab title indeed", 10);
        assert_eq!(out, "a very lo…");
        assert!(out.chars().count() <= 10, "got {out:?}");
    }

    #[test]
    fn tab_title_chars_fills_default_column() {
        // Old hard cap was 20; default 200 px column has room for more.
        let n = tab_title_chars(200.0);
        assert!(n > 20, "default column still capped at {n}");
        assert!(n >= 28, "expected ~30 chars at 200 px, got {n}");
        assert!(tab_title_chars(400.0) > n);
        assert!(tab_title_chars(120.0) >= 8);
    }

    #[test]
    fn tab_strip_label_prefers_title_then_url() {
        assert_eq!(tab_strip_label("Hi", "https://x", 200.0), "Hi");
        assert_eq!(tab_strip_label("", "https://x", 200.0), "https://x");
        assert_eq!(tab_strip_label("", "", 200.0), "Loading…");
        let long = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let out = tab_strip_label(long, "", 200.0);
        assert!(out.ends_with('…'), "{out:?}");
        assert!(out.chars().count() < long.chars().count());
    }

    #[test]
    fn normalize_url_adds_scheme_to_bare_host() {
        assert_eq!(normalize_url("example.com"), "https://example.com");
    }

    #[test]
    fn normalize_url_keeps_explicit_scheme() {
        assert_eq!(normalize_url("https://example.com"), "https://example.com");
        assert_eq!(normalize_url("about:blank"), "about:blank");
        assert_eq!(normalize_url("file:///home/x"), "file:///home/x");
    }

    #[test]
    fn normalize_url_treats_host_port_as_bare_host() {
        assert_eq!(normalize_url("localhost:3000"), "https://localhost:3000");
        assert_eq!(
            normalize_url("192.168.1.1:8080"),
            "https://192.168.1.1:8080"
        );
    }

    #[test]
    fn looks_like_url_accepts_hosts_schemes_and_localhost() {
        assert!(looks_like_url("github.com"));
        assert!(looks_like_url("https://github.com"));
        assert!(looks_like_url("about:blank"));
        assert!(looks_like_url("192.168.1.1"));
        assert!(looks_like_url("localhost"));
        assert!(looks_like_url("localhost:3000"));
    }

    #[test]
    fn looks_like_url_rejects_searches() {
        assert!(!looks_like_url("weather"));
        assert!(!looks_like_url("rust programming language"));
        assert!(!looks_like_url("how to tie a tie"));
        assert!(!looks_like_url(""));
        assert!(!looks_like_url(".")); // lone dot is not a host
    }

    #[test]
    fn resolve_query_navigates_to_urls() {
        assert_eq!(resolve_query("github.com"), "https://github.com");
        assert_eq!(resolve_query("https://slate.auto"), "https://slate.auto");
    }

    #[test]
    fn resolve_query_searches_kagi_for_text() {
        assert_eq!(
            resolve_query("how to tie a tie"),
            "https://kagi.com/search?q=how%20to%20tie%20a%20tie"
        );
        assert_eq!(
            resolve_query("weather"),
            "https://kagi.com/search?q=weather"
        );
    }

    #[test]
    fn resolve_query_encodes_reserved_characters() {
        assert_eq!(
            resolve_query("rust & c++"),
            "https://kagi.com/search?q=rust%20%26%20c%2B%2B"
        );
    }

    #[test]
    fn resolve_query_empty_is_empty() {
        assert_eq!(resolve_query("   "), "");
    }
}
