//! Splitter hit bands — same grammar as the storybook Split page.

use crate::dom::Elem;
use crate::markup;

pub fn vertical(next: &mut u32, action: &str, id: &str) -> Elem {
    let mut rule = markup::node(next, &["split-rule", "is-fill"], Some(action), Some(id), "");
    rule.children
        .push(markup::node(next, &["split-line"], None, None, ""));
    rule
}

pub fn horizontal(next: &mut u32, action: &str, id: &str) -> Elem {
    let mut rule = markup::node(
        next,
        &["split-rule-h", "is-fill"],
        Some(action),
        Some(id),
        "",
    );
    rule.children
        .push(markup::node(next, &["split-line-h"], None, None, ""));
    rule
}

pub fn hairline(next: &mut u32) -> Elem {
    markup::node(next, &["hairline"], None, None, "")
}

/// 1px vertical rule between panes.
pub fn vline(next: &mut u32) -> Elem {
    markup::node(next, &["v-hairline"], None, None, "")
}
