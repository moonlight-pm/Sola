//! Icon atom (`data-kind=icon`).

use crate::dom::Elem;
use crate::markup;

pub fn icon(next: &mut u32, name: &str, extra: &[&str]) -> Elem {
    let mut classes = vec!["icon"];
    classes.extend_from_slice(extra);
    let mut el = markup::node(next, &classes, None, None, "");
    el.data_kind = Some("icon".into());
    el.data_id = Some(name.into());
    el
}
