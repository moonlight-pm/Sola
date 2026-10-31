//! The slate base Sola shipped until Next (2026-10-23) and the neutral
//! greys that replaced it. A theme saved before then still carries slate;
//! [`Theme::retire_slate`] swaps each untouched slate value for its grey
//! and leaves every color the user picked alone.

use super::types::{Theme, TokenKind};

/// Each retired slate value and its grey: surfaces and lines at 0.8× their
/// CIE L*, text at the same L* (`design/next/next.css`).
const SLATE_TO_GREY: &[(&str, &str)] = &[
    ("#0c0e12", "#0b0b0b"),
    ("#151922", "#151515"),
    ("#1e2533", "#1f1f1f"),
    ("#2a3344", "#2a2a2a"),
    ("#2c333e", "#292929"),
    ("#050608", "#050505"),
    ("#e9ecf2", "#ececec"),
    ("#8b94a8", "#949494"),
    ("#6b7385", "#737373"),
];

/// `value`'s grey when its color part is a retired slate value (alpha
/// kept), else `None`.
fn grey_for(value: &str) -> Option<String> {
    let lower = value.trim().to_ascii_lowercase();
    if lower.len() != 7 && lower.len() != 9 {
        return None;
    }
    let (rgb, alpha) = lower.split_at(7);
    SLATE_TO_GREY
        .iter()
        .find(|(slate, _)| *slate == rgb)
        .map(|(_, grey)| format!("{grey}{alpha}"))
}

impl Theme {
    /// Swap every color token still on a retired slate value for its grey.
    /// Returns true when a token changed.
    pub fn retire_slate(&mut self) -> bool {
        let mut changed = false;
        for token in self.palette.tokens.values_mut() {
            if token.kind != TokenKind::Color {
                continue;
            }
            if let Some(grey) = grey_for(&token.value) {
                token.value = grey;
                changed = true;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Token;

    fn with(name: &str, value: &str) -> Theme {
        let mut theme = Theme::default();
        theme
            .palette
            .tokens
            .insert(name.into(), Token::new(TokenKind::Color, value, &["surface"]));
        theme
    }

    #[test]
    fn seed_has_no_slate() {
        assert!(!Theme::default().retire_slate());
    }

    #[test]
    fn slate_turns_grey_keeping_alpha() {
        let mut theme = with("bg-primary", "#0C0E12");
        theme
            .palette
            .tokens
            .insert("shell-switcher-bg".into(), Token::new(TokenKind::Color, "#151922e6", &["shell"]));
        assert!(theme.retire_slate());
        assert_eq!(theme.palette.tokens["bg-primary"].value, "#0b0b0b");
        assert_eq!(theme.palette.tokens["shell-switcher-bg"].value, "#151515e6");
        assert!(!theme.retire_slate());
    }

    #[test]
    fn picked_colors_stay() {
        let mut theme = with("bg-primary", "#101820");
        assert!(!theme.retire_slate());
        assert_eq!(theme.palette.tokens["bg-primary"].value, "#101820");
    }
}
