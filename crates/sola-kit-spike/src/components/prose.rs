//! Letter measure: paragraphs, quotes, inline links.
//!
//! Each block is one wrapping column (not a row of runs). A row of flex
//! runs parks leftover words in the gutter — iced letters wrap as
//! paragraphs.

use crate::dom::Elem;
use crate::markup;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub url: Option<String>,
}

impl Run {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Run>),
    Quote(Vec<Run>),
}

/// Column of letter blocks. Links carry `data-action=open-url`.
pub fn document(next: &mut u32, blocks: &[Block]) -> Vec<Elem> {
    blocks.iter().map(|b| block(next, b)).collect()
}

fn block(next: &mut u32, block: &Block) -> Elem {
    let (runs, quote) = match block {
        Block::Paragraph(runs) => (runs.as_slice(), false),
        Block::Quote(runs) => (runs.as_slice(), true),
    };
    let mut classes = vec!["prose-p"];
    if quote {
        classes.push("prose-quote");
    }
    let links_only = !runs.is_empty()
        && runs
            .iter()
            .all(|r| r.url.is_some() && !r.text.trim().is_empty());
    if links_only {
        classes.push("prose-links");
        let mut p = markup::node(next, &classes, Some("prose"), None, "");
        for r in runs {
            p.children.push(link_el(next, r));
        }
        return p;
    }
    let text: String = runs.iter().map(|r| r.text.as_str()).collect();
    let mut p = markup::node(next, &classes, Some("prose"), None, "");
    p.children
        .push(markup::node(next, &["prose-run"], None, None, &text));
    // Mixed text+link: wrap as one paragraph (iced measure). A lone URL
    // in the block stays clickable via the first link run.
    if let Some(run) = runs.iter().find(|r| r.url.is_some()) {
        if runs.iter().filter(|r| r.url.is_some()).count() == 1 && text.trim() == run.text.trim() {
            p.data_action = Some("open-url".into());
            p.data_id = run.url.clone();
        }
    }
    p
}

fn link_el(next: &mut u32, run: &Run) -> Elem {
    markup::node(
        next,
        &["prose-link"],
        Some("open-url"),
        run.url.as_deref(),
        &run.text,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_carry_open_url() {
        let mut n = 1u32;
        let kids = document(
            &mut n,
            &[Block::Paragraph(vec![
                Run::text("Hi "),
                Run::link("here", "https://ex.com/a"),
            ])],
        );
        assert_eq!(kids.len(), 1);
        let p = &kids[0];
        assert!(p.has_class("prose-p"));
        assert_eq!(p.children[0].text, "Hi here");
        assert_eq!(p.data_action.as_deref(), Some("prose"));
    }

    #[test]
    fn link_only_block_is_a_link_row() {
        let mut n = 1u32;
        let kids = document(
            &mut n,
            &[Block::Paragraph(vec![
                Run::link("Unsubscribe", "https://ex.com/u"),
                Run::link("Preferences", "https://ex.com/p"),
            ])],
        );
        assert!(kids[0].has_class("prose-links"));
        assert_eq!(kids[0].children.len(), 2);
        assert_eq!(kids[0].children[0].data_action.as_deref(), Some("open-url"));
    }
}
