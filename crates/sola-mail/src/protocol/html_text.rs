//! HTML message bodies → readable letter blocks (no HTML engine in the UI).

use html2text::render::{RichAnnotation, TaggedLine};
use sola_kit::components::prose::{flatten, parse_plain, ProseBlock, ProseRun};

/// Convert HTML mail into plain text suitable for copy / reply quoting.
#[allow(dead_code)] // used by tests; reading pane goes through `to_blocks`.
pub fn to_plain(html: &str) -> String {
    flatten(&to_blocks(html))
}

/// Convert HTML into kit prose blocks.
///
/// Marketing mail is table-heavy and stuffed with tracking URLs. We:
/// - flatten tables as stacked cells (no ASCII borders)
/// - keep each short cell as its own paragraph
/// - show link *labels*, never the raw tracking href
pub fn to_blocks(html: &str) -> Vec<ProseBlock> {
    let lines = html2text::config::rich()
        .raw_mode(true)
        .no_link_wrapping()
        .lines_from_read(html.as_bytes(), 120);
    match lines {
        Ok(lines) => {
            let blocks = rich_to_blocks(lines);
            if blocks.is_empty() {
                parse_plain(&crude_strip(html))
            } else {
                blocks
            }
        }
        Err(_) => parse_plain(&crude_strip(html)),
    }
}

fn rich_to_blocks(lines: Vec<TaggedLine<Vec<RichAnnotation>>>) -> Vec<ProseBlock> {
    let mut blocks = Vec::new();
    let mut para: Vec<ProseRun> = Vec::new();
    let mut quote: Vec<ProseRun> = Vec::new();

    let flush_para = |blocks: &mut Vec<ProseBlock>, runs: &mut Vec<ProseRun>| {
        let cleaned = humanize_runs(std::mem::take(runs));
        if runs_have_words(&cleaned) {
            blocks.push(ProseBlock::Paragraph(cleaned));
        }
    };
    let flush_quote = |blocks: &mut Vec<ProseBlock>, runs: &mut Vec<ProseRun>| {
        let cleaned = humanize_runs(std::mem::take(runs));
        if runs_have_words(&cleaned) {
            blocks.push(ProseBlock::Quote(cleaned));
        }
    };

    for line in lines {
        let mut runs: Vec<ProseRun> = Vec::new();
        let mut line_text = String::new();
        for ts in line.tagged_strings() {
            if ts.s.is_empty() {
                continue;
            }
            line_text.push_str(&ts.s);
            let url = ts.tag.iter().find_map(|ann| match ann {
                RichAnnotation::Link(u) if is_http_url(u) => Some(u.clone()),
                _ => None,
            });
            if let Some(last) = runs.last_mut() {
                if last.url == url {
                    last.text.push_str(&ts.s);
                    continue;
                }
            }
            runs.push(ProseRun {
                text: ts.s.clone(),
                url,
            });
        }

        let trimmed = line_text.trim();
        if trimmed.is_empty() {
            flush_para(&mut blocks, &mut para);
            flush_quote(&mut blocks, &mut quote);
            continue;
        }

        if trimmed.starts_with('>') {
            flush_para(&mut blocks, &mut para);
            strip_leading_quote(&mut runs);
            if !quote.is_empty() {
                quote.push(ProseRun::text("\n"));
            }
            quote.extend(runs);
            continue;
        }

        flush_quote(&mut blocks, &mut quote);
        // Long html2text wraps stay one paragraph. Short table cells
        // (the usual marketing-mail case) each become their own.
        let prev_len = para.iter().map(|r| r.text.len()).sum::<usize>();
        let prev_end = para
            .iter()
            .rev()
            .find_map(|r| r.text.chars().rev().find(|c| !c.is_whitespace()));
        let looks_wrap = prev_len > 70 && !matches!(prev_end, Some('.' | '!' | '?'));
        if !para.is_empty() && looks_wrap {
            para.push(ProseRun::text(" "));
            para.extend(runs);
        } else {
            flush_para(&mut blocks, &mut para);
            para = runs;
        }
    }
    flush_para(&mut blocks, &mut para);
    flush_quote(&mut blocks, &mut quote);
    collapse_blank_blocks(blocks)
}

/// Replace tracking-URL labels with a short host / "Link".
fn humanize_runs(runs: Vec<ProseRun>) -> Vec<ProseRun> {
    let mut out = Vec::new();
    for run in runs {
        if let Some(url) = run.url.clone() {
            let label = display_link_label(&run.text, &url);
            if label.is_empty() {
                continue;
            }
            out.push(ProseRun::link(label, url));
        } else {
            let t = run.text.replace(['(', ')'], " ");
            if t.trim().is_empty() {
                continue;
            }
            if looks_like_raw_url(t.trim()) {
                continue;
            }
            out.push(ProseRun::text(t));
        }
    }
    // Drop a paragraph that is only tracking links with no prose.
    let has_prose = out.iter().any(|r| r.url.is_none() && r.text.chars().any(|c| c.is_alphabetic()));
    let only_tracking = !has_prose
        && out.iter().all(|r| r.url.as_deref().is_some_and(is_tracking_url));
    if only_tracking {
        return Vec::new();
    }
    out
}

pub fn display_link_label(text: &str, url: &str) -> String {
    let t = text.trim();
    if t.is_empty() || looks_like_raw_url(t) || is_tracking_url(t) || t.len() > 48 {
        return short_host_label(url);
    }
    t.to_string()
}

fn short_host_label(url: &str) -> String {
    let stripped = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let host = stripped.split('/').next().unwrap_or(stripped);
    let host = host.split(':').next().unwrap_or(host);
    let host = host.trim_start_matches("www.");
    if host.is_empty() || is_tracking_host(host) {
        return "Link".into();
    }
    host.to_string()
}

fn looks_like_raw_url(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://") || l.starts_with("www.")
}

fn is_tracking_url(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.contains("upn=")
        || l.contains("utm_")
        || l.contains("/ls/click")
        || l.contains("list-manage")
        || s.len() > 96
}

fn is_tracking_host(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h.contains("ablink")
        || h.contains("click.")
        || h.starts_with("click")
        || h.contains("track")
        || h.contains("email.")
}

fn runs_have_words(runs: &[ProseRun]) -> bool {
    runs.iter().any(|r| r.text.chars().any(|c| !c.is_whitespace()))
}

fn strip_leading_quote(runs: &mut [ProseRun]) {
    if let Some(first) = runs.first_mut() {
        let t = first.text.trim_start();
        if let Some(rest) = t.strip_prefix('>') {
            first.text = rest.strip_prefix(' ').unwrap_or(rest).to_string();
        }
    }
}

fn collapse_blank_blocks(blocks: Vec<ProseBlock>) -> Vec<ProseBlock> {
    blocks
        .into_iter()
        .filter(|b| match b {
            ProseBlock::Paragraph(runs) | ProseBlock::Quote(runs) => runs_have_words(runs),
        })
        .collect()
}

fn crude_strip(html: &str) -> String {
    html.replace('<', " <")
        .split('<')
        .map(|chunk| chunk.split('>').nth(1).unwrap_or(chunk))
        .collect::<String>()
}

fn is_http_url(u: &str) -> bool {
    let l = u.to_ascii_lowercase();
    (l.starts_with("http://") || l.starts_with("https://")) && u.len() > 10
}

/// True when the multipart/plain part is a generator stub and the HTML
/// is the real letter.
#[allow(dead_code)] // reading pane always prefers HTML; kept for tests / callers.
pub fn plain_looks_like_stub(plain: &str, html: &str) -> bool {
    let plain = plain.trim();
    if plain.is_empty() {
        return !html.trim().is_empty();
    }
    let lower = plain.to_ascii_lowercase();
    if lower.contains("this is an html")
        || (lower.contains("html")
            && (lower.contains("view") || lower.contains("browser") || lower.contains("requires")))
    {
        return true;
    }
    // URL-dense plaintext is the ESP dump; HTML has the real letter.
    let url_hits = lower.matches("http://").count() + lower.matches("https://").count();
    if url_hits >= 3 && html.len() > 40 {
        return true;
    }
    html.len() > plain.len().saturating_mul(3) && plain.len() < 400
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_simple_markup() {
        let plain = to_plain("<p>Hello <b>world</b></p>");
        assert!(plain.to_lowercase().contains("hello"));
        assert!(plain.to_lowercase().contains("world"));
        assert!(!plain.contains("<b>"));
    }

    #[test]
    fn preserves_link_destination() {
        let blocks = to_blocks(r#"<p>Click <a href="https://example.com/path">here</a> now</p>"#);
        let has_link = blocks.iter().any(|b| match b {
            ProseBlock::Paragraph(runs) | ProseBlock::Quote(runs) => runs
                .iter()
                .any(|r| r.url.as_deref() == Some("https://example.com/path")),
        });
        assert!(has_link, "{blocks:?}");
        let plain = flatten(&blocks);
        assert!(
            plain.contains("example.com") || plain.contains("here"),
            "got: {plain:?}"
        );
    }

    #[test]
    fn tracking_href_is_not_the_visible_label() {
        let href = "https://ablink.news.gemini.com/ls/click?upn=u001.SWvH-2F-2Fdx6zVERYLONGTOKEN";
        let html = format!(r#"<p>Trade <a href="{href}">{href}</a> now</p>"#);
        let blocks = to_blocks(&html);
        let labels: Vec<String> = blocks
            .iter()
            .flat_map(|b| match b {
                ProseBlock::Paragraph(runs) | ProseBlock::Quote(runs) => runs.iter(),
            })
            .filter(|r| r.url.is_some())
            .map(|r| r.text.clone())
            .collect();
        assert!(
            labels.iter().all(|l| !l.contains("upn=") && l.len() < 40),
            "labels={labels:?}"
        );
        assert!(
            labels.iter().any(|l| l == "Link" || !l.contains("http")),
            "labels={labels:?}"
        );
    }

    #[test]
    fn stub_plain_prefers_html() {
        assert!(plain_looks_like_stub(
            "This is an HTML email. View it in a browser.",
            &"<p>".repeat(80)
        ));
        assert!(plain_looks_like_stub(
            "see https://a.com/x https://b.com/y https://c.com/z and more tracking",
            "<p>A real letter with a <a href=\"https://example.com\">button</a></p>"
        ));
        assert!(!plain_looks_like_stub(
            "A real plaintext letter with enough body that we should keep it.",
            "<p>also html</p>"
        ));
    }
}
