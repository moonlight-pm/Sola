//! Status strip with trailing actions (Undo / Dismiss).

use crate::dom::Elem;
use crate::markup;

pub fn bar(next: &mut u32, message: &str, actions: Vec<Elem>) -> Elem {
    let mut el = markup::node(next, &["toast"], None, Some("toast"), "");
    el.children
        .push(markup::node(next, &["t-body"], None, None, message));
    el.children
        .push(markup::node(next, &["spacer"], None, None, ""));
    el.children.extend(actions);
    el
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::button::{Kind, button};

    #[test]
    fn message_then_actions() {
        let mut n = 1u32;
        let dismiss = button(&mut n, Kind::Ghost, true, "toast-dismiss", None, "Dismiss");
        let el = bar(&mut n, "Moved to Archive", vec![dismiss]);
        assert!(el.has_class("toast"));
        assert_eq!(el.children[0].text, "Moved to Archive");
    }
}
