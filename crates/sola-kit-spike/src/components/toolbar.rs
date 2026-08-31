//! Toolbar strip.

use crate::dom::Elem;
use crate::markup;

pub fn bar(next: &mut u32, id: &str, extra: &[&str], kids: Vec<Elem>) -> Elem {
    let mut classes: Vec<&str> = if extra.is_empty() {
        vec!["toolbar"]
    } else {
        extra.to_vec()
    };
    if !classes
        .iter()
        .any(|c| *c == "toolbar" || c.ends_with("-toolbar"))
    {
        classes.insert(0, "toolbar");
    }
    let mut el = markup::node(next, &classes, None, Some(id), "");
    el.children = kids;
    el
}

/// Icon-only toolbar control. Omit `action` to mute (no hit).
pub fn icon_btn(next: &mut u32, action: Option<&str>, id: Option<&str>, icon_name: &str) -> Elem {
    let mut classes = vec!["toolbar-icon"];
    if action.is_none() {
        classes.push("is-disabled");
    }
    let mut el = markup::node(next, &classes, action, id, "");
    el.children
        .push(crate::components::icon::icon(next, icon_name, &["icon-16"]));
    el
}
