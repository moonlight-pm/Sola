//! Card surface.

use crate::dom::Elem;
use crate::markup;

pub fn card(next: &mut u32, extra: &[&str]) -> Elem {
    let mut classes = vec!["card"];
    classes.extend_from_slice(extra);
    markup::node(next, &classes, None, None, "")
}

pub fn with_title(mut card: Elem, next: &mut u32, title: &str, lede: &str) -> Elem {
    card.children
        .push(markup::node(next, &["card-title"], None, None, title));
    if !lede.is_empty() {
        card.children
            .push(markup::node(next, &["card-body"], None, None, lede));
    }
    card
}

/// Card plus optional title/lede and body children.
pub fn panel(
    next: &mut u32,
    extra: &[&str],
    title: Option<(&str, &str)>,
    kids: Vec<Elem>,
) -> Elem {
    let mut el = match title {
        Some((t, lede)) => with_title(card(next, extra), next, t, lede),
        None => card(next, extra),
    };
    el.children.extend(kids);
    el
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_emits_card_surface() {
        let mut n = 1u32;
        let el = panel(
            &mut n,
            &["settings-list-card"],
            Some(("Account", "IMAP receive")),
            vec![],
        );
        assert!(el.has_class("card"));
        assert!(el.has_class("settings-list-card"));
        let labels: Vec<_> = el.children.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(labels, ["Account", "IMAP receive"]);
    }
}
