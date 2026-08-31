//! Select + hanging menu.

use crate::components::icon;
use crate::dom::Elem;
use crate::markup;

pub fn select(next: &mut u32, id: &str, extra_class: Option<&str>, label_bind: &str) -> Elem {
    let classes: Vec<&str> = match extra_class {
        Some(c) => vec!["select", c],
        None => vec!["select"],
    };
    let mut el = markup::node(next, &classes, Some("select-toggle"), Some(id), "");
    let mut label = markup::node(next, &["select-label"], None, None, "");
    label.data_bind = Some(label_bind.into());
    el.children.push(label);
    el.children
        .push(icon::icon(next, "lucide/chevron-down", &["chevron"]));
    let mut menu = markup::node(next, &["menu", "menu-hang"], None, None, "");
    menu.data_slot = Some("select-menu".into());
    el.children.push(menu);
    el
}

pub fn menu_item(next: &mut u32, action: &str, id: &str, label: &str, active: bool) -> Elem {
    let classes: &[&str] = if active {
        &["menu-item", "is-active"]
    } else {
        &["menu-item"]
    };
    markup::node(next, classes, Some(action), Some(id), label)
}
