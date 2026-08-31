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
