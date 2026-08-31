//! Highlighted JSON — same token classes as the storybook JSON page.
//!
//! Keys `jk`, strings `js`, numbers `jn`, literals `jl`, punctuation `jp`.

use crate::dom::Elem;
use crate::markup;

const INSPECT_LINES: usize = 240;
const MONO_12: f32 = 7.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Punct,
    Key,
    String,
    Number,
    Literal,
    Other,
}

pub fn token_class(kind: TokenKind) -> &'static str {
    match kind {
        TokenKind::Punct | TokenKind::Other => "jp",
        TokenKind::Key => "jk",
        TokenKind::String => "js",
        TokenKind::Number => "jn",
        TokenKind::Literal => "jl",
    }
}

/// One-line preview cell (log payload). Tokens stop at `width`.
pub fn preview(next: &mut u32, src: &str, width: f32) -> Elem {
    let mut cell = markup::node(next, &["col-payload"], None, None, "");
    let w = width.round().max(8.0);
    cell.style_attr = Some(format!(
        "width:{w}px;min-width:0px;max-width:{w}px;flex-grow:0;flex-shrink:0;overflow:hidden"
    ));
    if src.is_empty() {
        cell.children
            .push(markup::node(next, &["jp"], None, None, "—"));
        return cell;
    }
    let budget = (width / MONO_12).floor().max(8.0);
    let mut used = 0.0;
    let toks = tokenize(src);
    for (kind, text) in &toks {
        if text.is_empty() {
            continue;
        }
        let n = text.chars().count() as f32;
        if used + n > budget && used > 0.5 {
            cell.children
                .push(markup::node(next, &["jp"], None, None, "…"));
            break;
        }
        let slice = if used + n > budget {
            let keep = ((budget - used).floor() as usize).saturating_sub(1).max(1);
            let s: String = text.chars().take(keep).collect();
            cell.children
                .push(markup::node(next, &[token_class(*kind)], None, None, &s));
            cell.children
                .push(markup::node(next, &["jp"], None, None, "…"));
            break;
        } else {
            text.as_str()
        };
        cell.children
            .push(markup::node(next, &[token_class(*kind)], None, None, slice));
        used += n;
    }
    cell
}

pub fn pretty(next: &mut u32, src: &str) -> Vec<Elem> {
    if src.is_empty() {
        return vec![markup::node(next, &["t-caption"], None, None, "—")];
    }
    let mut lines: Vec<&str> = src.lines().collect();
    let extra = if lines.len() > INSPECT_LINES {
        let n = lines.len() - INSPECT_LINES;
        lines.truncate(INSPECT_LINES);
        Some(n)
    } else {
        None
    };
    let mut out = Vec::new();
    for line in lines {
        let indent = line.chars().take_while(|c| *c == ' ').count();
        let mut row = markup::node(next, &["json-line"], None, None, "");
        if indent > 0 {
            row.style_attr = Some(format!("padding-left:{}px", indent * 8));
        }
        for (kind, text) in tokenize(line.trim_start()) {
            if text.is_empty() {
                continue;
            }
            row.children
                .push(markup::node(next, &[token_class(kind)], None, None, &text));
        }
        out.push(row);
    }
    if let Some(n) = extra {
        out.push(markup::node(
            next,
            &["t-caption"],
            None,
            None,
            &format!("… {n} more lines"),
        ));
    }
    out
}

/// Best-effort JSON tokenizer. Assumes pretty/compact `serde_json` output.
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
