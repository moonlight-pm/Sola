//! Calendar location/notes → kit prose (HTML `<a href>` and bare URLs).

use sola_kit::components::prose::{ProseBlock, ProseRun, parse_plain};

pub fn to_blocks(raw: &str) -> Vec<ProseBlock> {
    let t = raw.trim();
    if t.is_empty() {
        return Vec::new();
    }
    if looks_html(t) {
        let runs = html_runs(t);
        if runs.iter().any(|r| !r.text.trim().is_empty() || r.url.is_some()) {
            return vec![ProseBlock::Paragraph(runs)];
        }
    }
    parse_plain(raw)
}

fn looks_html(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.contains("<a ") || l.contains("<a>") || l.contains("</a>") || l.contains("<br")
}

fn html_runs(s: &str) -> Vec<ProseRun> {
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        match rest.find('<') {
            None => {
                push_text(&mut out, rest);
                break;
            }
            Some(0) => {
                if let Some((run, after)) = take_anchor(rest) {
                    out.push(run);
                    rest = after;
                    continue;
                }
                if let Some((nl, after)) = take_break(rest) {
                    if nl {
                        push_text(&mut out, "\n");
                    }
                    rest = after;
                    continue;
                }
                if let Some(after) = skip_tag(rest) {
                    rest = after;
                    continue;
                }
                push_text(&mut out, rest);
                break;
            }
            Some(i) => {
                push_text(&mut out, &rest[..i]);
                rest = &rest[i..];
            }
        }
    }
    out
}

fn push_text(out: &mut Vec<ProseRun>, raw: &str) {
    let decoded = decode_entities(raw);
    if decoded.is_empty() {
        return;
    }
    for run in linkify(&decoded) {
        if let Some(last) = out.last_mut() {
            if last.url.is_none() && run.url.is_none() {
                last.text.push_str(&run.text);
                continue;
            }
        }
        out.push(run);
    }
}

fn linkify(s: &str) -> Vec<ProseRun> {
    match parse_plain(s).into_iter().next() {
        Some(ProseBlock::Paragraph(runs)) if !runs.is_empty() => runs,
        _ => {
            if s.trim().is_empty() {
                Vec::new()
            } else {
                vec![ProseRun::text(s)]
            }
        }
    }
}

fn take_anchor(s: &str) -> Option<(ProseRun, &str)> {
    let body = s.strip_prefix('<')?;
    let body = match_tag_name(body, "a")?;
    let (attrs, after_gt) = split_tag_end(body)?;
    let href = attr(attrs, "href")?;
    let close = after_gt
        .to_ascii_lowercase()
        .find("</a>")
        .map(|i| (i, 4))?;
    let inner = &after_gt[..close.0];
    let after = &after_gt[close.0 + close.1..];
    let label = decode_entities(&strip_tags(inner));
    let href = decode_entities(&href);
    let label = if label.trim().is_empty() {
        href.clone()
    } else {
        label
    };
    Some((ProseRun::link(label.trim(), href.trim()), after))
}

fn match_tag_name<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(name).or_else(|| {
        let upper = name.to_ascii_uppercase();
        s.strip_prefix(&upper)
    })?;
    if rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace() || c == '/' || c == '>') {
        Some(rest)
    } else {
        None
    }
}

fn split_tag_end(s: &str) -> Option<(&str, &str)> {
    let i = s.find('>')?;
    Some((&s[..i], &s[i + 1..]))
}

fn take_break(s: &str) -> Option<(bool, &str)> {
    let body = s.strip_prefix('<')?;
    let lower = body.to_ascii_lowercase();
    if lower.starts_with("br") {
        let after = skip_tag(s)?;
        return Some((true, after));
    }
    if lower.starts_with("p") && (lower.len() == 1 || lower[1..].starts_with(|c: char| c.is_whitespace() || c == '/' || c == '>'))
        || lower.starts_with("/p")
    {
        let after = skip_tag(s)?;
        return Some((true, after));
    }
    None
}

fn skip_tag(s: &str) -> Option<&str> {
    if !s.starts_with('<') {
        return None;
    }
    let i = s.find('>')?;
    Some(&s[i + 1..])
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let key = format!("{name}=");
    let idx = lower.find(&key)?;
    let val = attrs[idx + key.len()..].trim_start();
    match val.chars().next() {
        Some(q @ '"' | q @ '\'') => {
            let rest = &val[q.len_utf8()..];
            let end = rest.find(q)?;
            Some(rest[..end].to_string())
        }
        Some(_) => Some(
            val.split(|c: char| c.is_whitespace() || c == '>' || c == '/')
                .next()
                .unwrap_or("")
                .to_string(),
        ),
        None => None,
    }
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        rest = skip_tag(&rest[i..]).unwrap_or("");
    }
    out.push_str(rest);
    out
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
    use super::to_blocks;
    use sola_kit::components::prose::ProseBlock;

    fn first_runs(raw: &str) -> Vec<(String, Option<String>)> {
        match to_blocks(raw).into_iter().next() {
            Some(ProseBlock::Paragraph(runs)) => {
                runs.into_iter().map(|r| (r.text, r.url)).collect()
            }
            _ => Vec::new(),
        }
    }

    #[test]
    fn html_anchor_becomes_link() {
        let runs = first_runs(
            r#"<a href="https://meet.jit.si/EXITTechCall">https://meet.jit.si/EXITTechCall</a>"#,
        );
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].0, "https://meet.jit.si/EXITTechCall");
        assert_eq!(
            runs[0].1.as_deref(),
            Some("https://meet.jit.si/EXITTechCall")
        );
    }

    #[test]
    fn html_anchor_uses_label() {
        let runs = first_runs(r#"Join <a href="https://meet.jit.si/x">the call</a> now"#);
        assert!(runs.iter().any(|(t, u)| t == "the call" && u.as_deref() == Some("https://meet.jit.si/x")));
    }

    #[test]
    fn bare_url_still_links() {
        let runs = first_runs("https://example.com/room");
        assert_eq!(
            runs[0].1.as_deref(),
            Some("https://example.com/room")
        );
    }
}
