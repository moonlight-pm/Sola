//! Chat markdown: headings, emphasis, lists, fences, hard line breaks.
//!
//! Unlike [`super::parse_plain`], a single newline in a paragraph is a
//! hard break (stanzas, lyrics, chat). Mail stays on `parse_plain`.

use super::{ProseBlock, ProseRun, runs_from_text};

pub fn parse_markdown(src: &str) -> Vec<ProseBlock> {
    let normalized = src.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let raw = lines[i];
        let trimmed = raw.trim_end();
        if trimmed.trim().is_empty() {
            i += 1;
            continue;
        }
        if let Some((lang, body, next)) = take_fence(&lines, i) {
            let verse = matches!(
                lang.to_ascii_lowercase().as_str(),
                "verse" | "lyrics" | "lyric" | "poem" | "song"
            );
            blocks.push(ProseBlock::Code { text: body, verse });
            i = next;
            continue;
        }
        if let Some((level, rest)) = heading_line(trimmed) {
            blocks.push(ProseBlock::Heading {
                level,
                runs: parse_inlines(rest),
            });
            i += 1;
            continue;
        }
        if let Some(rest) = quote_line(trimmed) {
            let (body, next) = take_quoted(&lines, i, rest);
            blocks.push(ProseBlock::Quote(parse_inlines(&body)));
            i = next;
            continue;
        }
        if let Some((ordered, item)) = list_line(trimmed) {
            let (items, next) = take_list(&lines, i, ordered, item);
            blocks.push(ProseBlock::List { ordered, items });
            i = next;
            continue;
        }
        let (body, next) = take_paragraph(&lines, i);
        blocks.push(ProseBlock::Paragraph(parse_inlines(&body)));
        i = next;
    }
    if blocks.is_empty() && !src.trim().is_empty() {
        blocks.push(ProseBlock::Paragraph(parse_inlines(src.trim())));
    }
    blocks
}

fn take_fence(lines: &[&str], start: usize) -> Option<(String, String, usize)> {
    let open = lines[start].trim_end();
    let rest = open.strip_prefix("```")?;
    let lang = rest.trim().to_string();
    let mut body = String::new();
    let mut i = start + 1;
    while i < lines.len() {
        if lines[i].trim_end().starts_with("```") {
            return Some((lang, body, i + 1));
        }
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(lines[i]);
        i += 1;
    }
    None
}

fn heading_line(line: &str) -> Option<(u8, &str)> {
    let t = line.trim_start();
    let rest = t.strip_prefix('#')?;
    let mut level = 1u8;
    let mut s = rest;
    while let Some(r) = s.strip_prefix('#') {
        level += 1;
        if level > 6 {
            return None;
        }
        s = r;
    }
    let s = s.strip_prefix(' ')?;
    Some((level, s.trim_end()))
}

fn quote_line(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let rest = t.strip_prefix('>')?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

fn list_line(line: &str) -> Option<(bool, &str)> {
    let t = line.trim_start();
    for p in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(p) {
            return Some((false, rest));
        }
    }
    let bytes = t.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && t[i..].starts_with(". ") {
        return Some((true, &t[i + 2..]));
    }
    None
}

fn take_quoted(lines: &[&str], start: usize, first: &str) -> (String, usize) {
    let mut body = first.to_string();
    let mut i = start + 1;
    while i < lines.len() {
        let Some(rest) = quote_line(lines[i].trim_end()) else {
            break;
        };
        body.push('\n');
        body.push_str(rest);
        i += 1;
    }
    (body, i)
}

fn take_list(
    lines: &[&str],
    start: usize,
    ordered: bool,
    first: &str,
) -> (Vec<Vec<ProseRun>>, usize) {
    let mut items = vec![parse_inlines(first)];
    let mut i = start + 1;
    while i < lines.len() {
        let trimmed = lines[i].trim_end();
        if trimmed.trim().is_empty() {
            break;
        }
        match list_line(trimmed) {
            Some((ord, item)) if ord == ordered => items.push(parse_inlines(item)),
            _ => break,
        }
        i += 1;
    }
    (items, i)
}

fn take_paragraph(lines: &[&str], start: usize) -> (String, usize) {
    let mut body = lines[start].trim_end().to_string();
    let mut i = start + 1;
    while i < lines.len() {
        let trimmed = lines[i].trim_end();
        if trimmed.trim().is_empty() {
            break;
        }
        if heading_line(trimmed).is_some()
            || quote_line(trimmed).is_some()
            || list_line(trimmed).is_some()
            || trimmed.trim_start().starts_with("```")
        {
            break;
        }
        body.push('\n');
        body.push_str(trimmed);
        i += 1;
    }
    (body, i)
}

fn parse_inlines(src: &str) -> Vec<ProseRun> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    let mut buf = String::new();
    let flush_plain = |buf: &mut String, out: &mut Vec<ProseRun>| {
        if buf.is_empty() {
            return;
        }
        out.extend(runs_from_text(&std::mem::take(buf)));
    };
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            buf.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if chars[i] == '`' {
            if let Some((code, next)) = take_delimited(&chars, i, "`") {
                flush_plain(&mut buf, &mut out);
                out.push(ProseRun {
                    text: code,
                    url: None,
                    bold: false,
                    italic: false,
                    code: true,
                });
                i = next;
                continue;
            }
        }
        if chars[i] == '[' {
            if let Some((label, url, next)) = take_link(&chars, i) {
                flush_plain(&mut buf, &mut out);
                out.push(ProseRun::link(label, url));
                i = next;
                continue;
            }
        }
        if let Some((mark, n)) = delim_at(&chars, i) {
            if let Some((inner, next)) = take_delimited(&chars, i, mark) {
                flush_plain(&mut buf, &mut out);
                let (bold, italic) = match n {
                    3 => (true, true),
                    2 => (true, false),
                    _ => (false, true),
                };
                let mut inner_runs = parse_inlines(&inner);
                if inner_runs.is_empty() {
                    inner_runs.push(ProseRun::text(inner));
                }
                for r in &mut inner_runs {
                    r.bold |= bold;
                    r.italic |= italic;
                }
                out.extend(inner_runs);
                i = next;
                continue;
            }
        }
        buf.push(chars[i]);
        i += 1;
    }
    flush_plain(&mut buf, &mut out);
    if out.is_empty() {
        out.push(ProseRun::text(src.to_string()));
    }
    out
}

fn delim_at(chars: &[char], i: usize) -> Option<(&'static str, usize)> {
    let c = chars[i];
    if c != '*' && c != '_' {
        return None;
    }
    let mut n = 0;
    while i + n < chars.len() && chars[i + n] == c {
        n += 1;
        if n == 3 {
            break;
        }
    }
    if n == 0 {
        return None;
    }
    let mark: &'static str = match (c, n) {
        ('*', 3) => "***",
        ('*', 2) => "**",
        ('*', 1) => "*",
        ('_', 3) => "___",
        ('_', 2) => "__",
        ('_', 1) => "_",
        _ => return None,
    };
    Some((mark, n))
}

fn take_delimited(chars: &[char], start: usize, mark: &str) -> Option<(String, usize)> {
    let m: Vec<char> = mark.chars().collect();
    if start + m.len() > chars.len() {
        return None;
    }
    if chars[start..start + m.len()] != m[..] {
        return None;
    }
    let inner_at = start + m.len();
    let mut i = inner_at;
    while i + m.len() <= chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            i += 2;
            continue;
        }
        if chars[i..i + m.len()] == m[..] {
            if i == inner_at {
                return None;
            }
            let inner: String = chars[inner_at..i].iter().collect();
            return Some((inner, i + m.len()));
        }
        i += 1;
    }
    None
}

fn take_link(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    if chars[start] != '[' {
        return None;
    }
    let mut i = start + 1;
    while i < chars.len() && chars[i] != ']' {
        i += 1;
    }
    if i >= chars.len() || i + 1 >= chars.len() || chars[i + 1] != '(' {
        return None;
    }
    let label: String = chars[start + 1..i].iter().collect();
    i += 2;
    let url_at = i;
    while i < chars.len() && chars[i] != ')' {
        i += 1;
    }
    if i >= chars.len() {
        return None;
    }
    let url: String = chars[url_at..i].iter().collect();
    if url.is_empty() {
        return None;
    }
    Some((label, url, i + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stanza_keeps_line_breaks() {
        let blocks = parse_markdown("Roses are red\nViolets are blue\n\nSugar is sweet");
        assert_eq!(blocks.len(), 2);
        let ProseBlock::Paragraph(runs) = &blocks[0] else {
            panic!("{:?}", blocks[0]);
        };
        let t: String = runs.iter().map(|r| r.text.as_str()).collect();
        assert!(t.contains("Roses are red\nViolets are blue"), "{t:?}");
        assert!(!t.contains("red Violets"));
    }

    #[test]
    fn verse_fence_is_verse_not_code() {
        let src = "```verse\nHello darkness\nMy old friend\n```";
        let blocks = parse_markdown(src);
        match &blocks[0] {
            ProseBlock::Code { text, verse } => {
                assert!(*verse);
                assert_eq!(text, "Hello darkness\nMy old friend");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn bold_and_italic() {
        let blocks = parse_markdown("Say **hello** and *softly*");
        let ProseBlock::Paragraph(runs) = &blocks[0] else {
            panic!();
        };
        assert!(runs.iter().any(|r| r.bold && r.text == "hello"));
        assert!(runs.iter().any(|r| r.italic && r.text == "softly"));
    }

    #[test]
    fn heading_and_list() {
        let blocks = parse_markdown("# Title\n\n- one\n- two");
        assert!(matches!(blocks[0], ProseBlock::Heading { level: 1, .. }));
        match &blocks[1] {
            ProseBlock::List { ordered, items } => {
                assert!(!*ordered);
                assert_eq!(items.len(), 2);
            }
            other => panic!("{other:?}"),
        }
    }
}
