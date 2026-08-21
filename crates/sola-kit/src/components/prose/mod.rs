//! Prose — letter-like reading: paragraphs, quotes, inline links.
//!
//! Mail (and any other long-form pane) should render through this instead
//! of a `text_editor`. Tokens only; no hex. Links are accent + underline
//! and emit `on_link` on click. Drag-select copies the visible text.

mod view;

pub use view::{prose, prose_selectable};

/// One styled run inside a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProseRun {
    pub text: String,
    pub url: Option<String>,
}

impl ProseRun {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            url: None,
        }
    }

    pub fn link(text: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            url: Some(url.into()),
        }
    }
}

/// A vertical unit of a letter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProseBlock {
    Paragraph(Vec<ProseRun>),
    Quote(Vec<ProseRun>),
}

/// Parse plain text into paragraphs, `>` quotes, and http(s) link runs.
/// Soft-wrapped URLs (a line break mid-path) are rejoined.
pub fn parse_plain(text: &str) -> Vec<ProseBlock> {
    let mut blocks = Vec::new();
    let mut para_lines: Vec<String> = Vec::new();
    let mut quote_lines: Vec<String> = Vec::new();

    let flush_para = |blocks: &mut Vec<ProseBlock>, lines: &mut Vec<String>| {
        if lines.is_empty() {
            return;
        }
        let body = join_soft_wrap(lines);
        lines.clear();
        if !body.trim().is_empty() {
            blocks.push(ProseBlock::Paragraph(runs_from_text(&body)));
        }
    };
    let flush_quote = |blocks: &mut Vec<ProseBlock>, lines: &mut Vec<String>| {
        if lines.is_empty() {
            return;
        }
        let body = lines.join("\n");
        lines.clear();
        if !body.trim().is_empty() {
            blocks.push(ProseBlock::Quote(runs_from_text(&body)));
        }
    };

    for raw in text.lines() {
        let trimmed = raw.trim_end();
        if trimmed.trim().is_empty() {
            flush_para(&mut blocks, &mut para_lines);
            flush_quote(&mut blocks, &mut quote_lines);
            continue;
        }
        if let Some(rest) = strip_quote_prefix(trimmed) {
            flush_para(&mut blocks, &mut para_lines);
            quote_lines.push(rest.to_string());
        } else {
            flush_quote(&mut blocks, &mut quote_lines);
            para_lines.push(trimmed.to_string());
        }
    }
    flush_para(&mut blocks, &mut para_lines);
    flush_quote(&mut blocks, &mut quote_lines);
    blocks
}

/// Flatten blocks back to copyable plain text. Links become `label <url>`
/// when the visible text is not already the URL.
pub fn flatten(blocks: &[ProseBlock]) -> String {
    let mut out = String::new();
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        match block {
            ProseBlock::Paragraph(runs) => append_runs(&mut out, runs),
            ProseBlock::Quote(runs) => {
                let mut body = String::new();
                append_runs(&mut body, runs);
                let mut first = true;
                for line in body.split('\n') {
                    if !first {
                        out.push('\n');
                    }
                    first = false;
                    out.push_str("> ");
                    out.push_str(line);
                }
            }
        }
    }
    out
}

/// Visible reading text (what the letter shows). Link labels stay labels;
/// quotes do not grow a `>` prefix. Use this for in-body selection copy.
pub fn visible_text(blocks: &[ProseBlock]) -> String {
    let mut out = String::new();
    for line in iter_lines(blocks) {
        out.push_str(&line.text);
        for _ in 0..line.gap {
            out.push('\n');
        }
    }
    out
}

/// Slice of [`visible_text`] for a byte range, snapped to char boundaries.
pub fn selected_visible(blocks: &[ProseBlock], start: usize, end: usize) -> String {
    let doc = visible_text(blocks);
    let a = snap_byte(&doc, start.min(end));
    let b = snap_byte(&doc, start.max(end));
    doc.get(a..b).unwrap_or("").to_string()
}

/// One hard line in the reading column (wrapping is the paragraph's job).
#[derive(Debug, Clone)]
pub(crate) struct LayoutLine {
    pub runs: Vec<ProseRun>,
    pub text: String,
    pub start: usize,
    pub gap: u8,
    pub quote: bool,
}

pub(crate) fn iter_lines(blocks: &[ProseBlock]) -> Vec<LayoutLine> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    for (bi, block) in blocks.iter().enumerate() {
        let (runs, quote) = match block {
            ProseBlock::Paragraph(r) => (r.as_slice(), false),
            ProseBlock::Quote(r) => (r.as_slice(), true),
        };
        let hard = split_runs_on_newline(runs);
        let n = hard.len();
        for (li, runs) in hard.into_iter().enumerate() {
            let text: String = runs.iter().map(|r| r.text.as_str()).collect();
            let last_in_block = li + 1 == n;
            let last_block = bi + 1 == blocks.len();
            let gap = if !last_in_block {
                1
            } else if !last_block {
                2
            } else {
                0
            };
            let start = offset;
            offset += text.len() + gap as usize;
            out.push(LayoutLine {
                runs,
                text,
                start,
                gap,
                quote,
            });
        }
    }
    out
}

pub(crate) fn snap_byte(s: &str, mut i: usize) -> usize {
    if i > s.len() {
        return s.len();
    }
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Word / whitespace cluster around `offset` in `text`.
pub(crate) fn word_at(text: &str, offset: usize) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }
    let offset = snap_byte(text, offset.min(text.len()));
    let probe = if offset == text.len() {
        text.char_indices().next_back().map(|(i, _)| i).unwrap_or(0)
    } else {
        offset
    };
    let ch = text[probe..].chars().next().unwrap_or(' ');
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    if is_word(ch) {
        expand(text, probe, is_word)
    } else if ch.is_whitespace() {
        expand(text, probe, |c| c.is_whitespace())
    } else {
        (probe, probe + ch.len_utf8())
    }
}

fn expand(text: &str, probe: usize, pred: impl Fn(char) -> bool) -> (usize, usize) {
    let ch = text[probe..].chars().next().unwrap_or(' ');
    let mut start = probe;
    for (i, c) in text[..probe].char_indices().rev() {
        if pred(c) {
            start = i;
        } else {
            break;
        }
    }
    let after = probe + ch.len_utf8();
    let mut end = after;
    for (i, c) in text[after..].char_indices() {
        if pred(c) {
            end = after + i + c.len_utf8();
        } else {
            break;
        }
    }
    (start, end)
}

fn split_runs_on_newline(runs: &[ProseRun]) -> Vec<Vec<ProseRun>> {
    let mut lines: Vec<Vec<ProseRun>> = vec![Vec::new()];
    for run in runs {
        let mut rest = run.text.as_str();
        loop {
            match rest.split_once('\n') {
                Some((head, tail)) => {
                    if !head.is_empty() {
                        lines.last_mut().unwrap().push(ProseRun {
                            text: head.to_string(),
                            url: run.url.clone(),
                        });
                    }
                    lines.push(Vec::new());
                    rest = tail;
                }
                None => {
                    if !rest.is_empty() {
                        lines.last_mut().unwrap().push(ProseRun {
                            text: rest.to_string(),
                            url: run.url.clone(),
                        });
                    }
                    break;
                }
            }
        }
    }
    lines.retain(|l| !l.is_empty());
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}

fn strip_quote_prefix(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix('>') {
        Some(rest.strip_prefix(' ').unwrap_or(rest))
    } else {
        None
    }
}

/// Join wrapped paragraph lines, but keep a hard break when the next
/// line looks like a new sentence after a completed one.
fn join_soft_wrap(lines: &[String]) -> String {
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            out.push_str(line);
            continue;
        }
        let prev_end = out.chars().rev().find(|c| !c.is_whitespace());
        let next_start = line.chars().find(|c| !c.is_whitespace());
        // Soft-wrapped URL: "https://ex.com/very/long/\npath" → no space.
        let url_cont = prev_end
            .is_some_and(|c| matches!(c, '/' | '&' | '=' | '-' | '_' | '%' | '#' | '?'))
            && next_start.is_some_and(|c| is_url_body(c) && !c.is_uppercase());
        if url_cont {
            out.push_str(line.trim_start());
            continue;
        }
        let hard = matches!(prev_end, Some('.' | '!' | '?' | ':'))
            && next_start.is_some_and(|c| c.is_uppercase());
        if hard {
            out.push('\n');
            out.push_str(line);
        } else {
            if !out.ends_with([' ', '\n']) && !line.starts_with([' ', '\n']) {
                out.push(' ');
            }
            out.push_str(line);
        }
    }
    out
}

fn runs_from_text(text: &str) -> Vec<ProseRun> {
    let chars: Vec<char> = text.chars().collect();
    let mut runs = Vec::new();
    let mut i = 0;
    let mut buf = String::new();
    while i < chars.len() {
        if let Some((scheme_len, scheme_prefix)) = scheme_at(&chars, i) {
            if !buf.is_empty() {
                runs.push(ProseRun::text(std::mem::take(&mut buf)));
            }
            let start = i;
            i += scheme_len;
            while i < chars.len() {
                let c = chars[i];
                if is_url_body(c) {
                    i += 1;
                    continue;
                }
                if c == '\n' || c == '\r' {
                    let mut k = i;
                    while k < chars.len() && (chars[k] == '\n' || chars[k] == '\r') {
                        k += 1;
                    }
                    while k < chars.len() && (chars[k] == ' ' || chars[k] == '\t') {
                        k += 1;
                    }
                    if k < chars.len()
                        && is_url_body(chars[k])
                        && scheme_at(&chars, k).is_none()
                        && !is_sentence_start(&chars, k)
                    {
                        i = k;
                        continue;
                    }
                }
                break;
            }
            let mut uri = String::new();
            if scheme_prefix.is_empty() {
                uri.push_str("https://");
            }
            let mut r = start;
            while r < i {
                let c = chars[r];
                if c == '\n' || c == '\r' {
                    r += 1;
                    while r < i && (chars[r] == ' ' || chars[r] == '\t') {
                        r += 1;
                    }
                    continue;
                }
                uri.push(c);
                r += 1;
            }
            while uri.chars().last().is_some_and(|c| {
                matches!(
                    c,
                    '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '\'' | '"'
                )
            }) {
                if let Some(c) = uri.pop() {
                    buf.insert(0, c);
                }
            }
            if is_http_url(&uri) {
                let label = short_url_label(&uri);
                // Trailing punct we popped belongs after the link.
                let trail = std::mem::take(&mut buf);
                runs.push(ProseRun::link(label, uri));
                buf = trail;
            } else {
                buf.push_str(&uri);
            }
            continue;
        }
        buf.push(chars[i]);
        i += 1;
    }
    if !buf.is_empty() {
        runs.push(ProseRun::text(buf));
    }
    if runs.is_empty() {
        runs.push(ProseRun::text(text.to_string()));
    }
    runs
}

fn append_runs(out: &mut String, runs: &[ProseRun]) {
    for run in runs {
        match &run.url {
            Some(url) if run.text != *url && !run.text.contains(url) => {
                out.push_str(&run.text);
                out.push_str(" <");
                out.push_str(url);
                out.push('>');
            }
            _ => out.push_str(&run.text),
        }
    }
}

fn scheme_at(chars: &[char], i: usize) -> Option<(usize, &'static str)> {
    if starts_with_ci(chars, i, "https://") {
        return Some((8, "https://"));
    }
    if starts_with_ci(chars, i, "http://") {
        return Some((7, "http://"));
    }
    if starts_with_ci(chars, i, "www.") {
        let boundary = i == 0 || !is_url_body(chars[i - 1]);
        if boundary {
            return Some((4, ""));
        }
    }
    None
}

fn starts_with_ci(chars: &[char], i: usize, prefix: &str) -> bool {
    let pref: Vec<char> = prefix.chars().collect();
    if i + pref.len() > chars.len() {
        return false;
    }
    chars[i..i + pref.len()]
        .iter()
        .zip(pref.iter())
        .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn is_url_body(c: char) -> bool {
    match c {
        ' ' | '\t' | '\n' | '\r' | '<' | '>' | '"' | '\'' | '`' | '{' | '}' | '|' | '\\' | '^'
        | '[' | ']' => false,
        _ => !c.is_control(),
    }
}

fn is_sentence_start(chars: &[char], i: usize) -> bool {
    let c = chars[i];
    if matches!(c, '/' | '?' | '#' | '&' | '=' | '%' | '-' | '_' | '.') {
        return false;
    }
    if c.is_uppercase() {
        if let Some(next) = chars.get(i + 1) {
            return next.is_lowercase() || next.is_whitespace();
        }
    }
    false
}

fn is_http_url(u: &str) -> bool {
    let l = u.to_ascii_lowercase();
    (l.starts_with("http://") || l.starts_with("https://")) && u.len() > 10
}

/// Visible label for a URL: host, or "Link" for tracking / click-wrapper hosts.
fn short_url_label(url: &str) -> String {
    let stripped = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let host = stripped.split('/').next().unwrap_or(stripped);
    let host = host.split(':').next().unwrap_or(host);
    let host = host.trim_start_matches("www.");
    let h = host.to_ascii_lowercase();
    if host.is_empty()
        || h.contains("ablink")
        || h.contains("click")
        || h.contains("track")
        || url.contains("upn=")
    {
        return "Link".into();
    }
    // First-party destinations (magic links, signed URLs): host + path,
    // never collapse a long URL to an anonymous "Link".
    let stripped = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let no_query = stripped.split('?').next().unwrap_or(stripped);
    let no_query = no_query.trim_end_matches('/');
    if !no_query.is_empty() && no_query.len() <= 56 {
        return no_query.to_string();
    }
    host.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_paragraphs_and_quotes() {
        let blocks = parse_plain("Hello there.\n\n> quoted\n> still\n\nBye https://ex.com/a");
        assert_eq!(blocks.len(), 3);
        match &blocks[0] {
            ProseBlock::Paragraph(runs) => assert!(runs[0].text.contains("Hello")),
            _ => panic!("expected paragraph"),
        }
        match &blocks[1] {
            ProseBlock::Quote(runs) => {
                let t: String = runs.iter().map(|r| r.text.as_str()).collect();
                assert!(t.contains("quoted"));
                assert!(!t.contains('>'));
            }
            _ => panic!("expected quote"),
        }
        match &blocks[2] {
            ProseBlock::Paragraph(runs) => {
                assert!(
                    runs.iter()
                        .any(|r| r.url.as_deref() == Some("https://ex.com/a"))
                );
            }
            _ => panic!("expected paragraph"),
        }
    }

    #[test]
    fn rejoins_soft_wrapped_url() {
        let blocks = parse_plain("see https://ex.com/very/long/\npath/here please");
        let ProseBlock::Paragraph(runs) = &blocks[0] else {
            panic!("para");
        };
        assert!(
            runs.iter()
                .any(|r| r.url.as_deref() == Some("https://ex.com/very/long/path/here")),
            "{runs:?}"
        );
    }

    #[test]
    fn flatten_round_trips_quote() {
        let blocks = parse_plain("> hi");
        assert_eq!(flatten(&blocks), "> hi");
    }

    #[test]
    fn visible_text_skips_quote_prefix_and_url_expand() {
        let blocks = parse_plain("Hi\n\n> quoted\n\nsee https://ex.com/a");
        let vis = visible_text(&blocks);
        assert!(vis.contains("Hi"));
        assert!(vis.contains("quoted"));
        assert!(!vis.contains("> quoted"));
        assert!(!vis.contains("<https://"));
        assert_eq!(vis.matches("Hi").count(), 1);
        let lines = iter_lines(&blocks);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].text, "Hi");
        assert_eq!(lines[1].text, "quoted");
        assert!(lines[1].quote);
        assert_eq!(lines[0].start, 0);
        assert_eq!(lines[1].start, "Hi".len() + 2);
    }

    #[test]
    fn selected_visible_snaps_range() {
        let blocks = parse_plain("Hello there");
        assert_eq!(selected_visible(&blocks, 0, 5), "Hello");
        assert_eq!(selected_visible(&blocks, 6, 11), "there");
    }

    #[test]
    fn long_first_party_url_is_not_anonymous_link() {
        let token = "A".repeat(43);
        let url = format!("https://auth.naturalethic.com/login/magic/verify?token={token}");
        let blocks = parse_plain(&format!("Use this link to sign in to Wicket:\n\n  {url}\n"));
        let links: Vec<_> = blocks
            .iter()
            .flat_map(|b| match b {
                ProseBlock::Paragraph(r) | ProseBlock::Quote(r) => r.iter(),
            })
            .filter(|r| r.url.is_some())
            .collect();
        assert_eq!(links.len(), 1, "{blocks:?}");
        assert_eq!(links[0].url.as_deref(), Some(url.as_str()));
        assert_ne!(links[0].text, "Link");
        assert!(links[0].text.contains("auth.naturalethic.com"));
    }

    #[test]
    fn word_at_expands_identifier() {
        let t = "say hello-world";
        assert_eq!(word_at(t, 5), (4, 9));
        assert_eq!(word_at(t, 10), (10, 15));
        assert_eq!(&t[10..15], "world");
    }
}
