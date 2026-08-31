//! Labeled field + input well.

use crate::dom::Elem;
use crate::markup;

pub fn input(next: &mut u32, id: &str) -> Elem {
    let mut el = markup::node(next, &["input"], Some("focus"), Some(id), "");
    el.data_bind = Some(id.into());
    el
}

/// Label stacked above an input. `id` is the focus/bind key.
pub fn stack(next: &mut u32, label: &str, id: &str) -> Elem {
    let mut field = markup::node(next, &["stack-field"], Some("focus"), Some(id), "");
    field
        .children
        .push(markup::node(next, &["stack-label"], None, None, label));
    field.children.push(input(next, id));
    field
}
