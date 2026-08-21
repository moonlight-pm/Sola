//! Highlighted JSON — theme-token spans for inspector payloads.
//!
//! Keys use primary text (not accent — neon stays sparse). Strings are
//! success, numbers warning, `true`/`false`/`null` accent, punctuation
//! muted. Callers pass the live [`Theme`] so a bus theme swap recolors
//! the next frame.

use iced::widget::text::{Span, Wrapping};
use iced::widget::{rich_text, span, text};
use iced::{Color, Element, Length, Never, Theme};

use crate::components::text as kit_text;
use crate::fonts;

/// Token classes produced by [`tokenize`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Punct,
    Key,
    String,
    Number,
    Literal,
    Other,
}

/// Best-effort JSON tokenizer. Assumes `src` is already valid JSON
/// (pretty-printed `serde_json` output). Malformed input yields
/// visually-wrong spans, not a panic.
pub fn tokenize(src: &str) -> Vec<(TokenKind, String)> {
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'{' | b'}' | b'[' | b']' | b',' | b':' => {
                out.push((TokenKind::Punct, String::from(b as char)));
                i += 1;
            }
            b'"' => {
                let start = i;
                i += 1;
                let mut escaped = false;
                while i < bytes.len() {
                    let c = bytes[i];
                    i += 1;
                    if escaped {
                        escaped = false;
                    } else if c == b'\\' {
                        escaped = true;
                    } else if c == b'"' {
                        break;
                    }
                }
                let s = String::from_utf8_lossy(&bytes[start..i]).into_owned();
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                let kind = if j < bytes.len() && bytes[j] == b':' {
                    TokenKind::Key
                } else {
                    TokenKind::String
                };
                out.push((kind, s));
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && matches!(bytes[i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                {
                    i += 1;
                }
                out.push((
                    TokenKind::Number,
                    String::from_utf8_lossy(&bytes[start..i]).into_owned(),
                ));
            }
            b't' | b'f' | b'n' => {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                out.push((
                    TokenKind::Literal,
                    String::from_utf8_lossy(&bytes[start..i]).into_owned(),
                ));
            }
            _ => {
                let start = i;
                while i < bytes.len()
                    && !matches!(
                        bytes[i],
                        b'{' | b'}' | b'[' | b']' | b',' | b':' | b'"' | b'-' | b'0'
                            ..=b'9' | b't' | b'f' | b'n'
                    )
                {
                    i += 1;
                }
                if i > start {
                    out.push((
                        TokenKind::Other,
                        String::from_utf8_lossy(&bytes[start..i]).into_owned(),
                    ));
                }
            }
        }
    }
    out
}

fn color_for(kind: TokenKind, theme: &Theme) -> Color {
    let p = theme.extended_palette();
    match kind {
        TokenKind::Punct | TokenKind::Other => p.secondary.base.text,
        TokenKind::Key => p.background.base.text,
        TokenKind::String => p.success.base.color,
        TokenKind::Number => p.warning.base.color,
        TokenKind::Literal => p.primary.base.color,
    }
}

fn spans(src: &str, theme: &Theme) -> Vec<Span<'static, Never>> {
    tokenize(src)
        .into_iter()
        .map(|(kind, text)| span(text).color(color_for(kind, theme)))
        .collect()
}

/// Multi-line highlighted JSON (inspector well). Empty `src` is a muted dash.
pub fn pretty<'a, Message: 'a>(src: &str, theme: &Theme) -> Element<'a, Message, Theme> {
    if src.is_empty() {
        return kit_text::code("—").style(kit_text::muted).into();
    }
    rich_text(spans(src, theme))
        .font(fonts::mono())
        .size(12)
        .on_link_click(iced::never)
        .into()
}

/// Single-line highlighted preview. Clips at the cell edge.
pub fn line<'a, Message: 'a>(src: &str, theme: &Theme) -> Element<'a, Message, Theme> {
    if src.is_empty() {
        return text("—")
            .font(fonts::mono())
            .size(12)
            .style(kit_text::muted)
            .into();
    }
    rich_text(spans(src, theme))
        .font(fonts::mono())
        .size(12)
        .wrapping(Wrapping::None)
        .width(Length::Fill)
        .on_link_click(iced::never)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_vs_strings() {
        let toks = tokenize(r#"{"a": "b", "n": 1, "ok": true, "z": null}"#);
        let kinds: Vec<TokenKind> = toks.iter().map(|(k, _)| *k).collect();
        assert!(kinds.contains(&TokenKind::Key));
        assert!(kinds.contains(&TokenKind::String));
        assert!(kinds.contains(&TokenKind::Number));
        assert!(kinds.contains(&TokenKind::Literal));
        let keys: Vec<&str> = toks
            .iter()
            .filter(|(k, _)| *k == TokenKind::Key)
            .map(|(_, s)| s.as_str())
            .collect();
        assert_eq!(keys, [r#""a""#, r#""n""#, r#""ok""#, r#""z""#]);
        let strings: Vec<&str> = toks
            .iter()
            .filter(|(k, _)| *k == TokenKind::String)
            .map(|(_, s)| s.as_str())
            .collect();
        assert_eq!(strings, [r#""b""#]);
    }
}
