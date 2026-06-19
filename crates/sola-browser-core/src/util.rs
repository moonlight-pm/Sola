//! Pure string helpers for the browser chrome.

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

pub fn normalize_url(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(colon) = trimmed.find(':') {
        let scheme = &trimmed[..colon];
        if !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphabetic()) {
            return trimmed.to_string();
        }
    }
    format!("https://{trimmed}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_leaves_short_strings_untouched() {
        assert_eq!(truncate("hi", 20), "hi");
    }

    #[test]
    fn truncate_shortens_long_strings_within_budget() {
        let out = truncate("a very long tab title indeed", 10);
        assert!(out.chars().count() <= 10, "got {out:?}");
    }

    #[test]
    fn normalize_url_adds_scheme_to_bare_host() {
        assert!(normalize_url("example.com").starts_with("http"));
    }
}
