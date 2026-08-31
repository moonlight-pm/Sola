//! Buttons — primary / secondary / ghost / danger / toolbar.

use crate::dom::Elem;
use crate::markup;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Primary,
    Secondary,
    Ghost,
    Danger,
    DangerOutline,
    Toolbar,
}

pub fn button(
    next: &mut u32,
    kind: Kind,
    small: bool,
    action: &str,
    id: Option<&str>,
    label: &str,
) -> Elem {
    let mut classes: Vec<&str> = match kind {
        Kind::Toolbar => vec!["toolbar-btn"],
        Kind::Primary => vec!["btn", "btn-primary"],
        Kind::Secondary => vec!["btn", "btn-secondary"],
        Kind::Ghost => vec!["btn", "btn-ghost"],
        Kind::Danger => vec!["btn", "btn-danger"],
        Kind::DangerOutline => vec!["btn", "btn-danger-outline"],
    };
    if small && kind != Kind::Toolbar {
        classes.push("btn-sm");
    }
    markup::node(next, &classes, Some(action), id, label)
}

pub fn row(next: &mut u32, kids: Vec<Elem>) -> Elem {
    let mut el = markup::node(next, &["btn-row"], None, None, "");
    el.children = kids;
    el
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_emit_kit_classes() {
        let mut n = 1u32;
        let p = button(&mut n, Kind::Primary, true, "save", None, "Save");
        assert!(p.has_class("btn") && p.has_class("btn-primary") && p.has_class("btn-sm"));
        let t = button(&mut n, Kind::Toolbar, false, "pause", None, "Pause");
        assert!(t.has_class("toolbar-btn"));
        assert!(!t.has_class("btn-sm"));
    }
}
