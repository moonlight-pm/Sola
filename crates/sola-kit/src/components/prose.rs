//! Prose — letter-like reading: paragraphs, quotes, inline links.
//!
//! Mail (and any other long-form pane) should render through this instead
//! of a `text_editor`. Tokens only; no hex. Links are accent + underline
//! and emit `on_link` on click.

use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{column, container, rich_text, row, span, Space};
use iced::{Background, Border, Color, Element, Length, Theme};

use crate::components::style::{mix_white, HAIRLINE_A, SPACE_LG, SPACE_MD};
use crate::components::text::PROSE_SIZE;
use crate::fonts;

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

/// Render `blocks` as a reading column. `on_link` receives the URL.
pub fn prose<'a, Message: Clone + 'a>(
    blocks: impl IntoIterator<Item = ProseBlock>,
    theme: &Theme,
    on_link: impl Fn(String) -> Message + Clone + 'a,
) -> Element<'a, Message, Theme> {
    let palette = theme.extended_palette();
    let link = palette.primary.base.color;
    let quote_ink = palette.secondary.base.text;
    let mut col = column![].spacing(SPACE_LG).width(Length::Fill);
    for block in blocks {
        match block {
            ProseBlock::Paragraph(runs) => {
                col = col.push(rich_runs(runs, None, link, on_link.clone()));
            }
            ProseBlock::Quote(runs) => {
                col = col.push(quote_block(runs, quote_ink, link, on_link.clone()));
            }
        }
    }
    col.into()
}

fn quote_block<'a, Message: Clone + 'a>(
    runs: Vec<ProseRun>,
    quote_ink: Color,
    link: Color,
    on_link: impl Fn(String) -> Message + Clone + 'a,
) -> Element<'a, Message, Theme> {
    row![
        container(Space::new().width(1).height(Length::Fill))
            .width(1)
            .height(Length::Fill)
            .style(quote_rule_style),
        rich_runs(runs, Some(quote_ink), link, on_link),
    ]
    .spacing(SPACE_MD)
    .width(Length::Fill)
    .into()
}

fn rich_runs<'a, Message: Clone + 'a>(
    runs: Vec<ProseRun>,
    ink: Option<Color>,
    link: Color,
    on_link: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message, Theme> {
    if runs.is_empty() {
        return Space::new().width(Length::Fill).height(0).into();
    }
    let spans: Vec<iced::widget::text::Span<'static, String>> = runs
        .into_iter()
        .map(|run| styled_span(run, ink, link))
        .collect();
    rich_text(spans)
        .font(fonts::ui())
        .size(PROSE_SIZE)
        .line_height(LineHeight::Relative(1.45))
        .wrapping(Wrapping::Word)
        .width(Length::Fill)
        .on_link_click(on_link)
        .into()
}

fn styled_span(
    run: ProseRun,
    ink: Option<Color>,
    link: Color,
) -> iced::widget::text::Span<'static, String> {
    let mut s = span(run.text)
        .font(fonts::ui())
        .size(PROSE_SIZE)
        .line_height(LineHeight::Relative(1.45));
    if let Some(url) = run.url {
        s = s.link(url).underline(true).color(link);
    } else if let Some(ink) = ink {
        s = s.color(ink);
    }
    s
}

fn quote_rule_style(theme: &Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    iced::widget::container::Style {
        background: Some(Background::Color(mix_white(
            p.background.base.color,
            HAIRLINE_A + 0.06,
        ))),
        border: Border::default(),
        ..Default::default()
    }
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
        let url_cont = prev_end.is_some_and(|c| {
            matches!(c, '/' | '&' | '=' | '-' | '_' | '%' | '#' | '?')
        }) && next_start.is_some_and(|c| is_url_body(c) && !c.is_uppercase());
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
        || url.len() > 96
    {
        return "Link".into();
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
                assert!(runs.iter().any(|r| r.url.as_deref() == Some("https://ex.com/a")));
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
}
