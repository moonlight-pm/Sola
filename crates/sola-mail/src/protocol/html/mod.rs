//! HTML letters, read safely: parsed with html5ever into a small DOM
//! ([`dom`]), walked into kit prose blocks ([`walk`]: a safe subset, no
//! scripts, no stylesheets, remote pictures only as references), link
//! labels made readable ([`links`]).

mod dom;
mod links;
mod walk;

use sola_kit::components::prose::parse_plain;

pub use walk::{ImageSource, Walked};

/// The reading pane's blocks and the pictures they show. Prefer HTML
/// whenever it is present — that is the part mail apps render.
pub fn letter(html: Option<&str>, text: &str) -> Walked {
    if let Some(html) = html.filter(|h| !h.trim().is_empty()) {
        // mail-parser lists text/plain parts in `html_body` too and
        // `body_html` then synthesizes `<br/>` HTML. Prefer the real
        // plaintext so bare URLs (Wicket magic links) stay links.
        if !is_synthesized_plain_html(html) || text.trim().is_empty() {
            let mut walked = walk::walk(&dom::parse(html));
            walked.blocks = links::humanize(walked.blocks);
            if !walked.blocks.is_empty() {
                return walked;
            }
        }
    }
    Walked {
        blocks: if text.trim().is_empty() {
            Vec::new()
        } else {
            parse_plain(text)
        },
        images: Vec::new(),
    }
}

/// mail-parser `text_to_html`: `<html><body>…</body></html>` with only `<br/>`.
fn is_synthesized_plain_html(html: &str) -> bool {
    let t = html.trim();
    const OPEN: &str = "<html><body>";
    const CLOSE: &str = "</body></html>";
    if !t.starts_with(OPEN) || !t.ends_with(CLOSE) {
        return false;
    }
    let inner = &t[OPEN.len()..t.len() - CLOSE.len()];
    !inner.replace("<br/>", "").contains('<')
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_kit::components::prose::{ProseBlock, flatten};

    fn link_urls(blocks: &[ProseBlock]) -> Vec<String> {
        blocks
            .iter()
            .flat_map(|b| match b {
                ProseBlock::Paragraph(runs) | ProseBlock::Quote(runs) => runs.clone(),
                _ => Vec::new(),
            })
            .filter_map(|r| r.url)
            .collect()
    }

    /// Wicket magic-link mail is text/plain; mail-parser still feeds
    /// `body_html` via `text_to_html`. The lone long URL must stay a
    /// clickable destination.
    #[test]
    fn synthesized_html_reads_the_plain_text() {
        let token = "A".repeat(43);
        let url = format!("https://auth.naturalethic.com/login/magic/verify?token={token}");
        let text =
            format!("Use this link to sign in to Wicket:\n\n  {url}\n\nIt expires shortly.\n");
        let html = format!("<html><body>{}</body></html>", text.replace('\n', "<br/>"));
        let blocks = letter(Some(&html), &text).blocks;
        assert!(link_urls(&blocks).contains(&url), "{blocks:?}");
    }

    #[test]
    fn real_html_is_walked() {
        let w = letter(
            Some("<html><head><style>p{}</style></head><body><h3>Wagon Rides</h3><p>Noon to 3 pm.</p></body></html>"),
            "",
        );
        assert!(matches!(&w.blocks[0], ProseBlock::Heading { level: 3, .. }));
        assert_eq!(flatten(&w.blocks).trim(), "Wagon Rides\n\nNoon to 3 pm.");
    }

    #[test]
    fn empty_html_falls_back_to_text() {
        let w = letter(Some("<div style=\"display:none\">x</div>"), "plain body");
        assert_eq!(flatten(&w.blocks).trim(), "plain body");
    }
}
