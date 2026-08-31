//! Status badge.

use crate::dom::Elem;
use crate::markup;

pub fn badge(next: &mut u32, tone: &str, label: &str) -> Elem {
    markup::node(next, &["badge", tone], None, None, label)
}
